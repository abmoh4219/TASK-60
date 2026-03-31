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
    PassengerItem, RefundBody,
};
use crate::app::Route;
use crate::auth::AuthContext;

use super::OpsLayout;

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsOrdersPage)]
pub fn ops_orders_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();
    let user  = auth.current_user().unwrap();

    // Role gates
    let can_manage_orders  = matches!(user.role, UserRole::Admin | UserRole::OpsAgent | UserRole::CsAgent);
    let can_process_refund = matches!(user.role, UserRole::Admin | UserRole::OpsAgent | UserRole::CsAgent);
    let can_override_fee   = matches!(user.role, UserRole::Admin | UserRole::OpsAgent);

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
    let pax_purge_ok = use_state(|| None::<String>); // id that was just purged

    // ── Order detail ──────────────────────────────────────────────────────
    let selected_order_id = use_state(|| None::<String>);
    let order_detail      = use_state(|| None::<OrderDetailResponse>);
    let detail_loading    = use_state(|| false);

    // ── Action forms: 0=none 1=confirm 2=cancel 3=refund 4=fee-override 5=disruption
    let action         = use_state(|| 0u8);
    let form_reason    = use_state(String::new);
    let form_amount    = use_state(String::new);
    let form_disrupt   = use_state(|| false);
    let action_err     = use_state(|| None::<String>);
    let action_ok      = use_state(|| false);

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
            if tab != 0 { return || (); }
            loading.set(true);
            spawn_local(async move {
                // If search looks like an order number (e.g. "RO-12345"), use by-number lookup.
                let result = if !search.is_empty() && search.starts_with("RO") {
                    ops_api::find_order_by_number(&token, &search).await
                        .map(|o| Page { items: vec![o], total: 1, page: 1, per_page: 20, total_pages: 1 })
                } else {
                    let pax_id = if !search.is_empty() { None } else { None };
                    ops_api::list_orders(&token, pax_id, None, Some(&status), pg).await
                };
                match result {
                    Ok(p)  => { orders.set(Some(p)); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
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
            if tab != 1 { return || (); }
            loading.set(true);
            spawn_local(async move {
                match ops_api::search_passengers(&token, &search, pg).await {
                    Ok(p)  => { passengers.set(Some(p)); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
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
        let (form_reason, form_amount, form_disrupt) =
            (form_reason.clone(), form_amount.clone(), form_disrupt.clone());
        Callback::from(move |kind: u8| {
            let id = match (*selected_order_id).clone() { Some(i) => i, None => return };
            let reason   = (*form_reason).trim().to_owned();
            let amount   = (*form_amount).trim().to_owned();
            let disrupt  = *form_disrupt;
            let (token, order_detail, action, action_err, action_ok) =
                (token.clone(), order_detail.clone(), action.clone(), action_err.clone(), action_ok.clone());
            spawn_local(async move {
                let result: Result<(), _> = match kind {
                    1 => ops_api::confirm_order(&token, &id).await,
                    2 => ops_api::cancel_order(&token, &id, &CancelBody {
                            reason:          reason,
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
                    }
                    Err(e) => { action_err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Render ────────────────────────────────────────────────────────────
    html! {
        <OpsLayout active={Route::OpsOrders}>
            <div class="space-y-4">
                <h1 class="text-xl font-semibold text-gray-900">{"Orders & Passengers"}</h1>

                // Tab bar
                <div class="flex border-b border-gray-200">
                    { tab_btn("Orders",     0, &active_tab) }
                    { tab_btn("Passengers", 1, &active_tab) }
                </div>

                // ── Orders tab ────────────────────────────────────────────
                if *active_tab == 0 {
                    <div class="space-y-3">
                        // Search + status filter
                        <div class="flex flex-wrap gap-3">
                            <input type="text" placeholder="Order number (RO-…) or search…"
                                class="rounded border border-gray-300 text-sm px-3 py-1.5 w-72"
                                value={(*order_search).clone()}
                                oninput={{ let s = order_search.clone(); let p = order_page.clone();
                                    Callback::from(move |e: InputEvent| {
                                        s.set(ev_input(&e)); p.set(1);
                                    })
                                }} />
                            <select class="rounded border border-gray-300 text-sm px-3 py-1.5"
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

                        <div class="flex gap-4 items-start">
                            // Order list
                            <div class="flex-1 min-w-0">
                                if *orders_loading {
                                    <p class="text-sm text-gray-400 animate-pulse">{"Loading…"}</p>
                                } else if let Some(p) = &*orders {
                                    if p.items.is_empty() {
                                        <p class="text-sm text-gray-500">{"No orders found."}</p>
                                    } else {
                                        <div class="overflow-x-auto rounded-lg border border-gray-200 bg-white">
                                            <table class="min-w-full text-sm">
                                                <thead class="bg-gray-50 text-gray-600 text-xs uppercase tracking-wide">
                                                    <tr>
                                                        <th class="px-4 py-3 text-left">{"Order #"}</th>
                                                        <th class="px-4 py-3 text-left">{"Status"}</th>
                                                        <th class="px-4 py-3 text-left">{"Fare"}</th>
                                                        <th class="px-4 py-3 text-left">{"Created"}</th>
                                                    </tr>
                                                </thead>
                                                <tbody class="divide-y divide-gray-100">
                                                { for p.items.iter().map(|o| {
                                                    let is_sel = selected_order_id.as_deref() == Some(&o.id);
                                                    let cls = if is_sel {
                                                        "bg-blue-50 cursor-pointer hover:bg-blue-100 transition"
                                                    } else {
                                                        "cursor-pointer hover:bg-gray-50 transition"
                                                    };
                                                    let id  = o.id.clone();
                                                    let sel = selected_order_id.clone();
                                                    let aok = action_ok.clone();
                                                    html! {
                                                        <tr class={cls}
                                                            onclick={Callback::from(move |_| { sel.set(Some(id.clone())); aok.set(false); })}>
                                                            <td class="px-4 py-3 font-mono text-blue-700">{ &o.order_number }</td>
                                                            <td class="px-4 py-3">
                                                                <span class={format!("text-xs font-medium px-2 py-0.5 rounded {}", o.status_color())}>{ &o.status }</span>
                                                            </td>
                                                            <td class="px-4 py-3 text-gray-700">{ format!("${}", &o.fare_amount) }</td>
                                                            <td class="px-4 py-3 text-gray-500 text-xs">{ crate::api::ops::fmt_dt(&o.created_at) }</td>
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
                                <div class="w-80 shrink-0 bg-white rounded-lg border border-gray-200 p-4 space-y-3 self-start">
                                    if *detail_loading {
                                        <p class="text-sm text-gray-400 animate-pulse">{"Loading…"}</p>
                                    } else if let Some(resp) = &*order_detail {
                                        { order_detail_html(resp, can_manage_orders, can_process_refund, can_override_fee, &action, &action_err, &action_ok, &form_reason, &form_amount, &form_disrupt, &dispatch_action) }
                                        <button class="w-full text-xs text-gray-400 hover:text-gray-600 text-center pt-1"
                                            onclick={{ let s = selected_order_id.clone(); Callback::from(move |_| s.set(None)) }}>
                                            {"✕ Close"}
                                        </button>
                                    }
                                </div>
                            }
                        </div>
                    </div>
                }

                // ── Passengers tab ────────────────────────────────────────
                if *active_tab == 1 {
                    <div class="space-y-3">
                        <input type="text" placeholder="Search by name or phone last 4…"
                            class="rounded border border-gray-300 text-sm px-3 py-1.5 w-72"
                            value={(*pax_search).clone()}
                            oninput={{ let s = pax_search.clone(); let p = pax_page.clone();
                                Callback::from(move |e: InputEvent| { s.set(ev_input(&e)); p.set(1); })
                            }} />

                        if *pax_loading {
                            <p class="text-sm text-gray-400 animate-pulse">{"Loading…"}</p>
                        } else if let Some(p) = &*passengers {
                            if p.items.is_empty() {
                                <p class="text-sm text-gray-500">{"No passengers found."}</p>
                            } else {
                                <div class="overflow-x-auto rounded-lg border border-gray-200 bg-white">
                                    <table class="min-w-full text-sm">
                                        <thead class="bg-gray-50 text-gray-600 text-xs uppercase tracking-wide">
                                            <tr>
                                                <th class="px-4 py-3 text-left">{"Name"}</th>
                                                <th class="px-4 py-3 text-left">{"Phone"}</th>
                                                <th class="px-4 py-3 text-left">{"PII Status"}</th>
                                                if can_manage_orders { <th class="px-4 py-3 text-left">{"Actions"}</th> }
                                            </tr>
                                        </thead>
                                        <tbody class="divide-y divide-gray-100">
                                        { for p.items.iter().map(|pax| {
                                            let pax_id = pax.id.clone();
                                            let pax_purge_ok = pax_purge_ok.clone();
                                            let token2 = token.clone();
                                            let just_purged = pax_purge_ok.as_deref() == Some(&pax.id);
                                            html! {
                                                <tr class="hover:bg-gray-50">
                                                    <td class="px-4 py-3 text-gray-900">{ &pax.full_name }</td>
                                                    <td class="px-4 py-3 font-mono text-gray-600">{ pax.masked_phone() }</td>
                                                    <td class="px-4 py-3 text-xs">
                                                        if pax.is_purged() {
                                                            <span class="text-gray-400 italic">{"Purged"}</span>
                                                        } else if pax.pii_purge_requested_at.is_some() {
                                                            <span class="text-amber-600">{"Purge requested"}</span>
                                                        } else {
                                                            <span class="text-green-600">{"Active"}</span>
                                                        }
                                                    </td>
                                                    if can_manage_orders {
                                                        <td class="px-4 py-3">
                                                            if just_purged {
                                                                <span class="text-xs text-green-600 font-medium">{"✓ Requested"}</span>
                                                            } else if !pax.is_purged() && pax.pii_purge_requested_at.is_none() {
                                                                <button
                                                                    class="text-xs text-red-600 hover:underline"
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
    }
}

// ── Order detail panel content ─────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn order_detail_html(
    resp:               &OrderDetailResponse,
    can_manage:         bool,
    can_refund:         bool,
    can_fee:            bool,
    action:             &UseStateHandle<u8>,
    action_err:         &UseStateHandle<Option<String>>,
    action_ok:          &UseStateHandle<bool>,
    form_reason:        &UseStateHandle<String>,
    form_amount:        &UseStateHandle<String>,
    form_disrupt:       &UseStateHandle<bool>,
    dispatch:           &Callback<u8>,
) -> Html {
    let o = &resp.order;
    html! {
        <>
            // Header
            <div>
                <div class="flex items-center justify-between">
                    <span class="font-mono font-semibold text-blue-700">{ &o.order_number }</span>
                    <span class={format!("text-xs px-2 py-0.5 rounded font-medium {}", o.status_color())}>
                        { &o.status }
                    </span>
                </div>
                <p class="text-sm text-gray-800 mt-0.5">{ &o.passenger_name }</p>
                <p class="text-xs text-gray-500">{ o.masked_phone() }</p>
            </div>

            // Journey
            <div class="text-xs space-y-0.5 text-gray-600">
                <p>{ format!("{} ({})", o.route_name, o.route_code) }</p>
                <p>{ format!("Train {}  ·  {}", o.train_number, o.fmt_departure()) }</p>
                <p>{ format!("{} — {}", o.seat_class_name, o.seat_number.as_deref().unwrap_or("—")) }</p>
                <p class="font-semibold text-gray-800">{ format!("Fare: ${}", o.fare_amount) }</p>
                if o.disruption_flag {
                    <p class="text-amber-600 font-medium">{"⚠ Disruption flagged"}</p>
                }
            </div>

            // Action success banner
            if **action_ok {
                <p class="text-xs text-green-700 font-medium bg-green-50 rounded px-2 py-1">{"✓ Action applied"}</p>
            }

            // Action buttons
            if can_manage && **action == 0 {
                <div class="flex flex-wrap gap-1.5">
                    if matches!(o.status.as_str(), "pending" | "held") {
                        <ActionBtn label="Confirm" kind=1u8 cls="bg-green-600 hover:bg-green-700" dispatch={dispatch.clone()} />
                    }
                    if matches!(o.status.as_str(), "pending" | "held" | "confirmed") {
                        <ActionBtn label="Cancel"       kind=2u8 cls="bg-red-600 hover:bg-red-700"    dispatch={dispatch.clone()} />
                    }
                    if can_refund && o.status == "cancelled" {
                        <ActionBtn label="Refund"       kind=3u8 cls="bg-purple-600 hover:bg-purple-700" dispatch={dispatch.clone()} />
                    }
                    if can_fee {
                        <ActionBtn label="Fee Override" kind=4u8 cls="bg-amber-600 hover:bg-amber-700"   dispatch={dispatch.clone()} />
                    }
                    if !o.disruption_flag {
                        <ActionBtn label="Flag Disruption" kind=5u8 cls="bg-slate-600 hover:bg-slate-700" dispatch={dispatch.clone()} />
                    }
                </div>
            }

            // Cancel form
            if **action == 2 {
                <div class="space-y-2">
                    <p class="text-xs font-semibold text-gray-600">{"Cancel Order"}</p>
                    <textarea placeholder="Cancellation reason (required)"
                        rows="2"
                        class="w-full rounded border border-gray-300 text-xs px-2 py-1.5 resize-none"
                        oninput={{ let s = form_reason.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }}>
                    </textarea>
                    <label class="flex items-center gap-2 text-xs">
                        <input type="checkbox"
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
                <div class="space-y-2">
                    <p class="text-xs font-semibold text-gray-600">{"Process Refund"}</p>
                    <input type="number" step="0.01" placeholder="Refund amount (USD)"
                        class="w-full rounded border border-gray-300 text-xs px-2 py-1.5"
                        oninput={{ let s = form_amount.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                    { err_html(action_err) }
                    { form_btns(dispatch, action, action_err, 3) }
                </div>
            }

            // Fee override form
            if **action == 4 {
                <div class="space-y-2">
                    <p class="text-xs font-semibold text-gray-600">{"Fee Override"}</p>
                    <input type="number" step="0.01" placeholder="Override amount (USD)"
                        class="w-full rounded border border-gray-300 text-xs px-2 py-1.5"
                        oninput={{ let s = form_amount.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                    <input type="text" placeholder="Reason (required)"
                        class="w-full rounded border border-gray-300 text-xs px-2 py-1.5"
                        oninput={{ let s = form_reason.clone(); Callback::from(move |e: InputEvent| s.set(ev_input(&e))) }} />
                    { err_html(action_err) }
                    { form_btns(dispatch, action, action_err, 4) }
                </div>
            }

            // Event timeline
            if !resp.events.is_empty() {
                <div>
                    <p class="text-xs font-semibold text-gray-500 uppercase mb-1">{"Event Timeline"}</p>
                    <div class="space-y-1">
                    { for resp.events.iter().map(|ev| html! {
                        <div class="text-xs border-l-2 border-gray-200 pl-2 py-0.5">
                            <span class="font-medium text-gray-700">{ &ev.event_type }</span>
                            if let Some(r) = &ev.reason {
                                <span class="text-gray-500">{ " — " }{ r }</span>
                            }
                            <p class="text-gray-400">{ crate::api::ops::fmt_dt(&ev.created_at) }</p>
                        </div>
                    })}
                    </div>
                </div>
            }
        </>
    }
}

// ── Sub-component: small action button ────────────────────────────────────────

#[derive(Properties, PartialEq)]
struct ActionBtnProps {
    label:    &'static str,
    kind:     u8,
    cls:      &'static str,
    dispatch: Callback<u8>,
}

#[function_component(ActionBtn)]
fn action_btn_comp(props: &ActionBtnProps) -> Html {
    let kind     = props.kind;
    let dispatch = props.dispatch.clone();
    html! {
        <button
            class={format!("rounded px-2.5 py-1 text-xs text-white transition {}", props.cls)}
            onclick={Callback::from(move |_| dispatch.emit(kind))}>
            { props.label }
        </button>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tab_btn(label: &'static str, idx: u8, active: &UseStateHandle<u8>) -> Html {
    let is_active = **active == idx;
    let cls = if is_active {
        "px-4 py-2 text-sm font-medium text-blue-700 border-b-2 border-blue-600"
    } else {
        "px-4 py-2 text-sm font-medium text-gray-600 hover:text-gray-900 border-b-2 border-transparent"
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
        <div class="mt-3 flex items-center gap-2 text-sm">
            if cur > 1 {
                <button class="px-3 py-1 rounded border border-gray-300 hover:bg-gray-100"
                    onclick={{ let p = page.clone(); Callback::from(move |_| p.set(cur - 1)) }}>
                    {"← Prev"}
                </button>
            }
            <span class="text-gray-500">{ format!("Page {} of {}", cur, total_pages) }</span>
            if cur < total_pages {
                <button class="px-3 py-1 rounded border border-gray-300 hover:bg-gray-100"
                    onclick={{ let p = page.clone(); Callback::from(move |_| p.set(cur + 1)) }}>
                    {"Next →"}
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
                class="flex-1 rounded bg-blue-600 px-3 py-1.5 text-xs text-white hover:bg-blue-700"
                onclick={Callback::from(move |_| dispatch.emit(kind))}>
                {"Submit"}
            </button>
            <button type="button"
                class="rounded border border-gray-300 px-3 py-1.5 text-xs hover:bg-gray-50"
                onclick={Callback::from(move |_| { action_c.set(0); action_err.set(None); })}>
                {"Cancel"}
            </button>
        </div>
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

fn err_html(err: &UseStateHandle<Option<String>>) -> Html {
    match err.as_ref() {
        Some(msg) => html! { <p class="text-xs text-red-600">{ msg }</p> },
        None      => html! {},
    }
}
