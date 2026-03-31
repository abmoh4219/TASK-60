//! Operations console shell — sidebar + top-bar layout.
//!
//! Every ops page wraps its content with `<OpsLayout active={Route::OpsXxx}>`.
//! The layout reads `AuthContext` for the user name/role and for gating nav items.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api;
use crate::app::Route;
use crate::auth::{AuthAction, AuthContext};

// ── Props ──────────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct OpsLayoutProps {
    /// The currently active route (used to highlight the sidebar item).
    pub active:   Route,
    pub children: Children,
}

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsLayout)]
pub fn ops_layout(props: &OpsLayoutProps) -> Html {
    let auth      = use_context::<AuthContext>().expect("AuthContext missing");
    let navigator = use_navigator().unwrap();

    // Redirect to login if not authenticated.
    if !auth.is_authenticated() {
        return html! { <Redirect<Route> to={Route::Login} /> };
    }

    let user = auth.current_user().unwrap();

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

    html! {
        <div class="min-h-screen bg-gray-50 flex flex-col">
            // ── Top bar ───────────────────────────────────────────────────
            <header class="bg-blue-900 text-white h-14 flex items-center px-6 shrink-0 shadow-md">
                <Link<Route> to={Route::OpsSchedules}
                    classes="text-lg font-bold tracking-tight mr-8 hover:text-blue-200 transition">
                    {"RailOps Console"}
                </Link<Route>>
                <div class="flex-1"></div>
                <div class="flex items-center gap-4 text-sm">
                    <span class="text-blue-200">{ user.display_name() }</span>
                    <span class="text-xs bg-blue-700 rounded px-2 py-0.5">
                        { user.role_label() }
                    </span>
                    <button
                        onclick={on_signout}
                        class="rounded bg-blue-700 px-3 py-1.5 text-xs font-medium
                               hover:bg-blue-600 active:bg-blue-800 transition"
                    >
                        {"Sign out"}
                    </button>
                </div>
            </header>

            <div class="flex flex-1 overflow-hidden">
                // ── Sidebar ───────────────────────────────────────────────
                <nav class="w-52 shrink-0 bg-white border-r border-gray-200 py-4 flex flex-col gap-1">
                    { nav_item("Schedules", "🗓", Route::OpsSchedules, &props.active) }
                    { nav_item("Orders",    "🎟", Route::OpsOrders,    &props.active) }
                    <div class="mt-auto border-t border-gray-100 pt-3 px-3">
                        <Link<Route> to={Route::Kiosk}
                            classes="flex items-center gap-2 text-xs text-gray-400 \
                                     hover:text-blue-600 transition px-2 py-1">
                            {"← Passenger kiosk"}
                        </Link<Route>>
                    </div>
                </nav>

                // ── Page content ──────────────────────────────────────────
                <main class="flex-1 overflow-y-auto p-6">
                    { for props.children.iter() }
                </main>
            </div>
        </div>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn nav_item(label: &'static str, icon: &'static str, route: Route, active: &Route) -> Html {
    let is_active = &route == active;
    let class = if is_active {
        "flex items-center gap-3 mx-2 px-3 py-2.5 rounded-lg \
         bg-blue-50 text-blue-700 font-semibold text-sm"
    } else {
        "flex items-center gap-3 mx-2 px-3 py-2.5 rounded-lg \
         text-gray-700 text-sm hover:bg-gray-100 transition"
    };
    html! {
        <Link<Route> to={route} classes={class}>
            <span class="text-base">{ icon }</span>
            <span>{ label }</span>
        </Link<Route>>
    }
}

