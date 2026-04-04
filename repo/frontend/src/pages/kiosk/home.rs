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
use crate::components::icons;

// ── Category metadata ─────────────────────────────────────────────────────────

struct CategoryCard {
    key:   &'static str,
    label: &'static str,
    icon:  fn(&str) -> Html,
    bg:    &'static str,
    text:  &'static str,
    ring:  &'static str,
}

const CATEGORIES: &[CategoryCard] = &[
    CategoryCard {
        key: "delays", label: "Service Delays",
        icon: icons::exclamation_triangle,
        bg: "bg-red-50", text: "text-red-900", ring: "border-red-200",
    },
    CategoryCard {
        key: "fares", label: "Fares & Tickets",
        icon: icons::ticket,
        bg: "bg-emerald-50", text: "text-emerald-900", ring: "border-emerald-200",
    },
    CategoryCard {
        key: "baggage", label: "Baggage",
        icon: icons::archive_box,
        bg: "bg-amber-50", text: "text-amber-900", ring: "border-amber-200",
    },
    CategoryCard {
        key: "accessibility", label: "Accessibility",
        icon: icons::user_group,
        bg: "bg-violet-50", text: "text-violet-900", ring: "border-violet-200",
    },
    CategoryCard {
        key: "general", label: "General Info",
        icon: icons::information_circle,
        bg: "bg-sky-50", text: "text-sky-900", ring: "border-sky-200",
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
                    kiosk::search_content(None, None, None, None, None, 1, 6),
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
            <div class="bg-gradient-to-br from-indigo-950 via-indigo-900 to-indigo-800 text-white">
                <div class="max-w-4xl mx-auto px-6 py-20 text-center">
                    <div class="inline-flex items-center gap-2 bg-white/10 rounded-full
                                px-4 py-1.5 text-indigo-200 text-sm font-medium mb-6">
                        { icons::information_circle("w-4 h-4") }
                        {"Passenger Information Portal"}
                    </div>
                    <h1 class="text-4xl sm:text-5xl font-bold mb-4 tracking-tight">
                        {"RailOps Travel Guide"}
                    </h1>
                    <p class="text-indigo-200 text-lg mb-10 max-w-xl mx-auto">
                        {"Find travel advisories, fare details, station services and more."}
                    </p>

                    // Search form
                    <form onsubmit={on_search}
                          class="flex max-w-2xl mx-auto shadow-2xl shadow-indigo-950/50 rounded-2xl overflow-hidden">
                        <div class="relative flex-1">
                            <span class="absolute left-4 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none">
                                { icons::magnifying_glass("w-5 h-5") }
                            </span>
                            <input
                                type="search"
                                placeholder="Search articles, topics, routes…"
                                autocomplete="off"
                                value={(*search_val).clone()}
                                oninput={on_input}
                                class="w-full border-0 px-5 py-4 pl-12 text-base text-slate-900
                                       placeholder-slate-400 focus:outline-none focus:ring-0"
                            />
                        </div>
                        <button
                            type="submit"
                            class="bg-indigo-500 hover:bg-indigo-400 active:bg-indigo-600
                                   px-7 py-4 text-base font-semibold text-white transition-colors"
                        >
                            {"Search"}
                        </button>
                    </form>
                </div>
            </div>

            // ── Category cards ─────────────────────────────────────────────
            <div class="max-w-5xl mx-auto px-6 py-14">
                <div class="flex items-center justify-between mb-7">
                    <h2 class="text-xl font-bold text-slate-900">{"Browse by Topic"}</h2>
                    <Link<Route> to={Route::KioskSearch}
                        classes="text-sm font-medium text-indigo-600 hover:text-indigo-800 transition">
                        {"All articles →"}
                    </Link<Route>>
                </div>
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
                                    "group flex flex-col items-center justify-center gap-3 p-5 \
                                     rounded-2xl border-2 {} {} {} min-h-[120px] text-center \
                                     hover:shadow-md hover:scale-[1.02] active:scale-[0.98] \
                                     transition-all cursor-pointer",
                                    cat.bg, cat.ring, cat.text
                                )}
                            >
                                <span class="opacity-75 group-hover:opacity-100 transition-opacity">
                                    { (cat.icon)("w-8 h-8") }
                                </span>
                                <span class="text-sm font-semibold leading-tight">{ cat.label }</span>
                                if count > 0 {
                                    <span class="text-xs opacity-50">{ format!("{count} articles") }</span>
                                }
                            </button>
                        }
                    }) }
                </div>
            </div>

            // ── Latest articles ────────────────────────────────────────────
            <div class="max-w-5xl mx-auto px-6 pb-20">
                <div class="flex items-center justify-between mb-7">
                    <h2 class="text-xl font-bold text-slate-900">{"Latest Updates"}</h2>
                    <Link<Route> to={Route::KioskSearch}
                        classes="text-sm font-medium text-indigo-600 hover:text-indigo-800 transition">
                        {"View all →"}
                    </Link<Route>>
                </div>

                if *loading {
                    { skeleton_cards(6) }
                } else if latest.is_empty() {
                    <p class="text-slate-500 text-center py-12">
                        {"No articles available yet."}
                    </p>
                } else {
                    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
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
        <nav class="bg-white border-b border-slate-200 sticky top-0 z-50 shadow-sm">
            <div class="max-w-5xl mx-auto px-6 h-16 flex items-center justify-between">
                <Link<Route> to={Route::Kiosk}
                    classes="flex items-center gap-2">
                    <span class="w-7 h-7 bg-indigo-700 rounded-lg flex items-center justify-center">
                        { icons::arrow_right("w-4 h-4 text-white") }
                    </span>
                    <span class="text-lg font-bold text-indigo-900 tracking-tight">{"RailOps"}</span>
                </Link<Route>>
                <div class="flex items-center gap-6 text-sm font-medium text-slate-600">
                    <Link<Route> to={Route::KioskSearch}
                        classes="hover:text-indigo-700 transition-colors">
                        {"All Articles"}
                    </Link<Route>>
                    <Link<Route> to={Route::KioskArchive}
                        classes="hover:text-indigo-700 transition-colors">
                        {"Archive"}
                    </Link<Route>>
                    <Link<Route> to={Route::Login}
                        classes="inline-flex items-center gap-1.5 rounded-lg bg-indigo-700 px-4 py-2 \
                                 text-white hover:bg-indigo-800 transition-colors text-xs font-semibold">
                        { icons::arrow_right_on_rect("w-3.5 h-3.5") }
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
            classes="block bg-white rounded-2xl border border-slate-200 p-5 \
                     hover:shadow-md hover:border-indigo-200 active:scale-[0.99] \
                     transition-all group"
        >
            <div class={format!("inline-block text-xs font-semibold px-2.5 py-0.5 rounded-full mb-3 {color}")}>
                { label }
            </div>
            <h3 class="text-slate-900 font-semibold text-base leading-snug line-clamp-2
                       group-hover:text-indigo-700 transition-colors mb-3">
                { title }
            </h3>
            if !date.is_empty() {
                <div class="flex items-center gap-1 text-xs text-slate-400">
                    { icons::calendar("w-3 h-3") }
                    <span>{ date }</span>
                </div>
            }
        </Link<Route>>
    }
}

fn skeleton_cards(n: usize) -> Html {
    html! {
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
            { for (0..n).map(|_| html! {
                <div class="bg-white rounded-2xl border border-slate-200 p-5 animate-pulse">
                    <div class="h-4 bg-slate-200 rounded-full w-20 mb-3"></div>
                    <div class="h-5 bg-slate-200 rounded w-full mb-2"></div>
                    <div class="h-5 bg-slate-200 rounded w-3/4 mb-4"></div>
                    <div class="h-3 bg-slate-100 rounded w-16"></div>
                </div>
            }) }
        </div>
    }
}

pub fn footer() -> Html {
    html! {
        <footer class="border-t border-slate-200 bg-white py-6">
            <div class="max-w-5xl mx-auto px-6 flex items-center justify-between">
                <span class="text-xs text-slate-400">
                    { "© RailOps Passenger Information System" }
                </span>
                <span class="text-xs text-slate-300">
                    { env!("CARGO_PKG_VERSION") }
                </span>
            </div>
        </footer>
    }
}
