//! Operations console shell — premium dark sidebar + top-bar layout.
//!
//! Features:
//!   • Collapsible sidebar (persisted to sessionStorage; `[` key hint shown)
//!   • Keyboard shortcut hints 1-5 on nav items
//!   • Role-gated nav items (Staffing, Credentials, Rules)
//!   • User profile card in sidebar footer with sign-out

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use shared::UserRole;

use crate::api;
use crate::app::Route;
use crate::auth::{AuthAction, AuthContext};
use crate::components::icons;

// ── Props ──────────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct OpsLayoutProps {
    pub active:   Route,
    pub children: Children,
}

// ── Nav item descriptor ────────────────────────────────────────────────────────

struct NavItem {
    route:   Route,
    label:   &'static str,
    shortcut: &'static str,
}

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsLayout)]
pub fn ops_layout(props: &OpsLayoutProps) -> Html {
    let auth      = use_context::<AuthContext>().expect("AuthContext missing");
    let navigator = use_navigator().unwrap();

    if !auth.is_authenticated() {
        return html! { <Redirect<Route> to={Route::Login} /> };
    }

    let user         = auth.current_user().unwrap();
    let is_admin     = user.role == UserRole::Admin;
    let can_staffing = matches!(
        user.role,
        UserRole::Admin | UserRole::Dispatcher | UserRole::OpsAgent
    );

    // ── Sidebar collapsed state (persisted) ──────────────────────────────
    let collapsed = use_state(|| {
        gloo_storage::SessionStorage::get::<bool>("railops_sidebar_collapsed")
            .unwrap_or(false)
    });

    let on_toggle = {
        let collapsed = collapsed.clone();
        Callback::from(move |_: MouseEvent| {
            let v = !*collapsed;
            collapsed.set(v);
            let _ = gloo_storage::SessionStorage::set("railops_sidebar_collapsed", v);
        })
    };

    // ── Sign-out ──────────────────────────────────────────────────────────
    let on_signout = {
        let auth      = auth.clone();
        let navigator = navigator.clone();
        Callback::from(move |_: MouseEvent| {
            let token     = auth.token.clone();
            let auth      = auth.clone();
            let navigator = navigator.clone();
            spawn_local(async move {
                if let Some(t) = token { let _ = api::logout(&t).await; }
                auth.dispatch(AuthAction::Logout);
                navigator.push(&Route::Login);
            });
        })
    };

    let col = *collapsed;

    // ── Build nav items ───────────────────────────────────────────────────
    let mut nav_items: Vec<(NavItem, Html)> = vec![
        (NavItem { route: Route::OpsSchedules, label: "Schedules",   shortcut: "1" },
         icons::calendar("w-5 h-5")),
        (NavItem { route: Route::OpsOrders,    label: "Orders",      shortcut: "2" },
         icons::ticket("w-5 h-5")),
    ];
    if can_staffing {
        nav_items.push((
            NavItem { route: Route::OpsStaffing,     label: "Staffing",     shortcut: "3" },
            icons::user_group("w-5 h-5"),
        ));
        nav_items.push((
            NavItem { route: Route::OpsCredentials,  label: "Credentials",  shortcut: "4" },
            icons::document_text("w-5 h-5"),
        ));
    }
    if is_admin {
        nav_items.push((
            NavItem { route: Route::OpsRules,  label: "Rules",   shortcut: "5" },
            icons::cog6("w-5 h-5"),
        ));
    }

    html! {
        <div class="flex h-screen overflow-hidden bg-slate-50">

            // ── Sidebar ───────────────────────────────────────────────────
            <aside class={format!(
                "sidebar-transition flex flex-col bg-slate-900 shrink-0 {}",
                if col { "w-[60px]" } else { "w-56" }
            )}>

                // Logo / brand
                <div class={format!(
                    "flex items-center h-14 shrink-0 px-3 border-b border-white/10 {}",
                    if col { "justify-center" } else { "justify-between" }
                )}>
                    if !col {
                        <div class="flex items-center gap-2 min-w-0">
                            <span class="text-indigo-400">{ icons::chart_bar("w-5 h-5") }</span>
                            <span class="text-white font-bold text-base tracking-tight truncate">
                                {"RailOps"}
                            </span>
                        </div>
                    } else {
                        <span class="text-indigo-400">{ icons::chart_bar("w-5 h-5") }</span>
                    }
                    <button
                        title={ if col { "Expand sidebar [" } else { "Collapse sidebar [" } }
                        onclick={on_toggle.clone()}
                        class="p-1.5 rounded-md text-slate-400 hover:text-white hover:bg-white/10
                               transition-colors focus-visible:ring-2 focus-visible:ring-indigo-500 shrink-0"
                    >
                        if col {
                            { icons::chevron_right("w-4 h-4") }
                        } else {
                            { icons::chevron_double_left("w-4 h-4") }
                        }
                    </button>
                </div>

                // Nav items
                <nav class="flex-1 overflow-y-auto scrollbar-thin py-4 px-2 space-y-0.5">
                    { nav_items.iter().map(|(item, icon_html)| {
                        nav_item(icon_html.clone(), item, &props.active, col)
                    }).collect::<Html>() }

                    // Divider
                    <div class="my-3 border-t border-white/10"></div>

                    // Kiosk link
                    {
                        let kiosk_item = NavItem { route: Route::Kiosk, label: "Passenger Kiosk", shortcut: "" };
                        nav_item(icons::home("w-5 h-5"), &kiosk_item, &props.active, col)
                    }
                </nav>

                // Sidebar footer — user info
                <div class="shrink-0 border-t border-white/10 p-3">
                    if col {
                        <div class="flex flex-col items-center gap-2">
                            <div class="w-8 h-8 rounded-full bg-indigo-600 flex items-center justify-center
                                        text-white text-xs font-bold shrink-0">
                                { user.display_name().chars().next().unwrap_or('?').to_uppercase().collect::<String>() }
                            </div>
                            <button
                                onclick={on_signout.clone()}
                                title="Sign out"
                                class="p-1.5 rounded-md text-slate-400 hover:text-red-400 hover:bg-white/10 transition-colors"
                            >
                                { icons::arrow_right_on_rect("w-4 h-4") }
                            </button>
                        </div>
                    } else {
                        <div class="flex items-center gap-3">
                            <div class="w-8 h-8 rounded-full bg-indigo-600 flex items-center justify-center
                                        text-white text-xs font-bold shrink-0">
                                { user.display_name().chars().next().unwrap_or('?').to_uppercase().collect::<String>() }
                            </div>
                            <div class="min-w-0 flex-1">
                                <p class="text-xs font-semibold text-white truncate">
                                    { user.display_name() }
                                </p>
                                <p class="text-[11px] text-slate-400 truncate">
                                    { user.role_label() }
                                </p>
                            </div>
                            <button
                                onclick={on_signout.clone()}
                                title="Sign out"
                                class="p-1.5 rounded-md text-slate-400 hover:text-red-400 hover:bg-white/10 transition-colors shrink-0"
                            >
                                { icons::arrow_right_on_rect("w-4 h-4") }
                            </button>
                        </div>
                        // Keyboard hint
                        <p class="mt-2.5 text-[10px] text-slate-600 text-center select-none">
                            {"[ to toggle • 1-5 to navigate"}
                        </p>
                    }
                </div>
            </aside>

            // ── Main area ─────────────────────────────────────────────────
            <div class="flex flex-col flex-1 min-w-0 overflow-hidden">

                // Top bar
                <header class="h-14 bg-white border-b border-slate-200/80 flex items-center
                               px-6 shrink-0 gap-4 shadow-[0_1px_0_0_rgb(0_0_0/0.05)]">
                    // Page title (filled by each page, show current route label)
                    <span class="text-sm font-semibold text-slate-400 uppercase tracking-wide">
                        { route_label(&props.active) }
                    </span>
                    <div class="flex-1"></div>
                    // User badge
                    <div class="flex items-center gap-3">
                        <span class="hidden sm:inline-flex items-center gap-1.5 rounded-full
                                     bg-indigo-50 px-2.5 py-1 text-xs font-medium text-indigo-700 border border-indigo-200/60">
                            { icons::shield_check("w-3.5 h-3.5") }
                            { user.role_label() }
                        </span>
                        <div class="hidden sm:block w-px h-5 bg-slate-200"></div>
                        <button
                            onclick={on_signout}
                            class="flex items-center gap-1.5 text-sm text-slate-500 hover:text-slate-800
                                   transition-colors px-2 py-1 rounded-lg hover:bg-slate-100"
                        >
                            { icons::arrow_right_on_rect("w-4 h-4") }
                            <span class="hidden sm:inline text-xs font-medium">{"Sign out"}</span>
                        </button>
                    </div>
                </header>

                // Page content
                <main class="flex-1 overflow-y-auto p-6">
                    { for props.children.iter() }
                </main>
            </div>
        </div>
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn nav_item(icon: Html, item: &NavItem, active: &Route, collapsed: bool) -> Html {
    let is_active = &item.route == active;
    let route = item.route.clone();

    let cls = if is_active {
        "flex items-center gap-3 w-full rounded-lg px-2.5 py-2 \
         bg-indigo-600 text-white font-medium text-sm transition-colors"
    } else {
        "flex items-center gap-3 w-full rounded-lg px-2.5 py-2 \
         text-slate-400 hover:text-white hover:bg-white/10 text-sm transition-colors"
    };

    html! {
        <Link<Route> to={route} classes={cls}
            title={if collapsed { item.label } else { "" }}>
            <span class="shrink-0">{ icon }</span>
            if !collapsed {
                <span class="flex-1 truncate">{ item.label }</span>
                if !item.shortcut.is_empty() {
                    <kbd class={format!(
                        "text-[10px] font-mono rounded px-1.5 py-0.5 border {}",
                        if is_active {
                            "border-white/30 bg-white/20 text-white/70"
                        } else {
                            "border-white/10 bg-white/5 text-slate-500"
                        }
                    )}>
                        { item.shortcut }
                    </kbd>
                }
            }
        </Link<Route>>
    }
}

fn route_label(r: &Route) -> &'static str {
    match r {
        Route::OpsSchedules   => "Schedules",
        Route::OpsOrders      => "Orders",
        Route::OpsStaffing    => "Staffing",
        Route::OpsCredentials => "Credentials",
        Route::OpsRules       => "Rules",
        Route::Kiosk          => "Kiosk",
        _                     => "RailOps",
    }
}
