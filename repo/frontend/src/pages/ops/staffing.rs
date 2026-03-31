//! Operations console — staffing view (Dispatcher / Admin / OpsAgent).
//!
//! Two tabs:
//!   • Shifts      — filter list; click row for detail panel showing assignments
//!                   and (for dispatchers/admins) ranked match candidates with
//!                   one-click propose.
//!   • Contractors — filter list; click row to see tags + active toggle.
//!
//! Subscription panel: visible in both tabs; shows the current user's shift/route
//! subscriptions and lets them subscribe / unsubscribe.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use shared::UserRole;

use crate::api::staffing::{
    self as staffing_api,
    AssignmentRow, CandidatesResponse, ContractorDetail, ContractorRow,
    MatchCandidateItem, Page, ShiftDetail, ShiftRow, SubscriptionItem,
};
use crate::app::Route;
use crate::auth::AuthContext;
use crate::components::{
    icons, status_badge, empty_state, btn_primary, btn_secondary, input_cls, select_cls,
    skeleton_rows,
};

use super::OpsLayout;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract value from an <input> change event.
fn ev_val(e: Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

/// Extract value from a <select> change event.
fn ev_sel(e: Event) -> String {
    e.target()
        .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

fn fmt_dt(s: &str) -> String {
    // "2025-06-01T08:00:00Z" → "2025-06-01 08:00"
    if s.len() >= 16 { s[..16].replace('T', " ") } else { s.to_owned() }
}

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsStaffingPage)]
pub fn ops_staffing_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();
    let user  = auth.current_user().unwrap();

    let can_manage = matches!(user.role, UserRole::Admin | UserRole::Dispatcher);

    // ── Tab: 0=Shifts  1=Contractors ──────────────────────────────────────
    let active_tab = use_state(|| 0u8);

    // ── Shifts tab ────────────────────────────────────────────────────────
    let shift_status = use_state(String::new);
    let shift_region = use_state(String::new);
    let shift_role   = use_state(String::new);
    let shift_page   = use_state(|| 1i64);
    let shifts       = use_state(|| None::<Page<ShiftRow>>);
    let shifts_loading = use_state(|| false);

    // ── Shift detail ──────────────────────────────────────────────────────
    let selected_shift   = use_state(|| None::<String>);
    let shift_detail     = use_state(|| None::<ShiftDetail>);
    let detail_loading   = use_state(|| false);
    let candidates       = use_state(|| None::<CandidatesResponse>);
    let cands_loading    = use_state(|| false);
    let cands_err        = use_state(|| None::<String>);

    // ── Assignment actions ────────────────────────────────────────────────
    let action_err  = use_state(|| None::<String>);
    let action_ok   = use_state(|| None::<String>);

    // ── Contractors tab ───────────────────────────────────────────────────
    let ctr_region   = use_state(String::new);
    let ctr_active   = use_state(|| None::<bool>);
    let ctr_page     = use_state(|| 1i64);
    let contractors  = use_state(|| None::<Page<ContractorRow>>);
    let ctrs_loading = use_state(|| false);
    let selected_ctr = use_state(|| None::<String>);
    let ctr_detail   = use_state(|| None::<ContractorDetail>);

    // ── Subscriptions ─────────────────────────────────────────────────────
    let subs         = use_state(|| None::<Vec<SubscriptionItem>>);
    let subs_loading = use_state(|| false);
    let sub_err      = use_state(|| None::<String>);

    // ── Load shifts ───────────────────────────────────────────────────────
    {
        let shifts   = shifts.clone();
        let loading  = shifts_loading.clone();
        let token    = token.clone();
        let status   = (*shift_status).clone();
        let region   = (*shift_region).clone();
        let role     = (*shift_role).clone();
        let pg       = *shift_page;
        let tab      = *active_tab;
        use_effect_with(
            (status.clone(), region.clone(), role.clone(), pg, tab),
            move |_| {
                if tab != 0 { return || (); }
                loading.set(true);
                spawn_local(async move {
                    match staffing_api::list_shifts(&token, &status, &region, &role, pg).await {
                        Ok(p)  => { shifts.set(Some(p));  loading.set(false); }
                        Err(_) => { loading.set(false); }
                    }
                });
                || ()
            },
        );
    }

    // ── Load contractors ──────────────────────────────────────────────────
    {
        let contractors = contractors.clone();
        let loading     = ctrs_loading.clone();
        let token       = token.clone();
        let region      = (*ctr_region).clone();
        let active      = *ctr_active;
        let pg          = *ctr_page;
        let tab         = *active_tab;
        use_effect_with((region.clone(), active, pg, tab), move |_| {
            if tab != 1 { return || (); }
            loading.set(true);
            spawn_local(async move {
                match staffing_api::list_contractors(&token, &region, active, pg).await {
                    Ok(p)  => { contractors.set(Some(p)); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Load subscriptions on mount ───────────────────────────────────────
    {
        let subs    = subs.clone();
        let loading = subs_loading.clone();
        let token   = token.clone();
        use_effect_with((), move |_| {
            loading.set(true);
            spawn_local(async move {
                match staffing_api::list_subscriptions(&token).await {
                    Ok(v)  => { subs.set(Some(v)); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Select shift → load detail ────────────────────────────────────────
    let on_select_shift = {
        let selected_shift = selected_shift.clone();
        let shift_detail   = shift_detail.clone();
        let detail_loading = detail_loading.clone();
        let candidates     = candidates.clone();
        let action_err     = action_err.clone();
        let action_ok      = action_ok.clone();
        let token          = token.clone();
        Callback::from(move |id: String| {
            selected_shift.set(Some(id.clone()));
            shift_detail.set(None);
            candidates.set(None);
            action_err.set(None);
            action_ok.set(None);
            let (detail_loading, shift_detail, token) =
                (detail_loading.clone(), shift_detail.clone(), token.clone());
            spawn_local(async move {
                detail_loading.set(true);
                match staffing_api::get_shift(&token, &id).await {
                    Ok(d)  => { shift_detail.set(Some(d)); detail_loading.set(false); }
                    Err(_) => { detail_loading.set(false); }
                }
            });
        })
    };

    // ── Load candidates ───────────────────────────────────────────────────
    let on_load_candidates = {
        let candidates  = candidates.clone();
        let cands_loading = cands_loading.clone();
        let cands_err   = cands_err.clone();
        let token       = token.clone();
        let shift_id    = (*selected_shift).clone();
        Callback::from(move |_: MouseEvent| {
            let id = match &shift_id {
                Some(id) => id.clone(),
                None     => return,
            };
            let (candidates, loading, err, token) =
                (candidates.clone(), cands_loading.clone(), cands_err.clone(), token.clone());
            spawn_local(async move {
                loading.set(true);
                err.set(None);
                match staffing_api::get_candidates(&token, &id).await {
                    Ok(r)  => { candidates.set(Some(r)); loading.set(false); }
                    Err(e) => { err.set(Some(e.message)); loading.set(false); }
                }
            });
        })
    };

    // ── Propose assignment ────────────────────────────────────────────────
    let on_propose = {
        let action_ok      = action_ok.clone();
        let action_err     = action_err.clone();
        let shift_detail   = shift_detail.clone();
        let selected_shift = selected_shift.clone();
        let token          = token.clone();
        Callback::from(move |(shift_id, contractor_id): (String, String)| {
            let (ok, err, detail, sel, token) = (
                action_ok.clone(),
                action_err.clone(),
                shift_detail.clone(),
                selected_shift.clone(),
                token.clone(),
            );
            spawn_local(async move {
                err.set(None);
                match staffing_api::propose_assignment(&token, &shift_id, &contractor_id).await {
                    Ok(_)  => {
                        ok.set(Some(format!("Proposed contractor {contractor_id}")));
                        // Refresh shift detail.
                        if let Ok(d) = staffing_api::get_shift(&token, &shift_id).await {
                            detail.set(Some(d));
                        }
                    }
                    Err(e) => { err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Respond to assignment ─────────────────────────────────────────────
    let on_respond = {
        let action_ok    = action_ok.clone();
        let action_err   = action_err.clone();
        let shift_detail = shift_detail.clone();
        let token        = token.clone();
        Callback::from(move |(assignment_id, status, shift_id): (String, String, String)| {
            let (ok, err, detail, token) =
                (action_ok.clone(), action_err.clone(), shift_detail.clone(), token.clone());
            spawn_local(async move {
                err.set(None);
                match staffing_api::respond_assignment(&token, &assignment_id, &status).await {
                    Ok(_)  => {
                        ok.set(Some(format!("Assignment {status}")));
                        if let Ok(d) = staffing_api::get_shift(&token, &shift_id).await {
                            detail.set(Some(d));
                        }
                    }
                    Err(e) => { err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Select contractor → load detail ───────────────────────────────────
    let on_select_ctr = {
        let selected_ctr = selected_ctr.clone();
        let ctr_detail   = ctr_detail.clone();
        let token        = token.clone();
        Callback::from(move |id: String| {
            selected_ctr.set(Some(id.clone()));
            ctr_detail.set(None);
            let (detail, token) = (ctr_detail.clone(), token.clone());
            spawn_local(async move {
                match staffing_api::get_contractor(&token, &id).await {
                    Ok(d)  => { detail.set(Some(d)); }
                    Err(_) => {}
                }
            });
        })
    };

    // ── Toggle contractor active ──────────────────────────────────────────
    let on_toggle_active = {
        let contractors  = contractors.clone();
        let selected_ctr = selected_ctr.clone();
        let ctr_detail   = ctr_detail.clone();
        let token        = token.clone();
        Callback::from(move |(id, active): (String, bool)| {
            let (sel, detail, token) =
                (selected_ctr.clone(), ctr_detail.clone(), token.clone());
            spawn_local(async move {
                let _ = staffing_api::set_contractor_active(&token, &id, active).await;
                // Refresh detail.
                if sel.as_deref() == Some(&id) {
                    if let Ok(d) = staffing_api::get_contractor(&token, &id).await {
                        detail.set(Some(d));
                    }
                }
            });
        })
    };

    // ── Subscribe / unsubscribe ───────────────────────────────────────────
    let on_unsub = {
        let subs    = subs.clone();
        let sub_err = sub_err.clone();
        let token   = token.clone();
        Callback::from(move |(target_type, target_id): (String, String)| {
            let (subs, err, token) = (subs.clone(), sub_err.clone(), token.clone());
            spawn_local(async move {
                err.set(None);
                match staffing_api::unsubscribe(&token, "dispatcher", &target_type, &target_id)
                    .await
                {
                    Ok(_) => {
                        if let Ok(v) = staffing_api::list_subscriptions(&token).await {
                            subs.set(Some(v));
                        }
                    }
                    Err(e) => { err.set(Some(e.message)); }
                }
            });
        })
    };

    let on_sub_shift = {
        let subs    = subs.clone();
        let sub_err = sub_err.clone();
        let token   = token.clone();
        let shift_id = (*selected_shift).clone();
        Callback::from(move |_: MouseEvent| {
            let id = match &shift_id {
                Some(id) => id.clone(),
                None     => return,
            };
            let (subs, err, token) = (subs.clone(), sub_err.clone(), token.clone());
            spawn_local(async move {
                err.set(None);
                match staffing_api::subscribe(&token, "dispatcher", "shift", &id).await {
                    Ok(_) => {
                        if let Ok(v) = staffing_api::list_subscriptions(&token).await {
                            subs.set(Some(v));
                        }
                    }
                    Err(e) => { err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Render ────────────────────────────────────────────────────────────
    html! {
        <OpsLayout active={Route::OpsStaffing}>
            <div class="space-y-6 max-w-7xl">
                // ── Page header ───────────────────────────────────────────
                <div class="flex items-center justify-between">
                    <div>
                        <h1 class="text-lg font-semibold text-slate-900">{"Staffing"}</h1>
                        <p class="text-sm text-slate-500 mt-0.5">
                            {"Manage shifts, contractors, and assignments."}
                        </p>
                    </div>
                </div>

                // ── Tab bar ───────────────────────────────────────────────
                <div class="flex border-b border-slate-200">
                    { tab_btn("Shifts",      0, *active_tab, {
                        let t = active_tab.clone();
                        Callback::from(move |_| t.set(0))
                    }) }
                    { tab_btn("Contractors", 1, *active_tab, {
                        let t = active_tab.clone();
                        Callback::from(move |_| t.set(1))
                    }) }
                </div>

                // ── Shifts tab ────────────────────────────────────────────
                if *active_tab == 0 {
                    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
                        // ── Left: shift list ──────────────────────────────
                        <div class="lg:col-span-1 space-y-3">
                            // Filter row
                            <div class="flex flex-wrap gap-2 p-3 bg-white rounded-xl
                                        border border-slate-200/80 shadow-card">
                                { icons::funnel("w-4 h-4 text-slate-400 shrink-0 self-center") }
                                <select
                                    class={select_cls()}
                                    onchange={{
                                        let shift_status = shift_status.clone();
                                        let shift_page   = shift_page.clone();
                                        Callback::from(move |e: Event| {
                                            shift_status.set(ev_sel(e));
                                            shift_page.set(1);
                                        })
                                    }}>
                                    <option value="">{"All statuses"}</option>
                                    <option value="open">{"Open"}</option>
                                    <option value="assigned">{"Assigned"}</option>
                                    <option value="completed">{"Completed"}</option>
                                    <option value="cancelled">{"Cancelled"}</option>
                                </select>
                                <select
                                    class={select_cls()}
                                    onchange={{
                                        let shift_role = shift_role.clone();
                                        let shift_page = shift_page.clone();
                                        Callback::from(move |e: Event| {
                                            shift_role.set(ev_sel(e));
                                            shift_page.set(1);
                                        })
                                    }}>
                                    <option value="">{"All roles"}</option>
                                    <option value="conductor">{"Conductor"}</option>
                                    <option value="driver">{"Driver"}</option>
                                </select>
                                <input
                                    type="text"
                                    placeholder="Region…"
                                    class={format!("{} w-24", input_cls())}
                                    value={(*shift_region).clone()}
                                    onchange={{
                                        let shift_region = shift_region.clone();
                                        let shift_page   = shift_page.clone();
                                        Callback::from(move |e: Event| {
                                            shift_region.set(ev_val(e));
                                            shift_page.set(1);
                                        })
                                    }}
                                />
                            </div>

                            // Shift list
                            <div class="bg-white rounded-xl border border-slate-200/80
                                        shadow-card overflow-hidden">
                                if *shifts_loading {
                                    <table class="min-w-full">
                                        <tbody>{ skeleton_rows(6) }</tbody>
                                    </table>
                                } else if let Some(page) = &*shifts {
                                    if page.items.is_empty() {
                                        { empty_state(
                                            icons::clock("w-8 h-8"),
                                            "No shifts found",
                                            "Try adjusting filters or check back later.",
                                        ) }
                                    } else {
                                        <div class="divide-y divide-slate-100">
                                        { for page.items.iter().map(|s| {
                                            let sid       = s.id.clone();
                                            let is_sel    = selected_shift.as_deref() == Some(&s.id);
                                            let on_click  = {
                                                let cb = on_select_shift.clone();
                                                let id = sid.clone();
                                                Callback::from(move |_: MouseEvent| cb.emit(id.clone()))
                                            };
                                            let row_cls = if is_sel {
                                                "px-4 py-3 cursor-pointer bg-indigo-50 \
                                                 border-l-2 border-l-indigo-500 transition-colors"
                                            } else {
                                                "px-4 py-3 cursor-pointer \
                                                 hover:bg-slate-50/60 transition-colors"
                                            };
                                            html! {
                                                <div class={row_cls} onclick={on_click}>
                                                    <div class="flex items-center justify-between gap-2">
                                                        <div class="flex items-center gap-2 min-w-0">
                                                            if s.is_critical {
                                                                { icons::exclamation_triangle("w-3.5 h-3.5 text-amber-500 shrink-0") }
                                                            }
                                                            <span class="text-sm font-medium text-slate-800 capitalize truncate">
                                                                { &s.role }
                                                            </span>
                                                        </div>
                                                        { status_badge(&s.status) }
                                                    </div>
                                                    <div class="flex items-center gap-1.5 text-xs text-slate-500 mt-1">
                                                        <span>{ s.region.as_deref().unwrap_or("—") }</span>
                                                        <span class="text-slate-300">{"·"}</span>
                                                        { icons::clock("w-3 h-3 shrink-0") }
                                                        <span>{ fmt_dt(&s.shift_start) }</span>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        </div>
                                        // Pagination
                                        <div class="px-4 py-2.5 border-t border-slate-100
                                                    flex items-center justify-between">
                                            <span class="text-xs text-slate-500">
                                                {format!("{} total", page.total)}
                                            </span>
                                            <div class="flex items-center gap-1">
                                                if page.page > 1 {
                                                    <button
                                                        class={btn_secondary()}
                                                        onclick={{
                                                            let p = shift_page.clone();
                                                            let cur = page.page;
                                                            Callback::from(move |_: MouseEvent| p.set(cur - 1))
                                                        }}>
                                                        { icons::chevron_left("w-3.5 h-3.5") }
                                                    </button>
                                                }
                                                if page.page < page.total_pages {
                                                    <button
                                                        class={btn_secondary()}
                                                        onclick={{
                                                            let p = shift_page.clone();
                                                            let cur = page.page;
                                                            Callback::from(move |_: MouseEvent| p.set(cur + 1))
                                                        }}>
                                                        { icons::chevron_right("w-3.5 h-3.5") }
                                                    </button>
                                                }
                                            </div>
                                        </div>
                                    }
                                }
                            </div>
                        </div>

                        // ── Right: shift detail ───────────────────────────
                        <div class="lg:col-span-2 space-y-4">
                            if let Some(detail) = &*shift_detail {
                                { render_shift_detail(
                                    detail,
                                    &candidates,
                                    *cands_loading,
                                    cands_err.as_ref().as_ref(),
                                    can_manage,
                                    action_err.as_ref().as_ref(),
                                    action_ok.as_ref().as_ref(),
                                    on_load_candidates.clone(),
                                    on_propose.clone(),
                                    on_respond.clone(),
                                    on_sub_shift.clone(),
                                ) }
                            } else if selected_shift.is_some() && *detail_loading {
                                <div class="bg-white rounded-xl border border-slate-200/80
                                            shadow-card p-8 text-center">
                                    <p class="text-sm text-slate-400 animate-pulse">
                                        {"Loading shift details…"}
                                    </p>
                                </div>
                            } else if selected_shift.is_none() {
                                <div class="bg-white rounded-xl border border-slate-200/80
                                            shadow-card p-10 text-center">
                                    { icons::user_group("w-10 h-10 mx-auto text-slate-300 mb-3") }
                                    <p class="text-sm font-medium text-slate-600">
                                        {"Select a shift"}
                                    </p>
                                    <p class="text-xs text-slate-400 mt-1">
                                        {"Click a shift to view details and manage assignments."}
                                    </p>
                                </div>
                            }

                            // ── Subscriptions ─────────────────────────────
                            { render_subscriptions(&subs, *subs_loading, sub_err.as_ref().as_ref(), on_unsub.clone()) }
                        </div>
                    </div>
                }

                // ── Contractors tab ───────────────────────────────────────
                if *active_tab == 1 {
                    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
                        // ── Left: contractor list ─────────────────────────
                        <div class="lg:col-span-1 space-y-3">
                            // Filter row
                            <div class="flex flex-wrap gap-2 p-3 bg-white rounded-xl
                                        border border-slate-200/80 shadow-card">
                                { icons::funnel("w-4 h-4 text-slate-400 shrink-0 self-center") }
                                <input
                                    type="text"
                                    placeholder="Region…"
                                    class={format!("{} w-24", input_cls())}
                                    value={(*ctr_region).clone()}
                                    onchange={{
                                        let ctr_region = ctr_region.clone();
                                        let ctr_page   = ctr_page.clone();
                                        Callback::from(move |e: Event| {
                                            ctr_region.set(ev_val(e));
                                            ctr_page.set(1);
                                        })
                                    }}
                                />
                                <select
                                    class={select_cls()}
                                    onchange={{
                                        let ctr_active = ctr_active.clone();
                                        let ctr_page   = ctr_page.clone();
                                        Callback::from(move |e: Event| {
                                            let v = ev_sel(e);
                                            ctr_active.set(match v.as_str() {
                                                "true"  => Some(true),
                                                "false" => Some(false),
                                                _       => None,
                                            });
                                            ctr_page.set(1);
                                        })
                                    }}>
                                    <option value="">{"All"}</option>
                                    <option value="true">{"Active only"}</option>
                                    <option value="false">{"Inactive only"}</option>
                                </select>
                            </div>

                            <div class="bg-white rounded-xl border border-slate-200/80
                                        shadow-card overflow-hidden">
                                if *ctrs_loading {
                                    <table class="min-w-full">
                                        <tbody>{ skeleton_rows(6) }</tbody>
                                    </table>
                                } else if let Some(page) = &*contractors {
                                    if page.items.is_empty() {
                                        { empty_state(
                                            icons::users("w-8 h-8"),
                                            "No contractors found",
                                            "Try adjusting your filters.",
                                        ) }
                                    } else {
                                        <div class="divide-y divide-slate-100">
                                        { for page.items.iter().map(|c| {
                                            let cid      = c.id.clone();
                                            let is_sel   = selected_ctr.as_deref() == Some(&c.id);
                                            let on_click = {
                                                let cb = on_select_ctr.clone();
                                                let id = cid.clone();
                                                Callback::from(move |_: MouseEvent| cb.emit(id.clone()))
                                            };
                                            let row_cls = if is_sel {
                                                "px-4 py-3 cursor-pointer bg-indigo-50 \
                                                 border-l-2 border-l-indigo-500 transition-colors"
                                            } else {
                                                "px-4 py-3 cursor-pointer \
                                                 hover:bg-slate-50/60 transition-colors"
                                            };
                                            let dot_cls = if c.is_active {
                                                "inline-block w-2 h-2 rounded-full bg-emerald-500 shrink-0"
                                            } else {
                                                "inline-block w-2 h-2 rounded-full bg-slate-300 shrink-0"
                                            };
                                            html! {
                                                <div class={row_cls} onclick={on_click}>
                                                    <div class="flex items-center gap-2">
                                                        <span class={dot_cls}></span>
                                                        <span class="text-sm font-medium text-slate-800 truncate">
                                                            { &c.full_name }
                                                        </span>
                                                    </div>
                                                    <div class="flex items-center gap-1.5 text-xs text-slate-500 mt-1 ml-4">
                                                        <span>{ c.region.as_deref().unwrap_or("—") }</span>
                                                        <span class="text-slate-300">{"·"}</span>
                                                        { icons::star("w-3 h-3 text-amber-400 shrink-0") }
                                                        <span>{ &c.quality_rating }</span>
                                                    </div>
                                                </div>
                                            }
                                        })}
                                        </div>
                                        <div class="px-4 py-2.5 border-t border-slate-100
                                                    flex items-center justify-between">
                                            <span class="text-xs text-slate-500">
                                                {format!("{} total", page.total)}
                                            </span>
                                            <div class="flex items-center gap-1">
                                                if page.page > 1 {
                                                    <button
                                                        class={btn_secondary()}
                                                        onclick={{
                                                            let p = ctr_page.clone();
                                                            let cur = page.page;
                                                            Callback::from(move |_: MouseEvent| p.set(cur - 1))
                                                        }}>
                                                        { icons::chevron_left("w-3.5 h-3.5") }
                                                    </button>
                                                }
                                                if page.page < page.total_pages {
                                                    <button
                                                        class={btn_secondary()}
                                                        onclick={{
                                                            let p = ctr_page.clone();
                                                            let cur = page.page;
                                                            Callback::from(move |_: MouseEvent| p.set(cur + 1))
                                                        }}>
                                                        { icons::chevron_right("w-3.5 h-3.5") }
                                                    </button>
                                                }
                                            </div>
                                        </div>
                                    }
                                }
                            </div>
                        </div>

                        // ── Right: contractor detail ───────────────────────
                        <div class="lg:col-span-2">
                            if let Some(detail) = &*ctr_detail {
                                { render_contractor_detail(detail, can_manage, on_toggle_active.clone()) }
                            } else if selected_ctr.is_none() {
                                <div class="bg-white rounded-xl border border-slate-200/80
                                            shadow-card p-10 text-center">
                                    { icons::user_circle("w-10 h-10 mx-auto text-slate-300 mb-3") }
                                    <p class="text-sm font-medium text-slate-600">
                                        {"Select a contractor"}
                                    </p>
                                    <p class="text-xs text-slate-400 mt-1">
                                        {"Click a contractor to view profile and tags."}
                                    </p>
                                </div>
                            }
                        </div>
                    </div>
                }
            </div>
        </OpsLayout>
    }
}

// ── Sub-render: shift detail ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_shift_detail(
    detail:          &ShiftDetail,
    candidates:      &UseStateHandle<Option<CandidatesResponse>>,
    cands_loading:   bool,
    cands_err:       Option<&String>,
    can_manage:      bool,
    action_err:      Option<&String>,
    action_ok:       Option<&String>,
    on_load_cands:   Callback<MouseEvent>,
    on_propose:      Callback<(String, String)>,
    on_respond:      Callback<(String, String, String)>,
    on_sub:          Callback<MouseEvent>,
) -> Html {
    let s = &detail.shift;
    html! {
        <div class="space-y-4">
            // ── Shift info card ────────────────────────────────────────────
            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                // Header
                <div class="px-5 py-4 border-b border-slate-100 flex items-center justify-between gap-4">
                    <div class="flex items-center gap-3">
                        { icons::clock("w-5 h-5 text-indigo-500 shrink-0") }
                        <div>
                            <h2 class="text-sm font-semibold text-slate-900 capitalize">
                                { &s.role }
                                if s.is_critical {
                                    <span class="ml-2 inline-flex items-center gap-1 text-xs
                                                 font-medium text-amber-700">
                                        { icons::exclamation_triangle("w-3 h-3") }
                                        {"Critical"}
                                    </span>
                                }
                            </h2>
                            <p class="text-xs text-slate-500">
                                { s.region.as_deref().unwrap_or("No region") }
                            </p>
                        </div>
                    </div>
                    <div class="flex items-center gap-2">
                        { status_badge(&s.status) }
                        if can_manage {
                            <button
                                class="inline-flex items-center gap-1 text-xs font-medium
                                       text-indigo-600 hover:text-indigo-800 px-2 py-1
                                       rounded hover:bg-indigo-50 transition-colors"
                                onclick={on_sub}>
                                { icons::plus("w-3 h-3") }
                                {"Subscribe"}
                            </button>
                        }
                    </div>
                </div>
                // Info grid
                <div class="px-5 py-4 grid grid-cols-2 gap-4">
                    <div>
                        <p class="text-xs text-slate-400 mb-0.5">{"Start"}</p>
                        <p class="text-sm font-medium text-slate-700">
                            { fmt_dt(&s.shift_start) }
                        </p>
                    </div>
                    <div>
                        <p class="text-xs text-slate-400 mb-0.5">{"End"}</p>
                        <p class="text-sm font-medium text-slate-700">
                            { fmt_dt(&s.shift_end) }
                        </p>
                    </div>
                    <div class="col-span-2">
                        <p class="text-xs text-slate-400 mb-1">{"Required tags"}</p>
                        if s.required_tags.is_empty() {
                            <p class="text-sm text-slate-400">{"None"}</p>
                        } else {
                            <div class="flex flex-wrap gap-1">
                                { for s.required_tags.iter().map(|t| html! {
                                    <span class="inline-flex items-center gap-1 bg-slate-100
                                                 text-slate-700 text-xs rounded-full px-2 py-0.5">
                                        { icons::tag("w-2.5 h-2.5") }
                                        { t }
                                    </span>
                                })}
                            </div>
                        }
                    </div>
                </div>
            </div>

            // ── Assignments ────────────────────────────────────────────────
            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                <div class="px-5 py-3 border-b border-slate-100 bg-slate-50">
                    <h3 class="text-xs font-semibold text-slate-500 uppercase tracking-wide">
                        {"Assignments"}
                    </h3>
                </div>
                if detail.assignments.is_empty() {
                    <p class="px-5 py-4 text-sm text-slate-400">{"No assignments yet."}</p>
                } else {
                    <div class="divide-y divide-slate-100">
                    { for detail.assignments.iter().map(|a| {
                        let aid     = a.id.clone();
                        let sid     = a.shift_id.clone();
                        let status  = a.status.clone();
                        html! {
                            <div class="px-5 py-3.5 flex items-start justify-between gap-4">
                                <div class="min-w-0">
                                    <p class="text-sm font-medium text-slate-800">
                                        { &a.contractor_name }
                                    </p>
                                    <div class="flex items-center gap-2 mt-1">
                                        { status_badge(&a.status) }
                                        if let Some(score) = &a.match_score {
                                            <span class="text-xs text-slate-500">
                                                {"Score: "}{ score }
                                            </span>
                                        }
                                    </div>
                                    if let Some(reasons) = &a.match_reasons {
                                        <ul class="mt-1.5 text-xs text-slate-500 space-y-0.5">
                                            { for reasons.iter().map(|r| html! {
                                                <li class="flex items-start gap-1">
                                                    <span class="text-indigo-400 mt-0.5">{"•"}</span>
                                                    { r }
                                                </li>
                                            })}
                                        </ul>
                                    }
                                </div>
                                if can_manage && status == "proposed" {
                                    <div class="flex gap-1 shrink-0">
                                        <button
                                            class="inline-flex items-center gap-1 text-xs font-medium
                                                   bg-emerald-600 text-white px-2.5 py-1 rounded-lg
                                                   hover:bg-emerald-700 transition-colors"
                                            onclick={{
                                                let cb  = on_respond.clone();
                                                let aid = aid.clone();
                                                let sid = sid.clone();
                                                Callback::from(move |_: MouseEvent|
                                                    cb.emit((aid.clone(), "accepted".into(), sid.clone()))
                                                )
                                            }}>
                                            { icons::check_circle("w-3.5 h-3.5") }
                                            {"Accept"}
                                        </button>
                                        <button
                                            class="inline-flex items-center gap-1 text-xs font-medium
                                                   border border-slate-200 text-slate-600 px-2.5 py-1
                                                   rounded-lg hover:bg-slate-50 transition-colors"
                                            onclick={{
                                                let cb  = on_respond.clone();
                                                let aid = aid.clone();
                                                let sid = sid.clone();
                                                Callback::from(move |_: MouseEvent|
                                                    cb.emit((aid.clone(), "rejected".into(), sid.clone()))
                                                )
                                            }}>
                                            { icons::x_circle("w-3.5 h-3.5") }
                                            {"Reject"}
                                        </button>
                                    </div>
                                }
                            </div>
                        }
                    })}
                    </div>
                }
            </div>

            // ── Action feedback ────────────────────────────────────────────
            if let Some(msg) = action_err {
                <div class="flex items-center gap-1.5 text-xs text-red-600 px-1">
                    { icons::exclamation_triangle("w-3.5 h-3.5 shrink-0") }
                    { msg }
                </div>
            }
            if let Some(msg) = action_ok {
                <div class="flex items-center gap-1.5 text-xs text-emerald-600 px-1">
                    { icons::check_circle("w-3.5 h-3.5 shrink-0") }
                    { msg }
                </div>
            }

            // ── Match candidates (dispatchers / admins only) ────────────────
            if can_manage {
                <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                    <div class="px-5 py-3 border-b border-slate-100 bg-slate-50
                                flex items-center justify-between">
                        <h3 class="text-xs font-semibold text-slate-500 uppercase tracking-wide">
                            {"Match Candidates"}
                        </h3>
                        <button class={btn_primary()} onclick={on_load_cands}>
                            { icons::users("w-3.5 h-3.5") }
                            if cands_loading { {"Running…"} } else { {"Run Matching"} }
                        </button>
                    </div>
                    if let Some(msg) = cands_err {
                        <div class="flex items-center gap-1.5 px-5 py-3 text-xs text-red-600">
                            { icons::exclamation_triangle("w-3.5 h-3.5 shrink-0") }
                            { msg }
                        </div>
                    } else if let Some(cr) = candidates.as_ref() {
                        if cr.candidates.is_empty() {
                            <p class="px-5 py-4 text-sm text-slate-400">
                                {"No available contractors match this shift."}
                            </p>
                        } else {
                            <div class="divide-y divide-slate-100">
                            { for cr.candidates.iter().enumerate().map(|(i, c)| {
                                let shift_id = s.id.clone();
                                let cid      = c.contractor_id.clone();
                                let propose_cb = {
                                    let cb    = on_propose.clone();
                                    let sid   = shift_id.clone();
                                    let cid2  = cid.clone();
                                    Callback::from(move |_: MouseEvent|
                                        cb.emit((sid.clone(), cid2.clone()))
                                    )
                                };
                                html! {
                                    <div class="px-5 py-3.5 flex items-start justify-between gap-4">
                                        <div class="min-w-0">
                                            <p class="text-sm font-medium text-slate-800">
                                                <span class="text-slate-400 font-normal mr-1.5">
                                                    {format!("{}.", i + 1)}
                                                </span>
                                                { &c.full_name }
                                                <span class="ml-2 text-xs font-semibold text-indigo-600">
                                                    { &c.score }{ " pts" }
                                                </span>
                                            </p>
                                            <ul class="mt-1.5 text-xs text-slate-500 space-y-0.5">
                                                { for c.reasons.iter().map(|r| html! {
                                                    <li class="flex items-start gap-1">
                                                        <span class="text-indigo-400 mt-0.5">{"•"}</span>
                                                        { r }
                                                    </li>
                                                })}
                                            </ul>
                                        </div>
                                        <button
                                            class={btn_primary()}
                                            onclick={propose_cb}>
                                            {"Propose"}
                                        </button>
                                    </div>
                                }
                            })}
                            </div>
                        }
                    } else {
                        <p class="px-5 py-4 text-sm text-slate-400">
                            {"Click \"Run Matching\" to find available contractors."}
                        </p>
                    }
                </div>
            }
        </div>
    }
}

// ── Sub-render: contractor detail ─────────────────────────────────────────────

fn render_contractor_detail(
    detail:          &ContractorDetail,
    can_manage:      bool,
    on_toggle_active: Callback<(String, bool)>,
) -> Html {
    let c = &detail.contractor;
    html! {
        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
            // Header
            <div class="px-5 py-4 border-b border-slate-100 flex items-start justify-between gap-4">
                <div class="flex items-center gap-3">
                    { icons::user_circle("w-9 h-9 text-slate-300 shrink-0") }
                    <div>
                        <h2 class="text-sm font-semibold text-slate-900">{ &c.full_name }</h2>
                        <div class="flex items-center gap-1.5 text-xs text-slate-500 mt-0.5">
                            <span>{ c.region.as_deref().unwrap_or("No region") }</span>
                            <span class="text-slate-300">{"·"}</span>
                            { icons::star("w-3 h-3 text-amber-400 shrink-0") }
                            <span>{ &c.quality_rating }</span>
                            <span class="text-slate-300">{"·"}</span>
                            if c.is_active {
                                <span class="text-emerald-600 font-medium">{"Active"}</span>
                            } else {
                                <span class="text-slate-400">{"Inactive"}</span>
                            }
                        </div>
                    </div>
                </div>
                if can_manage {
                    <button
                        class={if c.is_active {
                            "inline-flex items-center gap-1 text-xs font-medium border \
                             border-slate-200 text-slate-600 px-2.5 py-1 rounded-lg \
                             hover:border-red-300 hover:text-red-600 hover:bg-red-50 \
                             transition-colors"
                        } else {
                            "inline-flex items-center gap-1 text-xs font-medium \
                             bg-emerald-600 text-white px-2.5 py-1 rounded-lg \
                             hover:bg-emerald-700 transition-colors"
                        }}
                        onclick={{
                            let cb     = on_toggle_active.clone();
                            let id     = c.id.clone();
                            let active = c.is_active;
                            Callback::from(move |_: MouseEvent| cb.emit((id.clone(), !active)))
                        }}>
                        if c.is_active {
                            { icons::x_circle("w-3.5 h-3.5") }
                            {"Deactivate"}
                        } else {
                            { icons::check_circle("w-3.5 h-3.5") }
                            {"Activate"}
                        }
                    </button>
                }
            </div>
            // Tags section
            <div class="px-5 py-4">
                <p class="text-xs font-semibold text-slate-400 uppercase tracking-wide mb-2">
                    {"Skills / Tags"}
                </p>
                if detail.tags.is_empty() {
                    <p class="text-sm text-slate-400">{"No tags assigned."}</p>
                } else {
                    <div class="flex flex-wrap gap-1.5">
                        { for detail.tags.iter().map(|t| html! {
                            <span class="inline-flex items-center gap-1 bg-indigo-50 text-indigo-700
                                         text-xs rounded-full px-2.5 py-0.5 font-medium">
                                { icons::tag("w-2.5 h-2.5") }
                                { t }
                            </span>
                        })}
                    </div>
                }
            </div>
        </div>
    }
}

// ── Sub-render: subscriptions panel ───────────────────────────────────────────

fn render_subscriptions(
    subs:     &UseStateHandle<Option<Vec<SubscriptionItem>>>,
    loading:  bool,
    err:      Option<&String>,
    on_unsub: Callback<(String, String)>,
) -> Html {
    html! {
        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
            <div class="px-5 py-3 border-b border-slate-100 bg-slate-50">
                <h3 class="text-xs font-semibold text-slate-500 uppercase tracking-wide">
                    {"My Subscriptions"}
                </h3>
            </div>
            if loading {
                <p class="px-5 py-3 text-sm text-slate-400 animate-pulse">{"Loading…"}</p>
            } else if let Some(msg) = err {
                <div class="flex items-center gap-1.5 px-5 py-3 text-xs text-red-600">
                    { icons::exclamation_triangle("w-3.5 h-3.5 shrink-0") }
                    { msg }
                </div>
            } else if let Some(list) = subs.as_ref() {
                if list.is_empty() {
                    <p class="px-5 py-3 text-sm text-slate-400">{"No active subscriptions."}</p>
                } else {
                    <div class="divide-y divide-slate-100">
                    { for list.iter().map(|sub| {
                        let tt = sub.target_type.clone();
                        let tid = sub.target_id.clone();
                        let on_click = {
                            let cb  = on_unsub.clone();
                            let tt2 = tt.clone();
                            let ti2 = tid.clone();
                            Callback::from(move |_: MouseEvent| cb.emit((tt2.clone(), ti2.clone())))
                        };
                        html! {
                            <div class="px-5 py-2.5 flex items-center justify-between gap-4">
                                <div class="flex items-center gap-2 min-w-0">
                                    { icons::bell("w-3.5 h-3.5 text-indigo-400 shrink-0") }
                                    <span class="text-sm text-slate-700 capitalize truncate">
                                        { &sub.target_type }
                                    </span>
                                    <span class="font-mono text-xs text-slate-400 truncate">
                                        { &sub.target_id[..8] }{"…"}
                                    </span>
                                </div>
                                <button
                                    class="text-xs font-medium text-red-500 hover:text-red-700
                                           px-2 py-0.5 rounded hover:bg-red-50 transition-colors
                                           whitespace-nowrap shrink-0"
                                    onclick={on_click}>
                                    {"Unsubscribe"}
                                </button>
                            </div>
                        }
                    })}
                    </div>
                }
            }
        </div>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tab_btn(label: &'static str, idx: u8, active: u8, cb: Callback<MouseEvent>) -> Html {
    let cls = if idx == active {
        "px-4 py-2.5 text-sm font-medium text-indigo-700 border-b-2 border-indigo-600 transition-colors"
    } else {
        "px-4 py-2.5 text-sm font-medium text-slate-500 hover:text-slate-800 border-b-2 border-transparent transition-colors"
    };
    html! {
        <button class={cls} onclick={cb}>{ label }</button>
    }
}
