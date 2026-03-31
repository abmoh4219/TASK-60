//! Kiosk home page.
//!
//! Features:
//! - Hero search bar (navigates to /kiosk/search on submit)
//! - Category cards — click to filter all articles by category
//! - Latest articles strip (3 most recent)
//! - Live category counts loaded from the API

use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::kiosk::{self, CategoryCount, ContentSummary};
use crate::app::Route;

// ── Category metadata ─────────────────────────────────────────────────────────

struct CategoryCard {
    key:   &'static str,
    label: &'static str,
    icon:  &'static str,   // emoji
    bg:    &'static str,   // Tailwind bg class
    text:  &'static str,   // Tailwind text class
    ring:  &'static str,   // Tailwind ring/border class
}

const CATEGORIES: &[CategoryCard] = &[
    CategoryCard {
        key: "delays", label: "Service Delays", icon: "🚦",
        bg: "bg-red-50", text: "text-red-900", ring: "border-red-200",
    },
    CategoryCard {
        key: "fares", label: "Fares & Tickets", icon: "🎫",
        bg: "bg-green-50", text: "text-green-900", ring: "border-green-200",
    },
    CategoryCard {
        key: "baggage", label: "Baggage", icon: "🧳",
        bg: "bg-amber-50", text: "text-amber-900", ring: "border-amber-200",
    },
    CategoryCard {
        key: "accessibility", label: "Accessibility", icon: "♿",
        bg: "bg-purple-50", text: "text-purple-900", ring: "border-purple-200",
    },
    CategoryCard {
        key: "general", label: "General Info", icon: "ℹ️",
        bg: "bg-blue-50", text: "text-blue-900", ring: "border-blue-200",
    },
];

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component(KioskHomePage)]
pub fn kiosk_home() -> Html {
    let navigator  = use_navigator().unwrap();
    let search_val = use_state(String::new);
    let latest     = use_state(|| Vec::<ContentSummary>::new());
    let cat_counts = use_state(|| Vec::<CategoryCount>::new());
    let loading    = use_state(|| true);

    // Load latest articles + category counts on mount.
    {
        let latest     = latest.clone();
        let cat_counts = cat_counts.clone();
        let loading    = loading.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let (lat, cats) = futures::join!(
                    kiosk::search_content(None, None, None, 1, 6),
                    kiosk::list_categories(),
                );
                if let Ok(r) = lat  { latest.set(r.items); }
                if let Ok(c) = cats { cat_counts.set(c); }
                loading.set(false);
            });
            || ()
        });
    }

    // ── Search submission ─────────────────────────────────────────────────
    let on_search = {
        let search_val = search_val.clone();
        let navigator  = navigator.clone();
        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();
            let q = (*search_val).trim().to_owned();
            if !q.is_empty() {
                navigator.push_with_query(
                    &Route::KioskSearch,
                    &[("q", q.as_str())],
                ).ok();
            }
        })
    };

    let on_input = {
        let search_val = search_val.clone();
        Callback::from(move |e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            search_val.set(input.value());
        })
    };

    html! {
        <div class="min-h-screen bg-slate-50">
            // ── Navigation ─────────────────────────────────────────────────
            { nav_bar() }

            // ── Hero ───────────────────────────────────────────────────────
            <div class="bg-blue-800 text-white">
                <div class="max-w-4xl mx-auto px-6 py-16 text-center">
                    <h1 class="text-4xl font-bold mb-3 tracking-tight">
                        {"RailOps Passenger Information"}
                    </h1>
                    <p class="text-blue-200 text-lg mb-10">
                        {"Search for travel advisories, fare information, and station services."}
                    </p>

                    // Search form
                    <form onsubmit={on_search} class="flex max-w-2xl mx-auto">
                        <input
                            type="search"
                            placeholder="Search articles…"
                            autocomplete="off"
                            value={(*search_val).clone()}
                            oninput={on_input}
                            class="flex-1 rounded-l-xl border-0 px-5 py-4 text-lg text-gray-900
                                   placeholder-gray-400 focus:outline-none focus:ring-4
                                   focus:ring-blue-300 shadow-lg"
                        />
                        <button
                            type="submit"
                            class="rounded-r-xl bg-yellow-400 px-7 py-4 text-lg font-bold
                                   text-blue-900 hover:bg-yellow-300 active:bg-yellow-500
                                   shadow-lg transition"
                        >
                            {"Search"}
                        </button>
                    </form>
                </div>
            </div>

            // ── Category cards ─────────────────────────────────────────────
            <div class="max-w-5xl mx-auto px-6 py-12">
                <h2 class="text-2xl font-semibold text-gray-800 mb-6">{"Browse by Topic"}</h2>
                <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-4">
                    { for CATEGORIES.iter().map(|cat| {
                        let count = (*cat_counts).iter()
                            .find(|c| c.category == cat.key)
                            .map(|c| c.count)
                            .unwrap_or(0);
                        let nav = navigator.clone();
                        let key = cat.key;
                        let onclick = Callback::from(move |_: MouseEvent| {
                            nav.push_with_query(
                                &Route::KioskSearch,
                                &[("category", key)],
                            ).ok();
                        });
                        html! {
                            <button
                                key={cat.key}
                                onclick={onclick}
                                class={format!(
                                    "flex flex-col items-center justify-center gap-2 p-5 rounded-2xl \
                                     border-2 {} {} {} \
                                     hover:shadow-md active:scale-95 transition cursor-pointer \
                                     min-h-[120px] text-center",
                                    cat.bg, cat.ring, cat.text
                                )}
                            >
                                <span class="text-4xl">{ cat.icon }</span>
                                <span class="text-sm font-semibold leading-tight">{ cat.label }</span>
                                if count > 0 {
                                    <span class="text-xs opacity-60">{ format!("{count} articles") }</span>
                                }
                            </button>
                        }
                    }) }
                </div>
            </div>

            // ── Latest articles ────────────────────────────────────────────
            <div class="max-w-5xl mx-auto px-6 pb-16">
                <div class="flex items-center justify-between mb-6">
                    <h2 class="text-2xl font-semibold text-gray-800">{"Latest Updates"}</h2>
                    <Link<Route> to={Route::KioskSearch}
                        classes="text-blue-700 hover:underline text-sm font-medium">
                        {"View all articles →"}
                    </Link<Route>>
                </div>

                if *loading {
                    { skeleton_cards(6) }
                } else if latest.is_empty() {
                    <p class="text-gray-500 text-center py-12">
                        {"No articles available yet."}
                    </p>
                } else {
                    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
                        { for (*latest).iter().map(|a| article_card(a)) }
                    </div>
                }
            </div>

            // ── Footer ─────────────────────────────────────────────────────
            { footer() }
        </div>
    }
}

