//! Login page — shown to unauthenticated visitors.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{
    api,
    auth::{AuthAction, AuthContext, CurrentUser},
    app::Route,
};

#[function_component(LoginPage)]
pub fn login_page() -> Html {
    let auth      = use_context::<AuthContext>().expect("AuthContext missing");
    let navigator = use_navigator().unwrap();

    let username = use_state(String::new);
    let password = use_state(String::new);
    let error    = use_state(|| Option::<String>::None);
    let loading  = use_state(|| false);

    // Redirect if already authenticated.
    if auth.is_authenticated() {
        navigator.push(&Route::OpsSchedules);
        return html! {};
    }

    let on_username = {
        let username = username.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            username.set(input.value());
        })
    };

    let on_password = {
        let password = password.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            password.set(input.value());
        })
    };

    let on_submit = {
        let username  = username.clone();
        let password  = password.clone();
        let error     = error.clone();
        let loading   = loading.clone();
        let auth      = auth.clone();
        let navigator = navigator.clone();

        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();

            let u = (*username).clone();
            let p = (*password).clone();

            // Client-side validation
            if u.trim().is_empty() || p.is_empty() {
                error.set(Some("Username and password are required.".into()));
                return;
            }
            if p.len() < shared::rules::MIN_PASSWORD_LEN {
                error.set(Some(format!(
                    "Password must be at least {} characters.",
                    shared::rules::MIN_PASSWORD_LEN
                )));
                return;
            }

            error.set(None);
            loading.set(true);

            let error     = error.clone();
            let loading   = loading.clone();
            let auth      = auth.clone();
            let navigator = navigator.clone();

            spawn_local(async move {
                match api::login(&u, &p).await {
                    Ok(resp) => {
                        let user = CurrentUser {
                            id:        resp.user.id,
                            username:  resp.user.username,
                            role:      resp.user.role,
                            full_name: resp.user.full_name,
                        };
                        auth.dispatch(AuthAction::SetUser { token: resp.token, user });
                        navigator.push(&Route::OpsSchedules);
                    }
                    Err(e) => {
                        error.set(Some(e.message));
                        loading.set(false);
                    }
                }
            });
        })
    };

    html! {
        <div class="min-h-screen flex items-center justify-center bg-gray-50">
            <div class="w-full max-w-md space-y-8 p-8 bg-white rounded-xl shadow-lg">

                // ── Logo / title ──────────────────────────────────────────
                <div class="text-center">
                    <h1 class="text-3xl font-bold text-gray-900">{"RailOps"}</h1>
                    <p class="mt-2 text-sm text-gray-500">{"Operations & Booking System"}</p>
                </div>

                // ── Error banner ──────────────────────────────────────────
                if let Some(msg) = (*error).clone() {
                    <div class="rounded-md bg-red-50 border border-red-300 p-3 text-sm text-red-700">
                        { msg }
                    </div>
                }

                // ── Form ──────────────────────────────────────────────────
                <form onsubmit={on_submit} class="space-y-5">
                    <div>
                        <label class="block text-sm font-medium text-gray-700">{"Username"}</label>
                        <input
                            type="text"
                            autocomplete="username"
                            required=true
                            disabled={*loading}
                            value={(*username).clone()}
                            oninput={on_username}
                            class="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2
                                   text-sm shadow-sm focus:border-blue-500 focus:outline-none"
                        />
                    </div>
                    <div>
                        <label class="block text-sm font-medium text-gray-700">{"Password"}</label>
                        <input
                            type="password"
                            autocomplete="current-password"
                            required=true
                            disabled={*loading}
                            value={(*password).clone()}
                            oninput={on_password}
                            class="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2
                                   text-sm shadow-sm focus:border-blue-500 focus:outline-none"
                        />
                    </div>
                    <button
                        type="submit"
                        disabled={*loading}
                        class="w-full rounded-md bg-blue-600 px-4 py-2 text-sm font-semibold
                               text-white hover:bg-blue-700 disabled:opacity-50"
                    >
                        if *loading { {"Signing in…"} } else { {"Sign in"} }
                    </button>
                </form>

                <p class="text-center text-xs text-gray-400">
                    {"Secure connection  •  RailOps v"}
                    { env!("CARGO_PKG_VERSION") }
                </p>
            </div>
        </div>
    }
}
