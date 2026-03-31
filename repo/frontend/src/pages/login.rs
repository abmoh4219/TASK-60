//! Login page — premium split-screen design.
//!
//! Left panel: branded gradient with testimonial.
//! Right panel: clean login form with keyboard-first UX.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::{
    api,
    app::Route,
    auth::{AuthAction, AuthContext, CurrentUser},
    components::icons,
};

#[function_component(LoginPage)]
pub fn login_page() -> Html {
    let auth      = use_context::<AuthContext>().expect("AuthContext missing");
    let navigator = use_navigator().unwrap();

    let username = use_state(String::new);
    let password = use_state(String::new);
    let error    = use_state(|| Option::<String>::None);
    let loading  = use_state(|| false);

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
        let (username, password, error, loading, auth, navigator) = (
            username.clone(), password.clone(), error.clone(),
            loading.clone(), auth.clone(), navigator.clone(),
        );
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let u = (*username).trim().to_owned();
            let p = (*password).clone();

            if u.is_empty() || p.is_empty() {
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

            let (error, loading, auth, navigator) = (
                error.clone(), loading.clone(), auth.clone(), navigator.clone(),
            );
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
        <div class="min-h-screen flex">

            // ── Left branding panel ───────────────────────────────────────
            <div class="hidden lg:flex lg:w-[52%] xl:w-[55%] relative
                        bg-gradient-to-br from-slate-900 via-indigo-950 to-slate-900
                        flex-col items-start justify-between p-12 overflow-hidden">

                // Background decoration
                <div class="absolute inset-0 overflow-hidden pointer-events-none">
                    <div class="absolute -top-32 -left-32 w-[600px] h-[600px] rounded-full
                                bg-indigo-600/20 blur-[100px]"></div>
                    <div class="absolute bottom-0 right-0 w-[400px] h-[400px] rounded-full
                                bg-indigo-500/10 blur-[80px]"></div>
                    // Grid pattern
                    <svg class="absolute inset-0 w-full h-full opacity-[0.04]"
                         xmlns="http://www.w3.org/2000/svg">
                        <defs>
                            <pattern id="grid" width="40" height="40" patternUnits="userSpaceOnUse">
                                <path d="M 40 0 L 0 0 0 40" fill="none"
                                      stroke="white" stroke-width="1"/>
                            </pattern>
                        </defs>
                        <rect width="100%" height="100%" fill="url(#grid)"/>
                    </svg>
                </div>

                // Logo + brand
                <div class="relative flex items-center gap-3">
                    <div class="w-10 h-10 rounded-xl bg-indigo-600 flex items-center justify-center shadow-lg">
                        { icons::chart_bar("w-5 h-5 text-white") }
                    </div>
                    <div>
                        <span class="text-white font-bold text-xl tracking-tight">
                            {"RailOps"}
                        </span>
                        <p class="text-indigo-400 text-xs font-medium">
                            {"Operations Platform"}
                        </p>
                    </div>
                </div>

                // Hero copy
                <div class="relative max-w-md">
                    <h2 class="text-4xl font-bold text-white leading-tight mb-4">
                        {"Real-time rail operations, "}
                        <span class="text-indigo-400">{"beautifully managed."}</span>
                    </h2>
                    <p class="text-slate-400 text-base leading-relaxed">
                        {"Unified scheduling, live inventory, smart staffing, and compliant \
                          credential management — all in one secure platform."}
                    </p>
                </div>

                // Feature pills
                <div class="relative flex flex-wrap gap-2">
                    { for ["Live Scheduling", "Order Management", "Smart Matching",
                           "Document Control", "Audit Trails"].iter().map(|f| html! {
                        <span class="inline-flex items-center gap-1.5 rounded-full
                                     bg-white/10 border border-white/20 px-3 py-1
                                     text-xs font-medium text-slate-300">
                            { icons::check_circle("w-3.5 h-3.5 text-emerald-400") }
                            { *f }
                        </span>
                    })}
                </div>
            </div>

            // ── Right login panel ─────────────────────────────────────────
            <div class="flex-1 flex flex-col items-center justify-center
                        px-6 py-12 bg-white lg:px-16 xl:px-20">

                // Mobile logo
                <div class="mb-8 flex items-center gap-2 lg:hidden">
                    <div class="w-8 h-8 rounded-lg bg-indigo-600 flex items-center justify-center">
                        { icons::chart_bar("w-4 h-4 text-white") }
                    </div>
                    <span class="font-bold text-slate-900">{"RailOps"}</span>
                </div>

                <div class="w-full max-w-sm">
                    <div class="mb-8">
                        <h1 class="text-2xl font-bold text-slate-900">
                            {"Welcome back"}
                        </h1>
                        <p class="mt-1.5 text-sm text-slate-500">
                            {"Sign in to the operations console."}
                        </p>
                    </div>

                    // Error banner
                    if let Some(msg) = (*error).clone() {
                        <div class="mb-5 flex items-start gap-3 rounded-lg bg-red-50
                                    border border-red-200 px-4 py-3">
                            <span class="text-red-500 shrink-0 mt-0.5">
                                { icons::exclamation_triangle("w-4 h-4") }
                            </span>
                            <p class="text-sm text-red-700">{ msg }</p>
                        </div>
                    }

                    // Form
                    <form onsubmit={on_submit} class="space-y-5">
                        <div>
                            <label for="username"
                                   class="block text-sm font-medium text-slate-700 mb-1.5">
                                {"Username"}
                            </label>
                            <input
                                id="username"
                                type="text"
                                autocomplete="username"
                                required=true
                                autofocus=true
                                disabled={*loading}
                                placeholder="your.username"
                                value={(*username).clone()}
                                oninput={on_username}
                                class="block w-full rounded-lg border border-slate-300 bg-white \
                                       px-3.5 py-2.5 text-sm text-slate-900 placeholder-slate-400 \
                                       shadow-sm focus:border-indigo-500 focus:outline-none \
                                       focus:ring-2 focus:ring-indigo-500/20 disabled:opacity-60"
                            />
                        </div>
                        <div>
                            <label for="password"
                                   class="block text-sm font-medium text-slate-700 mb-1.5">
                                {"Password"}
                            </label>
                            <input
                                id="password"
                                type="password"
                                autocomplete="current-password"
                                required=true
                                disabled={*loading}
                                placeholder="••••••••••"
                                value={(*password).clone()}
                                oninput={on_password}
                                class="block w-full rounded-lg border border-slate-300 bg-white \
                                       px-3.5 py-2.5 text-sm text-slate-900 placeholder-slate-400 \
                                       shadow-sm focus:border-indigo-500 focus:outline-none \
                                       focus:ring-2 focus:ring-indigo-500/20 disabled:opacity-60"
                            />
                        </div>

                        <button
                            type="submit"
                            disabled={*loading}
                            class="w-full flex items-center justify-center gap-2 rounded-lg \
                                   bg-indigo-600 px-4 py-2.5 text-sm font-semibold text-white \
                                   shadow-sm hover:bg-indigo-700 active:bg-indigo-800 \
                                   focus-visible:outline focus-visible:outline-2 \
                                   focus-visible:outline-offset-2 focus-visible:outline-indigo-600 \
                                   disabled:opacity-50 transition-colors"
                        >
                            if *loading {
                                <svg class="animate-spin w-4 h-4 text-white/70"
                                     xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                                    <circle class="opacity-25" cx="12" cy="12" r="10"
                                            stroke="currentColor" stroke-width="4"/>
                                    <path class="opacity-75" fill="currentColor"
                                          d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/>
                                </svg>
                                {"Signing in…"}
                            } else {
                                { icons::arrow_right("w-4 h-4") }
                                {"Sign in"}
                            }
                        </button>
                    </form>

                    // Footer
                    <p class="mt-8 text-center text-xs text-slate-400">
                        <span class="inline-flex items-center gap-1">
                            { icons::shield_check("w-3.5 h-3.5 text-emerald-500") }
                            {"End-to-end encrypted  ·  RailOps v"}
                            { env!("CARGO_PKG_VERSION") }
                        </span>
                    </p>
                </div>
            </div>
        </div>
    }
}
