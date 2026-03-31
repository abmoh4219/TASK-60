//! Root application component.
//!
//! Responsibilities:
//!   • Provide `AuthContext` to the entire tree.
//!   • On first render, attempt to verify any stored token by calling /me.
//!   • Route between Login and Dashboard, enforcing auth guards declaratively.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{
    api,
    auth::{AuthAction, AuthContext, AuthState, AuthStatus},
    pages::LoginPage,
};

// ── Routes ────────────────────────────────────────────────────────────────────

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/login")]
    Login,
    #[at("/dashboard")]
    Dashboard,
    #[not_found]
    #[at("/404")]
    NotFound,
}

// ── Root app component ────────────────────────────────────────────────────────

#[function_component(App)]
pub fn app() -> Html {
    let auth = use_reducer(AuthState::load);

    // Verify any token found in sessionStorage.
    {
        let auth = auth.clone();
        use_effect_with((), move |_| {
            if let Some(token) = auth.token.clone() {
                let auth = auth.clone();
                spawn_local(async move {
                    match api::me(&token).await {
                        Ok(user) => auth.dispatch(AuthAction::SetUser { token, user }),
                        Err(_)   => auth.dispatch(AuthAction::ClearSession),
                    }
                });
            }
            || ()
        });
    }

    html! {
        <ContextProvider<AuthContext> context={auth}>
            <HashRouter>
                <Switch<Route> render={switch} />
            </HashRouter>
        </ContextProvider<AuthContext>>
    }
}

// ── Route switch ──────────────────────────────────────────────────────────────

fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! { <Redirect<Route> to={Route::Dashboard} /> },
        Route::Login => html! { <LoginPage /> },
        Route::Dashboard => html! { <RequireAuth><DashboardPlaceholder /></RequireAuth> },
        Route::NotFound => html! {
            <div class="flex items-center justify-center min-h-screen">
                <p class="text-gray-500">{"404 — Page not found"}</p>
            </div>
        },
    }
}

// ── Auth guard component ──────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
struct RequireAuthProps {
    children: Children,
}

/// Wraps protected routes.  Redirects to /login while unauthenticated;
/// shows a loading indicator while the stored token is being verified.
#[function_component(RequireAuth)]
fn require_auth(props: &RequireAuthProps) -> Html {
    let auth = use_context::<AuthContext>().expect("AuthContext missing");

    match &auth.status {
        AuthStatus::Loading => html! {
            <div class="flex items-center justify-center min-h-screen">
                <p class="text-gray-500 animate-pulse">{"Verifying session…"}</p>
            </div>
        },
        AuthStatus::Unauthenticated => html! {
            <Redirect<Route> to={Route::Login} />
        },
        AuthStatus::Authenticated(_) => html! {
            { for props.children.iter() }
        },
    }
}

// ── Dashboard placeholder (replaced in later steps) ──────────────────────────

#[function_component(DashboardPlaceholder)]
fn dashboard_placeholder() -> Html {
    let auth      = use_context::<AuthContext>().expect("AuthContext missing");
    let navigator = use_navigator().unwrap();

    let on_logout = {
        let auth      = auth.clone();
        let navigator = navigator.clone();
        Callback::from(move |_: MouseEvent| {
            let token     = auth.token.clone();
            let auth      = auth.clone();
            let navigator = navigator.clone();
            spawn_local(async move {
                if let Some(t) = token {
                    let _ = api::logout(&t).await;
                }
                auth.dispatch(AuthAction::Logout);
                navigator.push(&Route::Login);
            });
        })
    };

    let user = auth.current_user();
    html! {
        <div class="min-h-screen bg-gray-50 p-8">
            <div class="max-w-2xl mx-auto bg-white rounded-xl shadow p-8 space-y-4">
                <h1 class="text-2xl font-bold text-gray-900">{"RailOps Dashboard"}</h1>
                if let Some(u) = user {
                    <p class="text-gray-600">
                        {"Welcome, "}<strong>{ u.display_name() }</strong>
                        {" ("}<span class="text-blue-600">{ u.role_label() }</span>{")"}
                    </p>
                }
                <p class="text-sm text-gray-400">
                    {"Full dashboard is implemented in step 3."}
                </p>
                <button
                    onclick={on_logout}
                    class="rounded bg-red-600 px-4 py-2 text-sm text-white hover:bg-red-700"
                >
                    {"Sign out"}
                </button>
            </div>
        </div>
    }
}