// ── Shared UI pieces ──────────────────────────────────────────────────────────

pub fn nav_bar() -> Html {
    html! {
        <nav class="bg-white border-b border-gray-200 sticky top-0 z-50 shadow-sm">
            <div class="max-w-5xl mx-auto px-6 h-16 flex items-center justify-between">
                <Link<Route> to={Route::Kiosk}
                    classes="text-xl font-bold text-blue-800 tracking-tight">
                    {"🚆 RailOps"}
                </Link<Route>>
                <div class="flex items-center gap-6 text-sm font-medium text-gray-600">
                    <Link<Route> to={Route::KioskSearch}
                        classes="hover:text-blue-700 transition">
                        {"All Articles"}
                    </Link<Route>>
                    <Link<Route> to={Route::KioskArchive}
                        classes="hover:text-blue-700 transition">
                        {"Archive"}
                    </Link<Route>>
                    <Link<Route> to={Route::Login}
                        classes="ml-2 rounded-lg bg-blue-700 px-4 py-2 text-white \
                                 hover:bg-blue-800 transition text-xs">
                        {"Staff Login"}
                    </Link<Route>>
                </div>
            </div>
        </nav>
    }
}

pub fn article_card(article: &ContentSummary) -> Html {
    let slug   = article.slug.clone();
    let date   = article.display_date();
    let color  = article.category_color().to_owned();
    let label  = article.category_label().to_owned();
    let title  = article.title.clone();

    html! {
        <Link<Route>
            to={Route::KioskArticle { slug }}
            classes="block bg-white rounded-2xl border border-gray-200 p-5 hover:shadow-md \
                     active:scale-[0.99] transition group"
        >
            <div class={format!("inline-block text-xs font-semibold px-2.5 py-0.5 rounded-full mb-3 {color}")}>
                { label }
            </div>
            <h3 class="text-gray-900 font-semibold text-base leading-snug line-clamp-2
                       group-hover:text-blue-700 transition mb-2">
                { title }
            </h3>
            if !date.is_empty() {
                <p class="text-xs text-gray-400">{ date }</p>
            }
        </Link<Route>>
    }
}

fn skeleton_cards(n: usize) -> Html {
    html! {
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            { for (0..n).map(|_| html! {
                <div class="bg-white rounded-2xl border border-gray-200 p-5 animate-pulse">
                    <div class="h-4 bg-gray-200 rounded w-20 mb-3"></div>
                    <div class="h-5 bg-gray-200 rounded w-full mb-2"></div>
                    <div class="h-5 bg-gray-200 rounded w-3/4 mb-4"></div>
                    <div class="h-3 bg-gray-100 rounded w-16"></div>
                </div>
            }) }
        </div>
    }
}

pub fn footer() -> Html {
    html! {
        <footer class="border-t border-gray-200 bg-white py-6 text-center text-xs text-gray-400">
            { "RailOps Passenger Information System  •  " }
            { env!("CARGO_PKG_VERSION") }
        </footer>
    }
}
