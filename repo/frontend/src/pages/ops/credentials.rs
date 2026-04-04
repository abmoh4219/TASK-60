//! Ops credentials page — contract/credential document management and e-signing.
//!
//! Layout:
//!   • Header  — title, "Upload Document" button, "Run Expiry Sweep" (Admin|Dispatcher)
//!   • Filters — status select
//!   • Split view: credential list  ←→  detail panel
//!     - Detail: info card + Review form (pending + can_approve) + E-sign form + Audit log
//!   • Upload form (shown inline at top when active)
//!
//! RBAC (enforced by backend; mirrored here for UX):
//!   ViewCredentials:   Admin, OpsAgent, Dispatcher
//!   ApproveCredentials: Admin, Dispatcher

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement, HtmlSelectElement, InputEvent};
use yew::prelude::*;

use shared::UserRole;

use crate::api::credentials::{
    self as creds_api, CredentialAuditEntry, CredentialRow, EsignBody, ReviewBody,
};
use crate::app::Route;
use crate::auth::AuthContext;
use crate::components::{
    icons, status_badge, empty_state, skeleton_rows,
    btn_primary, btn_secondary, btn_danger, input_cls, select_cls,
};
use crate::components::toast::{use_toasts, ToastKind};

use super::OpsLayout;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn ev_val(e: Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

fn ev_sel(e: Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

fn fmt_dt(s: &str) -> String {
    if s.len() >= 16 { s[..16].replace('T', " ") } else { s.to_owned() }
}

fn fmt_size(bytes: i32) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

fn render_tab_bar(
    can_approve:    bool,
    action:         UseStateHandle<u8>,
    on_load_audit:  Callback<MouseEvent>,
) -> Html {
    let tabs: &[(&str, u8)] = if can_approve {
        &[("Info", 0), ("Review", 1), ("E-sign", 2), ("Audit", 3)]
    } else {
        &[("Info", 0), ("E-sign", 2), ("Audit", 3)]
    };
    tabs.iter().map(|(label, idx)| {
        let idx = *idx;
        let is_active = *action == idx;
        let on_click = if idx == 3 {
            on_load_audit.clone()
        } else {
            let action = action.clone();
            Callback::from(move |_: MouseEvent| action.set(idx))
        };
        html! {
            <button
                class={if is_active {
                    "px-3 py-2 text-xs font-medium text-indigo-700 \
                     border-b-2 border-indigo-600 transition-colors"
                } else {
                    "px-3 py-2 text-xs font-medium text-slate-500 \
                     border-b-2 border-transparent hover:text-slate-800 \
                     transition-colors"
                }}
                onclick={on_click}>
                { *label }
            </button>
        }
    }).collect::<Html>()
}

fn render_pagination(
    total:  i64,
    page_v: i64,
    page:   UseStateHandle<i64>,
    reload: UseStateHandle<u8>,
) -> Html {
    let total_pages = (total + 19) / 20;
    if total_pages <= 1 {
        return html! {};
    }
    html! {
        <div class="flex items-center justify-between px-4 py-3
                    border-t border-slate-100 text-xs text-slate-500">
            <span>{ format!("Page {page_v} / {total_pages}") }</span>
            <div class="flex gap-2">
                if page_v > 1 {
                    <button
                        class="hover:text-indigo-600 font-medium transition-colors"
                        onclick={{
                            let (page, reload) = (page.clone(), reload.clone());
                            Callback::from(move |_: MouseEvent| {
                                page.set(page_v - 1);
                                reload.set(reload.wrapping_add(1));
                            })
                        }}>
                        {"← Prev"}
                    </button>
                }
                if page_v < total_pages {
                    <button
                        class="hover:text-indigo-600 font-medium transition-colors"
                        onclick={{
                            let (page, reload) = (page.clone(), reload.clone());
                            Callback::from(move |_: MouseEvent| {
                                page.set(page_v + 1);
                                reload.set(reload.wrapping_add(1));
                            })
                        }}>
                        {"Next →"}
                    </button>
                }
            </div>
        </div>
    }
}

// ── Page component ─────────────────────────────────────────────────────────────

#[function_component(OpsCredentialsPage)]
pub fn ops_credentials_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();
    let user  = auth.current_user().unwrap();

    let can_approve = matches!(user.role, UserRole::Admin | UserRole::Dispatcher);
    let toasts = use_toasts();

    // ── List state ─────────────────────────────────────────────────────────
    let creds:   UseStateHandle<Vec<CredentialRow>> = use_state(Vec::new);
    let total:   UseStateHandle<i64>                = use_state(|| 0i64);
    let page:    UseStateHandle<i64>                = use_state(|| 1i64);
    let loading: UseStateHandle<bool>               = use_state(|| true);
    let reload:  UseStateHandle<u8>                 = use_state(|| 0u8);

    let status_filter = use_state(String::new);

    // ── Selection ──────────────────────────────────────────────────────────
    let selected: UseStateHandle<Option<CredentialRow>>     = use_state(|| None);
    let audit:    UseStateHandle<Vec<CredentialAuditEntry>> = use_state(Vec::new);

    // action: 0=info  1=review  2=esign  3=audit
    let action: UseStateHandle<u8> = use_state(|| 0u8);

    // ── Upload form state ──────────────────────────────────────────────────
    let show_upload   = use_state(|| false);
    let up_contractor = use_state(String::new);
    let up_doc_type   = use_state(String::new);
    let up_expires    = use_state(String::new);
    let file_input_ref = use_node_ref();

    // ── Review form state ──────────────────────────────────────────────────
    let rev_status = use_state(|| "approved".to_owned());
    let rev_notes  = use_state(String::new);

    // ── E-sign form state ──────────────────────────────────────────────────
    let esign_name = use_state(String::new);
    let esign_date = use_state(String::new);

    // ── Banners ────────────────────────────────────────────────────────────
    let error:   UseStateHandle<Option<String>> = use_state(|| None);
    let success: UseStateHandle<Option<String>> = use_state(|| None);

    // ── Load credential list ───────────────────────────────────────────────
    {
        let (creds, total, loading, token) =
            (creds.clone(), total.clone(), loading.clone(), token.clone());
        let page_v   = *page;
        let reload_v = *reload;
        let status_s = (*status_filter).clone();
        use_effect_with((page_v, reload_v, status_s.clone()), move |_| {
            let status_opt: Option<String> =
                if status_s.is_empty() { None } else { Some(status_s) };
            spawn_local(async move {
                match creds_api::list_credentials(
                    &token,
                    None,
                    status_opt.as_deref(),
                    page_v,
                ).await {
                    Ok(r) => {
                        creds.set(r.items);
                        total.set(r.total);
                        loading.set(false);
                    }
                    Err(_) => { loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Select credential ──────────────────────────────────────────────────
    let on_select = {
        let (selected, action, audit, error) =
            (selected.clone(), action.clone(), audit.clone(), error.clone());
        Callback::from(move |cred: CredentialRow| {
            action.set(0);
            audit.set(vec![]);
            error.set(None);
            selected.set(Some(cred));
        })
    };

    // ── Load audit log ─────────────────────────────────────────────────────
    let on_load_audit = {
        let (audit, action, error, token) =
            (audit.clone(), action.clone(), error.clone(), token.clone());
        let sel = selected.clone();
        Callback::from(move |_: MouseEvent| {
            let id = match &*sel { Some(c) => c.id, None => return };
            let (audit, action, error, token) =
                (audit.clone(), action.clone(), error.clone(), token.clone());
            spawn_local(async move {
                match creds_api::get_credential_audit(&token, id).await {
                    Ok(entries) => { audit.set(entries); action.set(3); }
                    Err(e)      => { error.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Review action ──────────────────────────────────────────────────────
    let on_review = {
        let (selected, rev_status, rev_notes, reload, success, error, action, token) = (
            selected.clone(), rev_status.clone(), rev_notes.clone(),
            reload.clone(), success.clone(), error.clone(), action.clone(), token.clone(),
        );
        let toast_push = toasts.push.clone();
        Callback::from(move |_: MouseEvent| {
            let id = match &*selected { Some(c) => c.id, None => return };
            let body = ReviewBody {
                status:       (*rev_status).clone(),
                review_notes: {
                    let n = (*rev_notes).trim().to_owned();
                    if n.is_empty() { None } else { Some(n) }
                },
            };
            let (reload, success, error, action, token, toast_push) = (
                reload.clone(), success.clone(), error.clone(), action.clone(),
                token.clone(), toast_push.clone(),
            );
            let status_label = body.status.clone();
            spawn_local(async move {
                match creds_api::review_credential(&token, id, &body).await {
                    Ok(_) => {
                        let msg = format!("Credential {}.", status_label);
                        success.set(Some(msg.clone()));
                        toast_push.emit((msg, ToastKind::Success));
                        action.set(0);
                        reload.set(reload.wrapping_add(1));
                    }
                    Err(e) => {
                        let msg = e.message.clone();
                        error.set(Some(e.message));
                        toast_push.emit((msg, ToastKind::Error));
                    }
                }
            });
        })
    };

    // ── E-sign action ──────────────────────────────────────────────────────
    let on_esign = {
        let (selected, esign_name, esign_date, reload, success, error, action, token) = (
            selected.clone(), esign_name.clone(), esign_date.clone(),
            reload.clone(), success.clone(), error.clone(), action.clone(), token.clone(),
        );
        let toast_push = toasts.push.clone();
        Callback::from(move |_: MouseEvent| {
            let id = match &*selected { Some(c) => c.id, None => return };
            let name = (*esign_name).trim().to_owned();
            let date = (*esign_date).trim().to_owned();
            if name.is_empty() || date.is_empty() {
                error.set(Some("Signer name and date are required.".into()));
                return;
            }
            let body  = EsignBody { signer_name: name, signed_date: date };
            let (reload, success, error, action, token, toast_push) = (
                reload.clone(), success.clone(), error.clone(), action.clone(),
                token.clone(), toast_push.clone(),
            );
            spawn_local(async move {
                match creds_api::esign_credential(&token, id, &body).await {
                    Ok(_) => {
                        success.set(Some("E-signature recorded.".into()));
                        toast_push.emit(("E-signature recorded.".into(), ToastKind::Success));
                        action.set(0);
                        reload.set(reload.wrapping_add(1));
                    }
                    Err(e) => {
                        let msg = e.message.clone();
                        error.set(Some(e.message));
                        toast_push.emit((msg, ToastKind::Error));
                    }
                }
            });
        })
    };

    // ── Run expiry sweep ───────────────────────────────────────────────────
    let on_sweep = {
        let (reload, success, error, token) =
            (reload.clone(), success.clone(), error.clone(), token.clone());
        let toast_push = toasts.push.clone();
        Callback::from(move |_: MouseEvent| {
            let (reload, success, error, token, toast_push) =
                (reload.clone(), success.clone(), error.clone(), token.clone(), toast_push.clone());
            spawn_local(async move {
                match creds_api::run_expiry_sweep(&token).await {
                    Ok(v) => {
                        let n   = v["expired_count"].as_u64().unwrap_or(0);
                        let msg = format!("Sweep complete — {n} credential(s) expired.");
                        success.set(Some(msg.clone()));
                        toast_push.emit((msg, ToastKind::Info));
                        reload.set(reload.wrapping_add(1));
                    }
                    Err(e) => {
                        let msg = e.message.clone();
                        error.set(Some(e.message));
                        toast_push.emit((msg, ToastKind::Error));
                    }
                }
            });
        })
    };

    // ── Upload submit ──────────────────────────────────────────────────────
    let on_upload = {
        let (file_input_ref, up_contractor, up_doc_type, up_expires) = (
            file_input_ref.clone(), up_contractor.clone(),
            up_doc_type.clone(), up_expires.clone(),
        );
        let (reload, success, error, show_upload, token) = (
            reload.clone(), success.clone(), error.clone(),
            show_upload.clone(), token.clone(),
        );
        let toast_push = toasts.push.clone();
        Callback::from(move |_: MouseEvent| {
            let input = match file_input_ref.cast::<HtmlInputElement>() {
                Some(el) => el,
                None     => { error.set(Some("File input not found.".into())); return; }
            };
            let files = match input.files() {
                Some(f) if f.length() > 0 => f,
                _ => { error.set(Some("Please select a file.".into())); return; }
            };
            let file = match files.get(0) {
                Some(f) => f,
                None    => { error.set(Some("No file selected.".into())); return; }
            };
            let contractor = (*up_contractor).trim().to_owned();
            let doc_type   = (*up_doc_type).trim().to_owned();
            let expires    = (*up_expires).trim().to_owned();
            if contractor.is_empty() {
                error.set(Some("Contractor ID is required.".into())); return;
            }
            if doc_type.is_empty() {
                error.set(Some("Document type is required.".into())); return;
            }
            let form = match web_sys::FormData::new() {
                Ok(f)  => f,
                Err(_) => { error.set(Some("Failed to create form.".into())); return; }
            };
            let blob: &web_sys::Blob = file.as_ref();
            let _ = form.append_with_blob_and_filename("file", blob, &file.name());
            let _ = form.append_with_str("contractor_id", &contractor);
            let _ = form.append_with_str("document_type", &doc_type);
            if !expires.is_empty() {
                let _ = form.append_with_str("expires_at", &expires);
            }
            let (reload, success, error, show_upload, token, toast_push) = (
                reload.clone(), success.clone(), error.clone(),
                show_upload.clone(), token.clone(), toast_push.clone(),
            );
            spawn_local(async move {
                match creds_api::upload_credential(&token, form).await {
                    Ok(_) => {
                        success.set(Some("Document uploaded.".into()));
                        toast_push.emit(("Document uploaded successfully.".into(), ToastKind::Success));
                        show_upload.set(false);
                        reload.set(reload.wrapping_add(1));
                    }
                    Err(e) => {
                        let msg = e.message.clone();
                        error.set(Some(e.message));
                        toast_push.emit((msg, ToastKind::Error));
                    }
                }
            });
        })
    };

    // ── Render ─────────────────────────────────────────────────────────────
    html! {
        <>
        <OpsLayout active={Route::OpsCredentials}>
            <div class="space-y-5">

                // ── Page header ────────────────────────────────────────────
                <div class="flex items-center justify-between gap-4">
                    <div>
                        <h1 class="text-lg font-semibold text-slate-900">
                            {"Credential Documents"}
                        </h1>
                        <p class="text-sm text-slate-500 mt-0.5">
                            { format!("{} credential(s) total", *total) }
                        </p>
                    </div>
                    <div class="flex gap-2 shrink-0">
                        <button
                            class={btn_secondary()}
                            onclick={{
                                let show_upload = show_upload.clone();
                                let su = *show_upload;
                                Callback::from(move |_: MouseEvent| show_upload.set(!su))
                            }}>
                            { if *show_upload {
                                icons::x_mark("w-4 h-4")
                            } else {
                                icons::cloud_arrow_up("w-4 h-4")
                            } }
                            { if *show_upload { "Cancel" } else { "Upload Document" } }
                        </button>
                        if can_approve {
                            <button
                                class={btn_danger()}
                                onclick={on_sweep}>
                                { icons::clock("w-4 h-4") }
                                {"Run Expiry Sweep"}
                            </button>
                        }
                    </div>
                </div>

                // ── Global banners ────────────────────────────────────────
                if let Some(msg) = (*error).clone() {
                    <div class="flex items-start gap-3 rounded-lg bg-red-50 border border-red-200 px-4 py-3">
                        <span class="text-red-500 shrink-0">
                            { icons::exclamation_triangle("w-4 h-4") }
                        </span>
                        <p class="flex-1 text-sm text-red-700">{ &msg }</p>
                        <button
                            onclick={{ let e = error.clone(); Callback::from(move |_: MouseEvent| e.set(None)) }}
                            class="text-red-400 hover:text-red-600 shrink-0">
                            { icons::x_mark("w-4 h-4") }
                        </button>
                    </div>
                }
                if let Some(msg) = (*success).clone() {
                    <div class="flex items-start gap-3 rounded-lg bg-emerald-50 border border-emerald-200 px-4 py-3">
                        <span class="text-emerald-500 shrink-0">
                            { icons::check_circle("w-4 h-4") }
                        </span>
                        <p class="flex-1 text-sm text-emerald-700">{ &msg }</p>
                        <button
                            onclick={{ let s = success.clone(); Callback::from(move |_: MouseEvent| s.set(None)) }}
                            class="text-emerald-400 hover:text-emerald-600 shrink-0">
                            { icons::x_mark("w-4 h-4") }
                        </button>
                    </div>
                }

                // ── Upload form ────────────────────────────────────────────
                if *show_upload {
                    <div class="bg-white rounded-xl border border-indigo-200 shadow-card p-5 space-y-4">
                        <div class="flex items-center gap-2">
                            <span class="text-indigo-500">{ icons::cloud_arrow_up("w-5 h-5") }</span>
                            <h2 class="text-sm font-semibold text-slate-800">
                                {"Upload New Credential Document"}
                            </h2>
                        </div>
                        <div class="grid grid-cols-2 gap-4">
                            <div>
                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                    {"Contractor ID (UUID)"}
                                </label>
                                <input
                                    type="text"
                                    placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                                    class={input_cls()}
                                    value={(*up_contractor).clone()}
                                    oninput={{
                                        let up_contractor = up_contractor.clone();
                                        Callback::from(move |e: InputEvent| {
                                            if let Some(v) = e.target()
                                                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                            { up_contractor.set(v.value()); }
                                        })
                                    }}
                                />
                            </div>
                            <div>
                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                    {"Document Type"}
                                </label>
                                <input
                                    type="text"
                                    placeholder="e.g. conductor_license"
                                    class={input_cls()}
                                    value={(*up_doc_type).clone()}
                                    oninput={{
                                        let up_doc_type = up_doc_type.clone();
                                        Callback::from(move |e: InputEvent| {
                                            if let Some(v) = e.target()
                                                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                            { up_doc_type.set(v.value()); }
                                        })
                                    }}
                                />
                            </div>
                            <div>
                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                    {"Expires At (optional)"}
                                </label>
                                <input
                                    type="date"
                                    class={input_cls()}
                                    value={(*up_expires).clone()}
                                    onchange={{
                                        let up_expires = up_expires.clone();
                                        Callback::from(move |e: Event| up_expires.set(ev_val(e)))
                                    }}
                                />
                            </div>
                            <div>
                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                    {"File (PDF / JPEG / PNG, max 10 MB)"}
                                </label>
                                <input
                                    type="file"
                                    ref={file_input_ref.clone()}
                                    accept=".pdf,.jpg,.jpeg,.png"
                                    class="block w-full text-sm text-slate-500 file:mr-3 file:py-1.5 \
                                           file:px-3 file:rounded-lg file:border-0 file:text-xs \
                                           file:font-medium file:bg-indigo-50 file:text-indigo-700 \
                                           hover:file:bg-indigo-100 cursor-pointer"
                                />
                            </div>
                        </div>
                        <div class="flex gap-2">
                            <button class={btn_primary()} onclick={on_upload}>
                                { icons::cloud_arrow_up("w-4 h-4") }
                                {"Upload"}
                            </button>
                            <button
                                class={btn_secondary()}
                                onclick={{ let su = show_upload.clone(); Callback::from(move |_: MouseEvent| su.set(false)) }}>
                                {"Cancel"}
                            </button>
                        </div>
                    </div>
                }

                // ── Filter row ─────────────────────────────────────────────
                <div class="flex items-center gap-3">
                    <span class="text-slate-400">{ icons::funnel("w-4 h-4") }</span>
                    <select
                        class={select_cls()}
                        value={(*status_filter).clone()}
                        onchange={{
                            let (status_filter, page, reload) =
                                (status_filter.clone(), page.clone(), reload.clone());
                            Callback::from(move |e: Event| {
                                status_filter.set(ev_sel(e));
                                page.set(1);
                                reload.set(reload.wrapping_add(1));
                            })
                        }}>
                        <option value="">{"All statuses"}</option>
                        <option value="pending">{"Pending"}</option>
                        <option value="approved">{"Approved"}</option>
                        <option value="rejected">{"Rejected"}</option>
                        <option value="expired">{"Expired"}</option>
                    </select>
                </div>

                // ── Split list + detail ────────────────────────────────────
                <div class="flex gap-5 items-start">

                    // ── Credential list ────────────────────────────────────
                    <div class="flex-1 min-w-0">
                        if *loading {
                            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                <table class="min-w-full">
                                    <tbody>{ skeleton_rows(6) }</tbody>
                                </table>
                            </div>
                        } else if creds.is_empty() {
                            { empty_state(icons::document_text("w-10 h-10"),
                                "No credentials found",
                                "Upload a document or adjust the status filter.") }
                        } else {
                            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                <table class="w-full text-sm">
                                    <thead class="bg-slate-50 border-b border-slate-200/60">
                                        <tr>
                                            <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                {"Type"}
                                            </th>
                                            <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                {"File"}
                                            </th>
                                            <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                {"Status"}
                                            </th>
                                            <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                {"Expires"}
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody class="divide-y divide-slate-100">
                                    { for creds.iter().map(|cred| {
                                        let cred_clone = cred.clone();
                                        let on_select  = on_select.clone();
                                        let is_sel     = selected.as_ref().map(|s| s.id) == Some(cred.id);
                                        let row_class  = if is_sel {
                                            "bg-indigo-50 cursor-pointer border-l-2 border-l-indigo-500"
                                        } else {
                                            "hover:bg-slate-50/60 cursor-pointer transition-colors"
                                        };
                                        html! {
                                            <tr class={row_class}
                                                onclick={Callback::from(move |_: MouseEvent| {
                                                    on_select.emit(cred_clone.clone())
                                                })}>
                                                <td class="px-4 py-3.5 font-mono text-xs font-medium text-slate-700">
                                                    { &cred.document_type }
                                                </td>
                                                <td class="px-4 py-3.5 text-sm text-slate-600 truncate max-w-[200px]">
                                                    { &cred.file_name }
                                                </td>
                                                <td class="px-4 py-3.5">
                                                    { status_badge(&cred.status) }
                                                </td>
                                                <td class="px-4 py-3.5 text-xs text-slate-400">
                                                    { cred.expires_at.as_deref().unwrap_or("—") }
                                                </td>
                                            </tr>
                                        }
                                    })}
                                    </tbody>
                                </table>
                                // Pagination
                                { render_pagination(*total, *page, page.clone(), reload.clone()) }
                            </div>
                        }
                    </div>

                    // ── Detail panel ───────────────────────────────────────
                    <div class="w-80 shrink-0">
                    if let Some(cred) = &*selected {
                        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">

                            // Header
                            <div class="px-5 py-4 border-b border-slate-100 flex items-start justify-between gap-3">
                                <div class="min-w-0">
                                    <p class="text-sm font-semibold text-slate-900 font-mono">
                                        { &cred.document_type }
                                    </p>
                                    <p class="text-xs text-slate-400 mt-0.5 truncate">
                                        { &cred.file_name }
                                    </p>
                                </div>
                                { status_badge(&cred.status) }
                            </div>

                            // Info grid
                            <div class="px-5 py-3 border-b border-slate-100 grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
                                <span class="text-slate-400">{"MIME type"}</span>
                                <span class="font-mono text-slate-600 truncate">{ &cred.mime_type }</span>
                                <span class="text-slate-400">{"File size"}</span>
                                <span class="text-slate-700">{ fmt_size(cred.file_size_bytes) }</span>
                                <span class="text-slate-400">{"Fingerprint"}</span>
                                <span class="font-mono text-slate-500 truncate">
                                    { &cred.fingerprint[..16] }{"…"}
                                </span>
                                <span class="text-slate-400">{"Expires"}</span>
                                <span class="text-slate-700">
                                    { cred.expires_at.as_deref().unwrap_or("—") }
                                </span>
                                <span class="text-slate-400">{"Uploaded"}</span>
                                <span class="text-slate-700">{ fmt_dt(&cred.uploaded_at) }</span>
                                if let Some(rat) = &cred.reviewed_at {
                                    <span class="text-slate-400">{"Reviewed"}</span>
                                    <span class="text-slate-700">{ fmt_dt(rat) }</span>
                                }
                                if let Some(notes) = &cred.review_notes {
                                    <span class="text-slate-400">{"Notes"}</span>
                                    <span class="text-slate-700">{ notes }</span>
                                }
                            </div>

                            // Tab bar
                            <div class="flex border-b border-slate-200/60 px-2 pt-1">
                                { render_tab_bar(can_approve, action.clone(), on_load_audit.clone()) }
                            </div>

                            // Tab content
                            <div class="px-5 py-4">

                                // Info tab (0)
                                if *action == 0 {
                                    <p class="text-xs text-slate-400">
                                        {"Select Review, E-sign, or Audit to take action."}
                                    </p>
                                }

                                // Review tab (1)
                                if *action == 1 && can_approve {
                                    <div class="space-y-3">
                                        if cred.status != "pending" {
                                            <p class="text-xs text-slate-500">
                                                {"Only pending credentials can be reviewed."}
                                            </p>
                                        } else {
                                            <div>
                                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                                    {"Decision"}
                                                </label>
                                                <select
                                                    class={select_cls()}
                                                    value={(*rev_status).clone()}
                                                    onchange={{
                                                        let rev_status = rev_status.clone();
                                                        Callback::from(move |e: Event| rev_status.set(ev_sel(e)))
                                                    }}>
                                                    <option value="approved">{"Approve"}</option>
                                                    <option value="rejected">{"Reject"}</option>
                                                </select>
                                            </div>
                                            <div>
                                                <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                                    {"Notes (optional)"}
                                                </label>
                                                <input
                                                    type="text"
                                                    class={input_cls()}
                                                    placeholder="Reason for decision…"
                                                    value={(*rev_notes).clone()}
                                                    oninput={{
                                                        let rev_notes = rev_notes.clone();
                                                        Callback::from(move |e: InputEvent| {
                                                            if let Some(el) = e.target()
                                                                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                                            { rev_notes.set(el.value()); }
                                                        })
                                                    }}
                                                />
                                            </div>
                                            <button
                                                class={btn_primary()}
                                                onclick={on_review.clone()}>
                                                { icons::check_circle("w-4 h-4") }
                                                {"Submit Review"}
                                            </button>
                                        }
                                    </div>
                                }

                                // E-sign tab (2)
                                if *action == 2 {
                                    <div class="space-y-3">
                                        <div>
                                            <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                                {"Signer Name"}
                                            </label>
                                            <input
                                                type="text"
                                                class={input_cls()}
                                                placeholder="Full legal name"
                                                value={(*esign_name).clone()}
                                                oninput={{
                                                    let esign_name = esign_name.clone();
                                                    Callback::from(move |e: InputEvent| {
                                                        if let Some(el) = e.target()
                                                            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                                        { esign_name.set(el.value()); }
                                                    })
                                                }}
                                            />
                                        </div>
                                        <div>
                                            <label class="block text-xs font-medium text-slate-600 mb-1.5">
                                                {"Signed Date"}
                                            </label>
                                            <input
                                                type="date"
                                                class={input_cls()}
                                                value={(*esign_date).clone()}
                                                onchange={{
                                                    let esign_date = esign_date.clone();
                                                    Callback::from(move |e: Event| esign_date.set(ev_val(e)))
                                                }}
                                            />
                                        </div>
                                        <button
                                            class={btn_primary()}
                                            onclick={on_esign.clone()}>
                                            { icons::pencil("w-4 h-4") }
                                            {"Record Signature"}
                                        </button>
                                    </div>
                                }

                                // Audit tab (3)
                                if *action == 3 {
                                    <div class="space-y-2 max-h-64 overflow-y-auto scrollbar-thin">
                                    if audit.is_empty() {
                                        <p class="text-xs text-slate-400">{"No audit entries."}</p>
                                    } else {
                                        { for audit.iter().map(|e| {
                                            let action_color = match e.action.as_str() {
                                                "approved" => "text-emerald-700 bg-emerald-50",
                                                "rejected" => "text-red-700 bg-red-50",
                                                "esigned"  => "text-indigo-700 bg-indigo-50",
                                                "uploaded" => "text-sky-700 bg-sky-50",
                                                _          => "text-slate-600 bg-slate-50",
                                            };
                                            html! {
                                                <div class="flex items-start gap-2.5 py-1.5 border-b border-slate-100 last:border-0">
                                                    <span class={format!("shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold {}", action_color)}>
                                                        { &e.action }
                                                    </span>
                                                    <div class="min-w-0 flex-1">
                                                        if let Some(name) = &e.viewer_name {
                                                            <p class="text-xs text-slate-700">{ name }</p>
                                                        }
                                                        <p class="text-[10px] text-slate-400 tabular-nums">
                                                            { fmt_dt(&e.created_at) }
                                                        </p>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                    }
                                    </div>
                                }

                            </div>
                        </div>
                    } else {
                        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card p-8 text-center">
                            <span class="text-slate-200 mb-3 block">
                                { icons::document_text("w-10 h-10 mx-auto") }
                            </span>
                            <p class="text-sm text-slate-400">
                                {"Select a credential to view details."}
                            </p>
                        </div>
                    }
                    </div>

                </div>
            </div>
        </OpsLayout>
        { toasts.view }
        </>
    }
}
