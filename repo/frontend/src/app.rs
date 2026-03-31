//! Root application component.
//!
//! Responsibilities:
//!   • Provide `AuthContext` to the entire tree.
//!   • On first render, attempt to verify any stored token by calling /me.
//!   • Route between the public Kiosk, Login, and Dashboard, enforcing auth
//!     guards on the staff-facing routes.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{
    api,
    auth::{AuthAction, AuthContext, AuthState, AuthStatus},
    pages::{
        KioskArchivePage, KioskArticlePage, KioskHomePage, KioskSearchPage,
        LoginPage, OpsOrdersPage, OpsSchedulesPage,
    },
};

// ── Routes ────────────────────────────────────────────────────────────────────

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    // Root redirects to the public kiosk home.
    #[at("/")]
    Home,

    // ── Public kiosk ─────────────────────────────────────────────────────
    #[at("/kiosk")]
    Kiosk,
    #[at("/kiosk/search")]
    KioskSearch,
    #[at("/kiosk/article/:slug")]
    KioskArticle { slug: String },
    #[at("/kiosk/archive")]
    KioskArchive,

    // ── Staff-facing ──────────────────────────────────────────────────────
    #[at("/login")]
    Login,
    #[at("/ops/schedules")]
    OpsSchedules,
    #[at("/ops/orders")]
    OpsOrders,

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
        // Redirect bare "/" to the public kiosk home.
        Route::Home => html! { <Redirect<Route> to={Route::Kiosk} /> },

        // ── Public kiosk ─────────────────────────────────────────────────
        Route::Kiosk                     => html! { <KioskHomePage /> },
        Route::KioskSearch               => html! { <KioskSearchPage /> },
        Route::KioskArticle { slug }     => html! { <KioskArticlePage slug={slug} /> },
        Route::KioskArchive              => html! { <KioskArchivePage /> },

        // ── Staff-facing ──────────────────────────────────────────────────
        Route::Login        => html! { <LoginPage /> },
        Route::OpsSchedules => html! { <OpsSchedulesPage /> },
        Route::OpsOrders    => html! { <OpsOrdersPage /> },

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
