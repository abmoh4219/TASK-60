//! Operations console — schedules & inventory page.
//!
//! Shows a filterable schedule table.  Clicking a row opens a detail panel
//! on the right with live inventory and (for Admin/OpsAgent) action forms to
//! update the schedule status or correct inventory counts.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use shared::UserRole;

use crate::api::ops::{
    self as ops_api, CorrectInventoryBody, Page, RouteItem, ScheduleDetail,
    ScheduleRow, UpdateStatusBody,
};
use crate::app::Route;
use crate::auth::AuthContext;
use crate::components::{
    empty_state, icons, input_cls, select_cls, skeleton_rows, status_badge,
    btn_primary, btn_secondary,
};

use super::OpsLayout;

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsSchedulesPage)]
pub fn ops_schedules_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();
    let user  = auth.current_user().unwrap();
    let can_manage = matches!(user.role, UserRole::Admin | UserRole::OpsAgent);

    // ── Filters ───────────────────────────────────────────────────────────
    let filter_route  = use_state(String::new);
    let filter_status = use_state(String::new);
    let page          = use_state(|| 1i64);

    // ── Table data ────────────────────────────────────────────────────────
    let routes    = use_state(Vec::<RouteItem>::new);
    let schedules = use_state(|| None::<Page<ScheduleRow>>);
    let loading   = use_state(|| true);

    // ── Selection + detail ────────────────────────────────────────────────
    let selected_id    = use_state(|| None::<String>);
    let detail         = use_state(|| None::<ScheduleDetail>);
    let detail_loading = use_state(|| false);

    // ── Action panel: 0=none  1=update-status  2=correct-inventory ────────
    let action      = use_state(|| 0u8);
    let st_status   = use_state(String::new);
    let st_delay    = use_state(String::new);
    let st_platform = use_state(String::new);
    let inv_class   = use_state(String::new);
    let inv_total   = use_state(String::new);
    let inv_avail   = use_state(String::new);
    let action_err  = use_state(|| None::<String>);
    let action_ok   = use_state(|| false);

    // ── Load route reference data once ───────────────────────────────────
    {
        let routes = routes.clone();
        let token  = token.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                if let Ok(r) = ops_api::list_routes(&token).await {
                    routes.set(r);
                }
            });
            || ()
        });
    }

    // ── Reload schedules when filters / page change ───────────────────────
    {
        let schedules = schedules.clone();
        let loading   = loading.clone();
        let token     = token.clone();
        let route_id  = (*filter_route).clone();
        let status    = (*filter_status).clone();
        let pg        = *page;
        use_effect_with((route_id.clone(), status.clone(), pg), move |_| {
            loading.set(true);
            spawn_local(async move {
                match ops_api::list_schedules(
                    &token, Some(&route_id), Some(&status), None, pg, 20,
                ).await {
                    Ok(p)  => { schedules.set(Some(p)); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Load detail when selection changes ────────────────────────────────
    {
        let detail         = detail.clone();
        let detail_loading = detail_loading.clone();
        let action         = action.clone();
        let action_ok      = action_ok.clone();
        let token          = token.clone();
        let sid            = (*selected_id).clone();
        use_effect_with(sid.clone(), move |sid| {
            if let Some(id) = sid {
                detail_loading.set(true);
                action.set(0);
                action_ok.set(false);
                let id = id.clone();
                spawn_local(async move {
                    match ops_api::get_schedule(&token, &id).await {
                        Ok(d)  => { detail.set(Some(d)); detail_loading.set(false); }
                        Err(_) => { detail.set(None);    detail_loading.set(false); }
                    }
                });
            } else {
                detail.set(None);
                action.set(0);
            }
            || ()
        });
    }

    // ── Status update handler ─────────────────────────────────────────────
    let on_update_status = {
        let (token, detail, selected_id) = (token.clone(), detail.clone(), selected_id.clone());
        let (st_status, st_delay, st_platform) = (st_status.clone(), st_delay.clone(), st_platform.clone());
        let (action, action_err, action_ok)    = (action.clone(), action_err.clone(), action_ok.clone());
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let id   = match (*selected_id).clone() { Some(i) => i, None => return };
            let body = UpdateStatusBody {
                status:        (*st_status).clone(),
                delay_minutes: (*st_delay).parse::<i32>().ok().filter(|&d| d > 0),
                platform:      non_empty(&st_platform),
            };
            let (token, detail, action, action_err, action_ok) =
                (token.clone(), detail.clone(), action.clone(), action_err.clone(), action_ok.clone());
            spawn_local(async move {
                match ops_api::update_schedule_status(&token, &id, &body).await {
                    Ok(_) => {
                        if let Ok(d) = ops_api::get_schedule(&token, &id).await {
                            detail.set(Some(d));
                        }
                        action.set(0); action_err.set(None); action_ok.set(true);
                    }
                    Err(e) => { action_err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Inventory correction handler ──────────────────────────────────────
    let on_correct_inv = {
        let (token, detail, selected_id) = (token.clone(), detail.clone(), selected_id.clone());
        let (inv_class, inv_total, inv_avail) = (inv_class.clone(), inv_total.clone(), inv_avail.clone());
        let (action, action_err, action_ok)   = (action.clone(), action_err.clone(), action_ok.clone());
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let id    = match (*selected_id).clone() { Some(i) => i, None => return };
            let total = match (*inv_total).parse::<i32>() {
                Ok(v) => v,
                Err(_) => { action_err.set(Some("Total seats must be a number".into())); return; }
            };
            let avail = match (*inv_avail).parse::<i32>() {
                Ok(v) => v,
                Err(_) => { action_err.set(Some("Available seats must be a number".into())); return; }
            };
            let body = CorrectInventoryBody {
                seat_class_id:   (*inv_class).clone(),
                total_seats:     total,
                available_seats: avail,
            };
            let (token, detail, action, action_err, action_ok) =
                (token.clone(), detail.clone(), action.clone(), action_err.clone(), action_ok.clone());
            spawn_local(async move {
                match ops_api::correct_inventory(&token, &id, &body).await {
                    Ok(_) => {
                        if let Ok(d) = ops_api::get_schedule(&token, &id).await {
                            detail.set(Some(d));
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
        <OpsLayout active={Route::OpsSchedules}>
            <div class="space-y-5">

                // ── Filter bar ────────────────────────────────────────────
                <div class="flex flex-wrap items-center gap-3">
                    <span class="text-slate-400 shrink-0">
                        { icons::funnel("w-4 h-4") }
                    </span>
                    <select class={select_cls()}
                        onchange={{
                            let filter_route = filter_route.clone();
                            let page = page.clone();
                            Callback::from(move |e: Event| {
                                filter_route.set(ev_val(&e)); page.set(1);
                            })
                        }}>
                        <option value="">{"All routes"}</option>
                        { for routes.iter().map(|r| html! {
                            <option value={r.id.clone()}>
                                { &r.route_code }{ " — " }{ &r.name }
                            </option>
                        })}
                    </select>
                    <select class={select_cls()}
                        onchange={{
                            let filter_status = filter_status.clone();
                            let page = page.clone();
                            Callback::from(move |e: Event| {
                                filter_status.set(ev_val(&e)); page.set(1);
                            })
                        }}>
                        <option value="">{"All statuses"}</option>
                        <option value="scheduled">{"Scheduled"}</option>
                        <option value="delayed">{"Delayed"}</option>
                        <option value="cancelled">{"Cancelled"}</option>
                        <option value="completed">{"Completed"}</option>
                    </select>
                </div>

                <div class="flex gap-5 items-start">

                    // ── Table ─────────────────────────────────────────────
                    <div class="flex-1 min-w-0 space-y-4">
                        if *loading {
                            <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                <table class="min-w-full">
                                    <thead class="bg-slate-50 border-b border-slate-200/60">
                                        <tr>
                                            { for ["Train","Route","Departure","Status","Platform"].iter().map(|h| html! {
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    { *h }
                                                </th>
                                            }) }
                                        </tr>
                                    </thead>
                                    <tbody>{ skeleton_rows(8) }</tbody>
                                </table>
                            </div>
                        } else if let Some(p) = &*schedules {
                            if p.items.is_empty() {
                                { empty_state(
                                    icons::calendar("w-10 h-10"),
                                    "No schedules found",
                                    "Try adjusting the route or status filter.",
                                ) }
                            } else {
                                <div class="bg-white rounded-xl border border-slate-200/80 shadow-card overflow-hidden">
                                    <table class="min-w-full text-sm">
                                        <thead class="bg-slate-50 border-b border-slate-200/60">
                                            <tr>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    {"Train"}
                                                </th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    {"Route"}
                                                </th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    {"Departure"}
                                                </th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    {"Status"}
                                                </th>
                                                <th class="px-4 py-3 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                                                    {"Platform"}
                                                </th>
                                            </tr>
                                        </thead>
                                        <tbody class="divide-y divide-slate-100">
                                        { for p.items.iter().map(|s| {
                                            let is_sel = selected_id.as_deref() == Some(&s.id);
                                            let row_cls = if is_sel {
                                                "bg-indigo-50 cursor-pointer border-l-2 border-l-indigo-500 transition-colors"
                                            } else {
                                                "cursor-pointer hover:bg-slate-50/60 transition-colors"
                                            };
                                            let id          = s.id.clone();
                                            let selected_id = selected_id.clone();
                                            let action_ok   = action_ok.clone();
                                            html! {
                                                <tr class={row_cls}
                                                    onclick={Callback::from(move |_| {
                                                        selected_id.set(Some(id.clone()));
                                                        action_ok.set(false);
                                                    })}>
                                                    <td class="px-4 py-3.5 font-mono text-sm font-medium text-slate-800">
                                                        { &s.train_number }
                                                    </td>
                                                    <td class="px-4 py-3.5 text-sm text-slate-600">
                                                        { &s.route_code }
                                                    </td>
                                                    <td class="px-4 py-3.5 text-sm text-slate-600">
                                                        { s.fmt_departure() }
                                                    </td>
                                                    <td class="px-4 py-3.5">
                                                        { status_badge(&s.status) }
                                                    </td>
                                                    <td class="px-4 py-3.5 text-sm text-slate-400">
                                                        { s.platform.as_deref().unwrap_or("—") }
                                                    </td>
                                                </tr>
                                            }
                                        })}
                                        </tbody>
                                    </table>
                                </div>
                                if p.total_pages > 1 {
                                    <div class="flex items-center gap-3">
                                        if *page > 1 {
                                            <button class={btn_secondary()}
                                                onclick={{ let page = page.clone(); Callback::from(move |_| page.set(*page - 1)) }}>
                                                { icons::chevron_left("w-4 h-4") }
                                                {"Prev"}
                                            </button>
                                        }
                                        <span class="text-sm text-slate-500">
                                            { format!("Page {} of {}", *page, p.total_pages) }
                                        </span>
                                        if *page < p.total_pages {
                                            <button class={btn_secondary()}
                                                onclick={{ let page = page.clone(); Callback::from(move |_| page.set(*page + 1)) }}>
                                                {"Next"}
                                                { icons::chevron_right("w-4 h-4") }
                                            </button>
                                        }
                                    </div>
                                }
                            }
                        }
                    </div>

                    // ── Detail panel ──────────────────────────────────────
                    if selected_id.is_some() {
                        <div class="w-80 shrink-0 bg-white rounded-xl border border-slate-200/80 shadow-card self-start overflow-hidden">
                            if *detail_loading {
                                <div class="p-5 space-y-3">
                                    <div class="skeleton h-5 w-32"></div>
                                    <div class="skeleton h-4 w-48"></div>
                                    <div class="skeleton h-4 w-40"></div>
                                </div>
                            } else if let Some(d) = &*detail {
                                // Header
                                <div class="px-5 py-4 border-b border-slate-100 flex items-start justify-between gap-3">
                                    <div class="min-w-0">
                                        <p class="font-mono font-semibold text-slate-900">
                                            { &d.schedule.train_number }
                                        </p>
                                        <p class="text-xs text-slate-500 mt-0.5">
                                            { d.schedule.route_label() }
                                        </p>
                                        <p class="text-xs text-slate-400 mt-0.5">
                                            { d.schedule.fmt_departure() }{ " → " }{ d.schedule.fmt_arrival() }
                                        </p>
                                        if d.schedule.delay_minutes > 0 {
                                            <p class="text-xs text-amber-600 mt-1 font-medium">
                                                { icons::clock("w-3.5 h-3.5 inline -mt-0.5 mr-0.5") }
                                                { format!("Delayed {} min", d.schedule.delay_minutes) }
                                            </p>
                                        }
                                    </div>
                                    { status_badge(&d.schedule.status) }
                                </div>

                                // Inventory
                                if !d.inventory.is_empty() {
                                    <div class="px-5 py-3 border-b border-slate-100">
                                        <p class="text-[11px] font-semibold text-slate-500 uppercase tracking-wide mb-2">
                                            {"Inventory"}
                                        </p>
                                        <div class="space-y-1.5">
                                        { for d.inventory.iter().map(|inv| html! {
                                            <div class="flex items-center justify-between text-xs">
                                                <span class="font-medium text-slate-700">
                                                    { &inv.seat_class_code }
                                                </span>
                                                <div class="flex items-center gap-2">
                                                    <div class="w-24 h-1.5 rounded-full bg-slate-100 overflow-hidden">
                                                        <div
                                                            class="h-full rounded-full bg-indigo-500"
                                                            style={format!("width: {}%", 100 - inv.occupancy_pct())}
                                                        ></div>
                                                    </div>
                                                    <span class="text-slate-500 tabular-nums">
                                                        { format!("{}/{}", inv.available_seats, inv.total_seats) }
                                                    </span>
                                                </div>
                                            </div>
                                        })}
                                        </div>
                                    </div>
                                }

                                // Success banner
                                if *action_ok {
                                    <div class="mx-5 my-3 flex items-center gap-2 rounded-lg bg-emerald-50
                                                border border-emerald-200 px-3 py-2 text-xs text-emerald-700">
                                        { icons::check_circle("w-4 h-4 text-emerald-500 shrink-0") }
                                        {"Updated successfully"}
                                    </div>
                                }

                                // Actions
                                <div class="px-5 py-4 space-y-3">
                                    if can_manage {
                                        if *action == 0 {
                                            <div class="flex gap-2">
                                                <button
                                                    class={btn_primary()}
                                                    onclick={{ let action = action.clone(); let aok = action_ok.clone();
                                                        Callback::from(move |_| { action.set(1); aok.set(false); }) }}>
                                                    { icons::arrow_path("w-4 h-4") }
                                                    {"Update Status"}
                                                </button>
                                                <button
                                                    class={crate::components::btn_secondary()}
                                                    onclick={{ let action = action.clone(); let aok = action_ok.clone();
                                                        Callback::from(move |_| { action.set(2); aok.set(false); }) }}>
                                                    {"Fix Inventory"}
                                                </button>
                                            </div>
                                        }

                                        // Status update form
                                        if *action == 1 {
                                            <form class="space-y-3" onsubmit={on_update_status.clone()}>
                                                <p class="text-xs font-semibold text-slate-600">
                                                    {"Update Schedule Status"}
                                                </p>
                                                <select class={input_cls()}
                                                    onchange={{ let s = st_status.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }}>
                                                    <option value="">{"— select status —"}</option>
                                                    <option value="scheduled">{"Scheduled"}</option>
                                                    <option value="delayed">{"Delayed"}</option>
                                                    <option value="cancelled">{"Cancelled"}</option>
                                                    <option value="completed">{"Completed"}</option>
                                                </select>
                                                <input type="number" placeholder="Delay minutes (optional)"
                                                    class={input_cls()}
                                                    onchange={{ let s = st_delay.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }} />
                                                <input type="text" placeholder="Platform (optional)"
                                                    class={input_cls()}
                                                    onchange={{ let s = st_platform.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }} />
                                                { err_html(&action_err) }
                                                <div class="flex gap-2">
                                                    <button type="submit" class={btn_primary()}>{"Save"}</button>
                                                    <button type="button" class={btn_secondary()}
                                                        onclick={{ let action = action.clone(); let e = action_err.clone();
                                                            Callback::from(move |_| { action.set(0); e.set(None); }) }}>
                                                        {"Cancel"}
                                                    </button>
                                                </div>
                                            </form>
                                        }

                                        // Inventory correction form
                                        if *action == 2 {
                                            <form class="space-y-3" onsubmit={on_correct_inv.clone()}>
                                                <p class="text-xs font-semibold text-slate-600">
                                                    {"Correct Inventory"}
                                                </p>
                                                <select class={input_cls()}
                                                    onchange={{ let s = inv_class.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }}>
                                                    <option value="">{"— seat class —"}</option>
                                                    { for d.inventory.iter().map(|inv| html! {
                                                        <option value={inv.seat_class_id.clone()}>
                                                            { &inv.seat_class_code }{ " (" }{ inv.seat_class_name.as_str() }{ ")" }
                                                        </option>
                                                    })}
                                                </select>
                                                <input type="number" placeholder="Total seats"
                                                    class={input_cls()}
                                                    onchange={{ let s = inv_total.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }} />
                                                <input type="number" placeholder="Available seats"
                                                    class={input_cls()}
                                                    onchange={{ let s = inv_avail.clone(); Callback::from(move |e: Event| s.set(ev_val(&e))) }} />
                                                { err_html(&action_err) }
                                                <div class="flex gap-2">
                                                    <button type="submit" class={btn_primary()}>{"Apply"}</button>
                                                    <button type="button" class={btn_secondary()}
                                                        onclick={{ let action = action.clone(); let e = action_err.clone();
                                                            Callback::from(move |_| { action.set(0); e.set(None); }) }}>
                                                        {"Cancel"}
                                                    </button>
                                                </div>
                                            </form>
                                        }
                                    }

                                    // Close
                                    <button
                                        class="w-full flex items-center justify-center gap-1.5 text-xs
                                               text-slate-400 hover:text-slate-600 transition-colors py-1"
                                        onclick={{ let s = selected_id.clone(); Callback::from(move |_| s.set(None)) }}>
                                        { icons::x_mark("w-3.5 h-3.5") }
                                        {"Close panel"}
                                    </button>
                                </div>
                            }
                        </div>
                    }
                </div>
            </div>
        </OpsLayout>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ev_val(e: &Event) -> String {
    let target = match e.target() { Some(t) => t, None => return String::new() };
    if let Some(el) = target.dyn_ref::<HtmlInputElement>() {
        return el.value();
    }
    if let Some(el) = target.dyn_ref::<HtmlSelectElement>() {
        return el.value();
    }
    String::new()
}

fn non_empty(s: &UseStateHandle<String>) -> Option<String> {
    let v = (**s).trim().to_owned();
    if v.is_empty() { None } else { Some(v) }
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
