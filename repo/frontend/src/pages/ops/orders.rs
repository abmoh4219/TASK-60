//! Operations console — orders & passengers page.
//!
//! Two tabs:
//!   • Orders  — search by order number or passenger; paginated list; click for
//!               full detail + event timeline + role-gated action buttons.
//!   • Passengers — search by name; paginated list; PII purge request.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use shared::UserRole;

use crate::api::ops::{
    self as ops_api,
    CancelBody, FeeOverrideBody, OrderDetailResponse, OrderItem, Page,
    PassengerItem, RebookBody, RefundBody,
};
use crate::app::Route;
use crate::auth::AuthContext;
use crate::components::{
    empty_state, icons, input_cls, select_cls, skeleton_rows, status_badge,
    btn_primary, btn_secondary, btn_danger,
};
use crate::components::toast::{use_toasts, ToastKind};

use super::OpsLayout;

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsOrdersPage)]
pub fn ops_orders_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();
    let user  = auth.current_user().unwrap();

    let can_manage_orders  = matches!(user.role, UserRole::Admin | UserRole::OpsAgent | UserRole::CsAgent);
    let can_process_refund = matches!(user.role, UserRole::Admin | UserRole::OpsAgent | UserRole::CsAgent);
    let can_override_fee   = matches!(user.role, UserRole::Admin | UserRole::OpsAgent | UserRole::CsAgent);

    let toasts = use_toasts();

    // ── Tab: 0=orders  1=passengers ───────────────────────────────────────
    let active_tab = use_state(|| 0u8);

    // ── Orders tab state ──────────────────────────────────────────────────
    let order_search   = use_state(String::new);
    let order_status   = use_state(String::new);
    let order_page     = use_state(|| 1i64);
    let orders         = use_state(|| None::<Page<OrderItem>>);
    let orders_loading = use_state(|| false);

    // ── Passenger tab state ───────────────────────────────────────────────
    let pax_search   = use_state(String::new);
    let pax_page     = use_state(|| 1i64);
    let passengers   = use_state(|| None::<Page<PassengerItem>>);
    let pax_loading  = use_state(|| false);
    let pax_purge_ok = use_state(|| None::<String>);

    // ── Order detail ──────────────────────────────────────────────────────
    let selected_order_id = use_state(|| None::<String>);
    let order_detail      = use_state(|| None::<OrderDetailResponse>);
    let detail_loading    = use_state(|| false);

    // ── Action forms: 0=none 1=confirm 2=cancel 3=refund 4=fee-override 5=disruption 6=rebook
    let action          = use_state(|| 0u8);
    let form_reason     = use_state(String::new);
    let form_amount     = use_state(String::new);
    let form_disrupt    = use_state(|| false);
    let form_schedule   = use_state(String::new);  // for rebook
    let action_err      = use_state(|| None::<String>);
    let action_ok       = use_state(|| false);

    // ── Load orders ───────────────────────────────────────────────────────
    {
        let orders = orders.clone();
        let loading = orders_loading.clone();
        let token   = token.clone();
        let search  = (*order_search).clone();
        let status  = (*order_status).clone();
        let pg      = *order_page;
        let tab     = *active_tab;
        use_effect_with((search.clone(), status.clone(), pg, tab), move |_| {
            if tab == 0 {
                loading.set(true);
                spawn_local(async move {
                    let result = if !search.is_empty() && search.starts_with("ORD-") {
                        // Exact order-number lookup
                        ops_api::find_order_by_number(&token, &search).await
                            .map(|o| Page { items: vec![o], total: 1, page: 1, per_page: 20, total_pages: 1 })
                    } else if !search.is_empty() {
                        // Free-text: search by passenger name OR phone last-4 (digits only)
                        let is_digits = search.chars().all(|c| c.is_ascii_digit());
                        let (pax_name, pax_phone) = if is_digits {
                            (None, Some(search.as_str()))
                        } else {
                            (Some(search.as_str()), None)
                        };
                        ops_api::list_orders(&token, None, None, Some(&status), pax_name, pax_phone, pg).await
                    } else {
                        ops_api::list_orders(&token, None, None, Some(&status), None, None, pg).await
                    };
                    match result {
                        Ok(p)  => { orders.set(Some(p)); loading.set(false); }
                        Err(_) => { loading.set(false); }
                    }
                });
            }
            || ()
        });
    }

    // ── Load passengers ───────────────────────────────────────────────────
    {
        let passengers = passengers.clone();
        let loading    = pax_loading.clone();
        let token      = token.clone();
        let search     = (*pax_search).clone();
        let pg         = *pax_page;
        let tab        = *active_tab;
        use_effect_with((search.clone(), pg, tab), move |_| {
            if tab == 1 {
                loading.set(true);
                spawn_local(async move {
                    match ops_api::search_passengers(&token, &search, pg).await {
                        Ok(p)  => { passengers.set(Some(p)); loading.set(false); }
                        Err(_) => { loading.set(false); }
                    }
                });
            }
            || ()
        });
    }

    // ── Load order detail when selection changes ───────────────────────────
    {
        let order_detail   = order_detail.clone();
        let detail_loading = detail_loading.clone();
        let action         = action.clone();
        let action_ok      = action_ok.clone();
        let token          = token.clone();
        let oid            = (*selected_order_id).clone();
        use_effect_with(oid.clone(), move |oid| {
            if let Some(id) = oid {
                detail_loading.set(true);
                action.set(0); action_ok.set(false);
                let id = id.clone();
                spawn_local(async move {
                    match ops_api::get_order(&token, &id).await {
                        Ok(d)  => { order_detail.set(Some(d)); detail_loading.set(false); }
                        Err(_) => { order_detail.set(None);    detail_loading.set(false); }
                    }
                });
            } else {
                order_detail.set(None);
                action.set(0);
            }
            || ()
        });
    }

    // ── Action dispatcher ─────────────────────────────────────────────────
    let dispatch_action = {
        let (token, order_detail, selected_order_id) =
            (token.clone(), order_detail.clone(), selected_order_id.clone());
        let (action, action_err, action_ok) = (action.clone(), action_err.clone(), action_ok.clone());
        let (form_reason, form_amount, form_disrupt, form_schedule) =
            (form_reason.clone(), form_amount.clone(), form_disrupt.clone(), form_schedule.clone());
        let toast_push = toasts.push.clone();
        Callback::from(move |kind: u8| {
            let id = match (*selected_order_id).clone() { Some(i) => i, None => return };
            let reason    = (*form_reason).trim().to_owned();
            let amount    = (*form_amount).trim().to_owned();
            let disrupt   = *form_disrupt;
            let schedule  = (*form_schedule).trim().to_owned();
            let (token, order_detail, action, action_err, action_ok) =
                (token.clone(), order_detail.clone(), action.clone(), action_err.clone(), action_ok.clone());
            let toast_push = toast_push.clone();
            spawn_local(async move {
                // Rebook returns a different type, handle separately.
                if kind == 6 {
                    match ops_api::rebook_order(&token, &id, &RebookBody {
                        new_schedule_id:   schedule,
                        new_seat_class_id: None,
                        new_seat_number:   None,
                        new_fare_amount:   None,
                        reason:            if reason.is_empty() { None } else { Some(reason) },
                    }).await {
                        Ok(r) => {
                            if let Ok(d) = ops_api::get_order(&token, &id).await {
                                order_detail.set(Some(d));
                            }
                            action.set(0); action_err.set(None); action_ok.set(true);
                            toast_push.emit((
                                format!("Rebooked → {}", r.new_order_number),
                                ToastKind::Success,
                            ));
                        }
                        Err(e) => {
                            let msg = e.message.clone();
                            action_err.set(Some(e.message));
                            toast_push.emit((msg, ToastKind::Error));
                        }
                    }
                    return;
                }
                let result: Result<(), _> = match kind {
                    1 => ops_api::confirm_order(&token, &id).await,
                    2 => ops_api::cancel_order(&token, &id, &CancelBody {
                            reason,
                            disruption_flag: disrupt,
                            refund_amount:   None,
                        }).await,
                    3 => ops_api::process_refund(&token, &id, &RefundBody { amount }).await,
                    4 => ops_api::apply_fee_override(&token, &id, &FeeOverrideBody {
                            override_amount: amount,
                            reason,
                        }).await,
                    5 => ops_api::flag_disruption(&token, &id).await,
                    _ => return,
                };
                match result {
                    Ok(_) => {
                        if let Ok(d) = ops_api::get_order(&token, &id).await {
                            order_detail.set(Some(d));
                        }
                        action.set(0); action_err.set(None); action_ok.set(true);
                        toast_push.emit(("Action completed successfully.".into(), ToastKind::Success));
                    }
                    Err(e) => {
                        let msg = e.message.clone();
                        action_err.set(Some(e.message));
                        toast_push.emit((msg, ToastKind::Error));
                    }
                }
            });
        })
    };

    // ── Render ────────────────────────────────────────────────────────────
    html! {
        <>
        <OpsLayout active={Route::OpsOrders}>
            <div class="space-y-5">

                // ── Tab bar ───────────────────────────────────────────────
                <div class="flex border-b border-slate-200">
                    { tab_btn("Orders",     0, &active_tab) }
                    { tab_btn("Passengers", 1, &active_tab) }
                </div>

                // ── Orders tab ────────────────────────────────────────────
                if *active_tab == 0 {
                    <div class="space-y-4">
                        // Search + filter bar
                        <div class="flex flex-wrap gap-3">
                            <div class="relative flex-1 min-w-[280px] max-w-sm">
                                <span class="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400">
                                    { icons::magnifying_glass("w-4 h-4") }
                                </span>
                                <input type="text"
                                    placeholder="Order number (RO-…) or search…"
                                    class={format!("pl-9 {}", input_cls())}
                                    value={(*order_search).clone()}
                                    oninput={{ let s = order_search.clone(); let p = order_page.clone();
                                        Callback::from(move |e: InputEvent| {
                                            s.set(ev_input(&e)); p.set(1);
                                        })
                                    }} />
                            </div>
                            <select class={select_cls()}
                                onchange={{ let s = order_status.clone(); let p = order_page.clone();
                                    Callback::from(move |e: Event| { s.set(ev_val(&e)); p.set(1); })
                                }}>
                                <option value="">{"All statuses"}</option>
                                <option value="pending">{"Pending"}</option>
                                <option value="held">{"Held"}</option>
                                <option value="confirmed">{"Confirmed"}</option>
                                <option value="cancelled">{"Cancelled"}</option>
                                <option value="refunded">{"Refunded"}</option>
                                <option value="completed">{"Completed"}</option>
                            </select>
                        </div>

                        <div class="flex gap-5 items-start">
                            // Order list
                            <div class="flex-1 min-w-0 space-y-4">
                                if *orders_loading {
                                    <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                        <table class="min-w-full">
                                            <tbody>{ skeleton_rows(6) }</tbody>
                                        </table>
                                    </div>
                                } else if let Some(p) = &*orders {
                                    if p.items.is_empty() {
                                        { empty_state(icons::ticket("w-10 h-10"), "No orders found",
                                            "Try a different search term or status filter.") }
                                    } else {
                                        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                            <table class="min-w-full text-sm">
                                                <thead class="bg-slate-50 border-b border-slate-200/60">
                                                    <tr>
                                                        <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Order #"}</th>
                                                        <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Status"}</th>
                                                        <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Fare"}</th>
                                                        <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Created"}</th>
                                                    </tr>
                                                </thead>
                                                <tbody class="divide-y divide-slate-100">
                                                { for p.items.iter().map(|o| {
                                                    let is_sel = selected_order_id.as_deref() == Some(&o.id);
                                                    let cls = if is_sel {
                                                        "bg-indigo-50 cursor-pointer border-l-2 border-l-indigo-500"
                                                    } else {
                                                        "cursor-pointer hover:bg-slate-50/60 transition-colors"
                                                    };
                                                    let id  = o.id.clone();
                                                    let sel = selected_order_id.clone();
                                                    let aok = action_ok.clone();
                                                    html! {
                                                        <tr class={cls}
                                                            onclick={Callback::from(move |_| {
                                                                sel.set(Some(id.clone())); aok.set(false);
                                                            })}>
                                                            <td class="px-4 py-3.5 font-mono text-sm font-semibold text-indigo-700">
                                                                { &o.order_number }
                                                            </td>
                                                            <td class="px-4 py-3.5">
                                                                { status_badge(&o.status) }
                                                            </td>
                                                            <td class="px-4 py-3.5 text-sm text-slate-700 font-medium">
                                                                { format!("${}", &o.fare_amount) }
                                                            </td>
                                                            <td class="px-4 py-3.5 text-xs text-slate-400 tabular-nums">
                                                                { crate::api::ops::fmt_dt(&o.created_at) }
                                                            </td>
                                                        </tr>
                                                    }
                                                })}
                                                </tbody>
                                            </table>
                                        </div>
                                        { pagination_html(&order_page, p.total_pages) }
                                    }
                                }
                            </div>

                            // Order detail panel
                            if selected_order_id.is_some() {
                                <div class="w-80 shrink-0 bg-white rounded-xl border border-slate-200/80 shadow-card self-start overflow-hidden">
                                    if *detail_loading {
                                        <div class="p-5 space-y-3">
                                            <div class="skeleton h-5 w-32"></div>
                                            <div class="skeleton h-4 w-48"></div>
                                            <div class="skeleton h-4 w-40"></div>
                                        </div>
                                    } else if let Some(resp) = &*order_detail {
                                        { order_detail_html(resp, can_manage_orders, can_process_refund, can_override_fee, &action, &action_err, &action_ok, &form_reason, &form_amount, &form_disrupt, &form_schedule, &dispatch_action) }
                                        <div class="px-5 py-3 border-t border-slate-100">
                                            <button
                                                class="w-full flex items-center justify-center gap-1.5 text-xs
                                                       text-slate-400 hover:text-slate-600 transition-colors py-1"
                                                onclick={{ let s = selected_order_id.clone(); Callback::from(move |_| s.set(None)) }}>
                                                { icons::x_mark("w-3.5 h-3.5") }
                                                {"Close panel"}
                                            </button>
                                        </div>
                                    }
                                </div>
                            }
                        </div>
                    </div>
                }

                // ── Passengers tab ────────────────────────────────────────
                if *active_tab == 1 {
                    <div class="space-y-4">
                        <div class="relative max-w-sm">
                            <span class="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400">
                                { icons::magnifying_glass("w-4 h-4") }
                            </span>
                            <input type="text"
                                placeholder="Search by name or phone last 4…"
                                class={format!("pl-9 {}", input_cls())}
                                value={(*pax_search).clone()}
                                oninput={{ let s = pax_search.clone(); let p = pax_page.clone();
                                    Callback::from(move |e: InputEvent| { s.set(ev_input(&e)); p.set(1); })
                                }} />
                        </div>

                        if *pax_loading {
                            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                <table class="min-w-full">
                                    <tbody>{ skeleton_rows(5) }</tbody>
                                </table>
                            </div>
                        } else if let Some(p) = &*passengers {
                            if p.items.is_empty() {
                                { empty_state(icons::users("w-10 h-10"), "No passengers found",
                                    "Try a different name or phone search.") }
                            } else {
                                <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                    <table class="min-w-full text-sm">
                                        <thead class="bg-slate-50 border-b border-slate-200/60">
                                            <tr>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Name"}</th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Phone"}</th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"PII Status"}</th>
                                                if can_manage_orders {
                                                    <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{"Actions"}</th>
                                                }
                                            </tr>
                                        </thead>
                                        <tbody class="divide-y divide-slate-100">
                                        { for p.items.iter().map(|pax| {
                                            let pax_id = pax.id.clone();
                                            let pax_purge_ok = pax_purge_ok.clone();
                                            let token2 = token.clone();
                                            let just_purged = pax_purge_ok.as_deref() == Some(&pax.id);
                                            html! {
                                                <tr class="hover:bg-slate-50/60 transition-colors">
                                                    <td class="px-4 py-3.5 text-sm text-slate-900 font-medium">
                                                        { &pax.full_name }
                                                    </td>
                                                    <td class="px-4 py-3.5 font-mono text-sm text-slate-500">
                                                        { pax.masked_phone() }
                                                    </td>
                                                    <td class="px-4 py-3.5">
                                                        if pax.is_purged() {
                                                            <span class="text-xs text-slate-400 italic">{"Purged"}</span>
                                                        } else if pax.pii_purge_requested_at.is_some() {
                                                            <span class="inline-flex items-center gap-1 text-xs text-amber-600 font-medium">
                                                                { icons::clock("w-3 h-3") }
                                                                {"Purge requested"}
                                                            </span>
                                                        } else {
                                                            <span class="inline-flex items-center gap-1 text-xs text-emerald-600 font-medium">
                                                                { icons::check_circle("w-3 h-3") }
                                                                {"Active"}
                                                            </span>
                                                        }
                                                    </td>
                                                    if can_manage_orders {
                                                        <td class="px-4 py-3.5">
                                                            if just_purged {
                                                                <span class="inline-flex items-center gap-1 text-xs text-emerald-600 font-medium">
                                                                    { icons::check_circle("w-3.5 h-3.5") }
                                                                    {"Requested"}
                                                                </span>
                                                            } else if !pax.is_purged() && pax.pii_purge_requested_at.is_none() {
                                                                <button
                                                                    class="text-xs font-medium text-red-600 hover:text-red-700 transition-colors"
                                                                    onclick={Callback::from(move |_| {
                                                                        let pax_id      = pax_id.clone();
                                                                        let pax_purge_ok = pax_purge_ok.clone();
                                                                        let token2      = token2.clone();
                                                                        spawn_local(async move {
                                                                            if ops_api::request_pii_purge(&token2, &pax_id).await.is_ok() {
                                                                                pax_purge_ok.set(Some(pax_id));
                                                                            }
                                                                        });
                                                                    })}>
                                                                    {"Request PII purge"}
                                                                </button>
                                                            }
                                                        </td>
                                                    }
                                                </tr>
                                            }
                                        })}
                                        </tbody>
                                    </table>
                                </div>
                                { pagination_html(&pax_page, p.total_pages) }
                            }
                        }
                    </div>
                }
            </div>
        </OpsLayout>
        { toasts.view }
        </>
    }
}

// ── Order detail panel content ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn order_detail_html(
    resp:          &OrderDetailResponse,
    can_manage:    bool,
    can_refund:    bool,
    can_fee:       bool,
    action:        &UseStateHandle<u8>,
    action_err:    &UseStateHandle<Option<String>>,
    action_ok:     &UseStateHandle<bool>,
    form_reason:   &UseStateHandle<String>,
    form_amount:   &UseStateHandle<String>,
    form_disrupt:  &UseStateHandle<bool>,
    form_schedule: &UseStateHandle<String>,
    dispatch:      &Callback<u8>,
) -> Html {
    let o = &resp.order;
    html! {
        <>
            // Header
            <div class="px-5 py-4 border-b border-slate-100">
                <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                        <p class="font-mono font-semibold text-indigo-700 text-sm">
                            { &o.order_number }
                        </p>
                        <p class="text-sm text-slate-800 font-medium mt-0.5">{ &o.passenger_name }</p>
                        <p class="text-xs text-slate-400 font-mono">{ o.masked_phone() }</p>
                    </div>
                    { status_badge(&o.status) }
                </div>
            </div>

            // Journey details
            <div class="px-5 py-3 border-b border-slate-100 space-y-1.5">
                <div class="flex items-center gap-1.5 text-xs text-slate-600">
                    { icons::ticket("w-3.5 h-3.5 text-slate-400 shrink-0") }
                    { format!("{} ({})", o.route_name, o.route_code) }
                </div>
                <div class="flex items-center gap-1.5 text-xs text-slate-600">
                    { icons::calendar("w-3.5 h-3.5 text-slate-400 shrink-0") }
                    { format!("Train {}  ·  {}", o.train_number, o.fmt_departure()) }
                </div>
                <div class="flex items-center gap-1.5 text-xs text-slate-600">
                    { icons::tag("w-3.5 h-3.5 text-slate-400 shrink-0") }
                    { format!("{} — {}", o.seat_class_name, o.seat_number.as_deref().unwrap_or("—")) }
                </div>
                <div class="flex items-center justify-between">
                    <span class="text-sm font-semibold text-slate-900">
                        { format!("${}", o.fare_amount) }
                    </span>
                    if o.disruption_flag {
                        <span class="inline-flex items-center gap-1 text-xs text-amber-600 font-medium">
                            { icons::exclamation_triangle("w-3.5 h-3.5") }
                            {"Disruption"}
                        </span>
                    }
                </div>
            </div>

            // Action success banner
            if **action_ok {
                <div class="mx-5 my-3 flex items-center gap-2 rounded-lg bg-emerald-50
                            border border-emerald-200 px-3 py-2 text-xs text-emerald-700">
                    { icons::check_circle("w-4 h-4 text-emerald-500 shrink-0") }
                    {"Action applied successfully"}
                </div>
            }

            // Action buttons
            <div class="px-5 py-4 space-y-3">
                if can_manage && **action == 0 {
                    <div class="flex flex-wrap gap-1.5">
                        if matches!(o.status.as_str(), "pending" | "held") {
                            <ActionBtn label="Confirm" kind=1u8 style="success" dispatch={dispatch.clone()} />
                        }
                        if matches!(o.status.as_str(), "pending" | "held" | "confirmed") {
                            <ActionBtn label="Cancel"  kind=2u8 style="danger"   dispatch={dispatch.clone()} />
                        }
                        if can_refund && o.status == "cancelled" {
                            <ActionBtn label="Refund"       kind=3u8 style="warning"  dispatch={dispatch.clone()} />
                        }
                        if can_fee {
                            <ActionBtn label="Fee Override" kind=4u8 style="secondary" dispatch={dispatch.clone()} />
                        }
                        if !o.disruption_flag {
                            <ActionBtn label="Flag Disruption" kind=5u8 style="secondary" dispatch={dispatch.clone()} />
                        }
                        if can_manage && matches!(o.status.as_str(), "confirmed" | "cancelled") {
                            <ActionBtn label="Rebook" kind=6u8 style="secondary" dispatch={dispatch.clone()} />
                        }
                    </div>
                }

                // Cancel form
                if **action == 2 {
                    <div class="space-y-2.5">
                        <p class="text-xs font-semibold text-slate-600">{"Cancel Order"}</p>
                        <textarea placeholder="Cancellation reason (required)"
                            rows="2"
                            class="block w-full rounded-lg border border-slate-300 bg-white px-3 py-2 \
                                   text-sm text-slate-900 placeholder-slate-400 shadow-sm resize-none \
                                   focus:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/20"
                            oninput={{ let s = form_reason.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }}>
                        </textarea>
                        <label class="flex items-center gap-2 text-xs text-slate-600 cursor-pointer">
                            <input type="checkbox"
                                class="rounded border-slate-300 text-indigo-600 focus:ring-indigo-500"
                                onchange={{ let d = form_disrupt.clone(); Callback::from(move |e: Event| {
                                    use web_sys::HtmlInputElement;
                                    if let Some(el) = e.target().and_then(|t| t.dyn_into::<HtmlInputElement>().ok()) {
                                        d.set(el.checked());
                                    }
                                })}} />
                            {"Service disruption"}
                        </label>
                        { err_html(action_err) }
                        { form_btns(dispatch, action, action_err, 2) }
                    </div>
                }

                // Refund form
                if **action == 3 {
                    <div class="space-y-2.5">
                        <p class="text-xs font-semibold text-slate-600">{"Process Refund"}</p>
                        <input type="number" step="0.01" placeholder="Refund amount (USD)"
                            class={input_cls()}
                            oninput={{ let s = form_amount.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                        { err_html(action_err) }
                        { form_btns(dispatch, action, action_err, 3) }
                    </div>
                }

                // Fee override form
                if **action == 4 {
                    <div class="space-y-2.5">
                        <p class="text-xs font-semibold text-slate-600">{"Fee Override"}</p>
                        <input type="number" step="0.01" placeholder="Override amount (USD)"
                            class={input_cls()}
                            oninput={{ let s = form_amount.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                        <input type="text" placeholder="Reason (required)"
                            class={input_cls()}
                            oninput={{ let s = form_reason.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                        { err_html(action_err) }
                        { form_btns(dispatch, action, action_err, 4) }
                    </div>
                }

                // Rebook form
                if **action == 6 {
                    <div class="space-y-2.5">
                        <p class="text-xs font-semibold text-slate-600">{"Rebook onto New Schedule"}</p>
                        <input type="text" placeholder="New schedule UUID (required)"
                            class={input_cls()}
                            value={(**form_schedule).clone()}
                            oninput={{ let s = form_schedule.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                        <input type="text" placeholder="Reason (optional)"
                            class={input_cls()}
                            oninput={{ let s = form_reason.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                        { err_html(action_err) }
                        { form_btns(dispatch, action, action_err, 6) }
                    </div>
                }
            </div>

            // Event timeline
            if !resp.events.is_empty() {
                <div class="px-5 pb-4">
                    <p class="text-[11px] font-semibold text-slate-500 uppercase tracking-wide mb-3">
                        {"Event Timeline"}
                    </p>
                    <div class="space-y-2">
                    { for resp.events.iter().map(|ev| html! {
                        <div class="flex items-start gap-2.5">
                            <div class="mt-0.5 w-1.5 h-1.5 rounded-full bg-slate-300 shrink-0 mt-1.5"></div>
                            <div class="min-w-0 flex-1">
                                <div class="flex items-baseline gap-1.5 flex-wrap">
                                    <span class="text-xs font-semibold text-slate-700">
                                        { &ev.event_type }
                                    </span>
                                    if let Some(r) = &ev.reason {
                                        <span class="text-xs text-slate-500 truncate">{ r }</span>
                                    }
                                </div>
                                <p class="text-[10px] text-slate-400 tabular-nums">
                                    { crate::api::ops::fmt_dt(&ev.created_at) }
                                </p>
                            </div>
                        </div>
                    })}
                    </div>
                </div>
            }
        </>
    }
}

// ── Sub-component: action button ───────────────────────────────────────────────

#[derive(Properties, PartialEq)]
struct ActionBtnProps {
    label:    &'static str,
    kind:     u8,
    style:    &'static str,
    dispatch: Callback<u8>,
}

#[function_component(ActionBtn)]
fn action_btn_comp(props: &ActionBtnProps) -> Html {
    let kind     = props.kind;
    let dispatch = props.dispatch.clone();
    let cls = match props.style {
        "success"   => "inline-flex items-center gap-1 rounded-lg bg-emerald-600 px-2.5 py-1.5 \
                        text-xs font-semibold text-white hover:bg-emerald-700 transition-colors",
        "danger"    => "inline-flex items-center gap-1 rounded-lg bg-red-600 px-2.5 py-1.5 \
                        text-xs font-semibold text-white hover:bg-red-700 transition-colors",
        "warning"   => "inline-flex items-center gap-1 rounded-lg bg-amber-600 px-2.5 py-1.5 \
                        text-xs font-semibold text-white hover:bg-amber-700 transition-colors",
        _ /* secondary */ => "inline-flex items-center gap-1 rounded-lg border border-slate-300 \
                        bg-white px-2.5 py-1.5 text-xs font-semibold text-slate-700 \
                        hover:bg-slate-50 transition-colors",
    };
    html! {
        <button class={cls} onclick={Callback::from(move |_| dispatch.emit(kind))}>
            { props.label }
        </button>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tab_btn(label: &'static str, idx: u8, active: &UseStateHandle<u8>) -> Html {
    let is_active = **active == idx;
    let cls = if is_active {
        "px-4 py-2.5 text-sm font-medium text-indigo-700 border-b-2 border-indigo-600 transition-colors"
    } else {
        "px-4 py-2.5 text-sm font-medium text-slate-500 hover:text-slate-800 \
         border-b-2 border-transparent transition-colors"
    };
    let active = active.clone();
    html! {
        <button class={cls} onclick={Callback::from(move |_| active.set(idx))}>
            { label }
        </button>
    }
}

fn pagination_html(page: &UseStateHandle<i64>, total_pages: i64) -> Html {
    if total_pages <= 1 { return html! {}; }
    let cur = **page;
    html! {
        <div class="flex items-center gap-3">
            if cur > 1 {
                <button
                    class={btn_secondary()}
                    onclick={{ let p = page.clone(); Callback::from(move |_| p.set(cur - 1)) }}>
                    { icons::chevron_left("w-4 h-4") }
                    {"Prev"}
                </button>
            }
            <span class="text-sm text-slate-500">
                { format!("Page {} of {}", cur, total_pages) }
            </span>
            if cur < total_pages {
                <button
                    class={btn_secondary()}
                    onclick={{ let p = page.clone(); Callback::from(move |_| p.set(cur + 1)) }}>
                    {"Next"}
                    { icons::chevron_right("w-4 h-4") }
                </button>
            }
        </div>
    }
}

fn form_btns(
    dispatch:   &Callback<u8>,
    action:     &UseStateHandle<u8>,
    action_err: &UseStateHandle<Option<String>>,
    kind:       u8,
) -> Html {
    let dispatch   = dispatch.clone();
    let action_c   = action.clone();
    let action_err = action_err.clone();
    html! {
        <div class="flex gap-2">
            <button type="button"
                class={btn_primary()}
                onclick={Callback::from(move |_| dispatch.emit(kind))}>
                {"Submit"}
            </button>
            <button type="button"
                class={btn_secondary()}
                onclick={Callback::from(move |_| { action_c.set(0); action_err.set(None); })}>
                {"Cancel"}
            </button>
        </div>
    }
}

fn err_html(err: &UseStateHandle<Option<String>>) -> Html {
    match err.as_ref() {
        Some(msg) => html! {
            <div class="flex items-center gap-1.5 text-xs text-red-600">
                { icons::exclamation_triangle("w-3.5 h-3.5 shrink-0") }
                { msg }
            </div>
        },
        None => html! {},
    }
}

fn ev_val(e: &Event) -> String {
    let target = match e.target() { Some(t) => t, None => return String::new() };
    if let Some(el) = target.dyn_ref::<HtmlInputElement>() { return el.value(); }
    if let Some(el) = target.dyn_ref::<HtmlSelectElement>() { return el.value(); }
    String::new()
}

fn ev_input(e: &InputEvent) -> String {
    use wasm_bindgen::JsCast;
    e.target()
        .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}
