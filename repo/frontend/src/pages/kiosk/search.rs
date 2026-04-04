//! Kiosk search page.
//!
//! Features:
//! - URL-driven state: all filters are query parameters so results are bookmarkable
//! - 300 ms debounced text input
//! - Multi-condition filters: full-text query + category + tag
//! - FTS results with fuzzy fallback (shown with different heading)
//! - Pagination with first/prev/next/last controls
//! - Active filter chips with individual remove buttons
//! - Tag suggestions sidebar loaded from the API

use gloo_timers::callback::Timeout;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::kiosk::{self, category_label, CategoryCount, SearchResponse, TagCount};
use crate::app::Route;
use crate::pages::kiosk::home::{article_card, footer, nav_bar};

// ── URL query params ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, Default)]
pub struct SearchQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub q:              Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category:       Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag:            Option<String>,
    /// City filter (matched against content city references).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city:           Option<String>,
    /// ISO-8601 departure lower bound, e.g. `"2026-06-01"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub departure_from: Option<String>,
    /// ISO-8601 departure upper bound, e.g. `"2026-06-30"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub departure_to:   Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page:           Option<i64>,
}

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component(KioskSearchPage)]
pub fn kiosk_search() -> Html {
    let location  = use_location().unwrap();
    let navigator = use_navigator().unwrap();

    // Parse URL query params.
    let url_q: SearchQuery = location.query::<SearchQuery>().unwrap_or_default();

    // Raw input state (what the user is currently typing — debounced).
    let raw_input = use_state(|| url_q.q.clone().unwrap_or_default());

    // Committed filter state (what was last submitted to the API).
    let committed = use_state(|| url_q.clone());

    // Results
    let results     = use_state(|| Option::<SearchResponse>::None);
    let loading     = use_state(|| false);
    let error       = use_state(|| Option::<String>::None);

    // Sidebar data
    let cat_counts = use_state(|| Vec::<CategoryCount>::new());
    let top_tags   = use_state(|| Vec::<TagCount>::new());

    // ── Load sidebar data once ────────────────────────────────────────────
    {
        let cat_counts = cat_counts.clone();
        let top_tags   = top_tags.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let (cats, tags) = futures::join!(
                    kiosk::list_categories(),
                    kiosk::list_tags(),
                );
                if let Ok(c) = cats { cat_counts.set(c); }
                if let Ok(t) = tags { top_tags.set(t); }
            });
            || ()
        });
    }

    // ── Sync raw_input when URL changes externally (e.g. back/forward) ───
    {
        let raw_input = raw_input.clone();
        let committed = committed.clone();
        let q_str     = url_q.q.clone().unwrap_or_default();
        use_effect_with(url_q.clone(), move |uq| {
            raw_input.set(uq.q.clone().unwrap_or_default());
            committed.set(uq.clone());
            drop(q_str);
            || ()
        });
    }

    // ── Run search whenever committed filter changes ───────────────────────
    {
        let results  = results.clone();
        let loading  = loading.clone();
        let error    = error.clone();
        use_effect_with((*committed).clone(), move |params| {
            let params  = params.clone();
            let results = results.clone();
            let loading = loading.clone();
            let error   = error.clone();

            loading.set(true);
            error.set(None);

            spawn_local(async move {
                let q              = params.q.as_deref();
                let category       = params.category.as_deref();
                let tag            = params.tag.as_deref();
                let city           = params.city.as_deref();
                let departure_from = params.departure_from.as_deref();
                let departure_to   = params.departure_to.as_deref();
                let page           = params.page.unwrap_or(1);

                match kiosk::search_content(q, category, tag, city, departure_from, departure_to, page, 12).await {
                    Ok(r)  => { results.set(Some(r)); loading.set(false); }
                    Err(e) => {
                        error.set(Some(e.message));
                        results.set(None);
                        loading.set(false);
                    }
                }
            });
            || ()
        });
    }

    // ── Debounce text input → push URL ────────────────────────────────────
    {
        let navigator = navigator.clone();
        let url_q2    = url_q.clone();
        let raw_input = raw_input.clone();
        use_effect_with((*raw_input).clone(), move |text| {
            let text  = text.clone();
            let nav   = navigator.clone();
            let mut q = url_q2.clone();
            let handle = Timeout::new(300, move || {
                q.q    = if text.trim().is_empty() { None } else { Some(text.trim().to_owned()) };
                q.page = None;
                nav.push_with_query(&Route::KioskSearch, &q).ok();
            });
            move || drop(handle)
        });
    }

    // ── City filter handler ────────────────────────────────────────────────
    let on_city_change = {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            let val = el.value();
            let mut qq = q.clone();
            qq.city = if val.is_empty() { None } else { Some(val) };
            qq.page = None;
            navigator.push_with_query(&Route::KioskSearch, &qq).ok();
        })
    };

    // ── Departure filter handlers ─────────────────────────────────────────
    let on_departure_from = {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            let val = el.value();
            let mut qq = q.clone();
            qq.departure_from = if val.is_empty() { None } else { Some(val) };
            qq.page = None;
            navigator.push_with_query(&Route::KioskSearch, &qq).ok();
        })
    };

    let on_departure_to = {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            let val = el.value();
            let mut qq = q.clone();
            qq.departure_to = if val.is_empty() { None } else { Some(val) };
            qq.page = None;
            navigator.push_with_query(&Route::KioskSearch, &qq).ok();
        })
    };

    // ── Event handlers ────────────────────────────────────────────────────
    let on_input = {
        let raw_input = raw_input.clone();
        Callback::from(move |e: InputEvent| {
            let el: HtmlInputElement = e.target_unchecked_into();
            raw_input.set(el.value());
        })
    };

    let on_clear_query = {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |_: MouseEvent| {
            let mut q = q.clone();
            q.q    = None;
            q.page = None;
            navigator.push_with_query(&Route::KioskSearch, &q).ok();
        })
    };

    let make_category_cb = |cat: Option<String>| {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |_: MouseEvent| {
            let mut q = q.clone();
            q.category = cat.clone();
            q.page     = None;
            navigator.push_with_query(&Route::KioskSearch, &q).ok();
        })
    };

    let make_tag_cb = |tag: Option<String>| {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |_: MouseEvent| {
            let mut q = q.clone();
            q.tag  = tag.clone();
            q.page = None;
            navigator.push_with_query(&Route::KioskSearch, &q).ok();
        })
    };

    let make_page_cb = |p: i64| {
        let navigator = navigator.clone();
        let q = url_q.clone();
        Callback::from(move |_: MouseEvent| {
            let mut q = q.clone();
            q.page = Some(p);
            navigator.push_with_query(&Route::KioskSearch, &q).ok();
        })
    };

    // ── Render ────────────────────────────────────────────────────────────
    let active_q           = url_q.q.as_deref().unwrap_or("").to_owned();
    let active_category    = url_q.category.clone();
    let active_tag         = url_q.tag.clone();
    let active_city        = url_q.city.clone().unwrap_or_default();
    let active_dep_from    = url_q.departure_from.clone().unwrap_or_default();
    let active_dep_to      = url_q.departure_to.clone().unwrap_or_default();

    html! {
        <div class="min-h-screen bg-slate-50">
            { nav_bar() }

            <div class="max-w-6xl mx-auto px-6 py-8">

                // ── Search bar ────────────────────────────────────────────
                <div class="flex gap-3 mb-6">
                    <div class="relative flex-1">
                        <input
                            type="search"
                            placeholder="Search articles…"
                            autocomplete="off"
                            value={(*raw_input).clone()}
                            oninput={on_input}
                            class="w-full rounded-xl border border-slate-300 px-5 py-3 pr-10
                                   text-base shadow-sm focus:border-indigo-500 focus:outline-none
                                   focus:ring-2 focus:ring-indigo-200"
                        />
                        if !active_q.is_empty() {
                            <button
                                onclick={on_clear_query}
                                class="absolute right-3 top-1/2 -translate-y-1/2 text-slate-400
                                       hover:text-slate-700 text-xl leading-none"
                                title="Clear search"
                            >{"×"}</button>
                        }
                    </div>
                    <Link<Route> to={Route::Kiosk}
                        classes="rounded-xl border border-slate-300 px-4 py-3 text-sm font-medium \
                                 text-slate-600 hover:bg-slate-50 transition">
                        {"← Home"}
                    </Link<Route>>
                </div>

                // ── City + Departure window filter ───────────────────────────
                <div class="flex flex-wrap gap-3 mb-4">
                    <div class="flex items-center gap-2">
                        <label class="text-xs text-slate-500 font-medium whitespace-nowrap">
                            {"City"}
                        </label>
                        <input
                            type="text"
                            placeholder="e.g. Denver"
                            value={active_city.clone()}
                            oninput={on_city_change}
                            class="rounded-lg border border-slate-300 px-3 py-2 text-sm w-32 \
                                   focus:border-indigo-500 focus:outline-none focus:ring-2 \
                                   focus:ring-indigo-200"
                        />
                    </div>
                    <div class="flex items-center gap-2">
                        <label class="text-xs text-slate-500 font-medium whitespace-nowrap">
                            {"Departs from"}
                        </label>
                        <input
                            type="date"
                            value={active_dep_from.clone()}
                            oninput={on_departure_from}
                            class="rounded-lg border border-slate-300 px-3 py-2 text-sm \
                                   focus:border-indigo-500 focus:outline-none focus:ring-2 \
                                   focus:ring-indigo-200"
                        />
                    </div>
                    <div class="flex items-center gap-2">
                        <label class="text-xs text-slate-500 font-medium whitespace-nowrap">
                            {"to"}
                        </label>
                        <input
                            type="date"
                            value={active_dep_to.clone()}
                            oninput={on_departure_to}
                            class="rounded-lg border border-slate-300 px-3 py-2 text-sm \
                                   focus:border-indigo-500 focus:outline-none focus:ring-2 \
                                   focus:ring-indigo-200"
                        />
                    </div>
                </div>

                // ── Active filter chips ───────────────────────────────────
                { active_chips(&active_category, &active_tag, &url_q.city, &url_q, navigator.clone()) }

                <div class="flex gap-8">
                    // ── Sidebar ───────────────────────────────────────────
                    <aside class="w-56 shrink-0 hidden lg:block">
                        // Category filter
                        <div class="bg-white rounded-2xl border border-slate-200 p-4 mb-4">
                            <h3 class="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-3">
                                {"Category"}
                            </h3>
                            <ul class="space-y-1">
                                <li>
                                    <button
                                        onclick={make_category_cb(None)}
                                        class={sidebar_btn_class(active_category.is_none())}
                                    >
                                        {"All topics"}
                                    </button>
                                </li>
                                { for (*cat_counts).iter().map(|cc| {
                                    let active = active_category.as_deref() == Some(&cc.category);
                                    let cat    = cc.category.clone();
                                    let cnt    = cc.count;
                                    html! {
                                        <li key={cat.clone()}>
                                            <button
                                                onclick={make_category_cb(Some(cat))}
                                                class={sidebar_btn_class(active)}
                                            >
                                                <span>{ category_label(&cc.category) }</span>
                                                <span class="ml-auto text-xs text-slate-400">{ cnt }</span>
                                            </button>
                                        </li>
                                    }
                                }) }
                            </ul>
                        </div>

                        // Top tags
                        if !(*top_tags).is_empty() {
                            <div class="bg-white rounded-2xl border border-slate-200 p-4">
                                <h3 class="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-3">
                                    {"Popular Tags"}
                                </h3>
                                <div class="flex flex-wrap gap-2">
                                    { for (*top_tags).iter().take(20).map(|tc| {
                                        let active = active_tag.as_deref() == Some(&tc.tag);
                                        let tag    = tc.tag.clone();
                                        html! {
                                            <button
                                                key={tag.clone()}
                                                onclick={make_tag_cb(if active { None } else { Some(tag) })}
                                                class={if active {
                                                    "text-xs rounded-full px-2.5 py-0.5 bg-indigo-600 \
                                                     text-white font-medium"
                                                } else {
                                                    "text-xs rounded-full px-2.5 py-0.5 bg-slate-100 \
                                                     text-slate-700 hover:bg-indigo-100 hover:text-indigo-700"
                                                }}
                                            >
                                                { &tc.tag }
                                            </button>
                                        }
                                    }) }
                                </div>
                            </div>
                        }
                    </aside>

                    // ── Results area ──────────────────────────────────────
                    <div class="flex-1 min-w-0">
                        { results_panel(&results, &loading, &error, &active_q, &make_page_cb) }
                    </div>
                </div>
            </div>

            { footer() }
        </div>
    }
}

// ── Helper components / functions ─────────────────────────────────────────────

fn active_chips(
    category: &Option<String>,
    tag:      &Option<String>,
    city:     &Option<String>,
    url_q:    &SearchQuery,
    nav:      Navigator,
) -> Html {
    if category.is_none() && tag.is_none() && city.is_none() {
        return html! {};
    }
    html! {
        <div class="flex flex-wrap gap-2 mb-4">
            if let Some(cat) = category {
                { {
                    let mut q = url_q.clone();
                    let nav2  = nav.clone();
                    let c2    = cat.clone();
                    let onclick = Callback::from(move |_: MouseEvent| {
                        let mut qq = q.clone();
                        qq.category = None;
                        qq.page     = None;
                        nav2.push_with_query(&Route::KioskSearch, &qq).ok();
                    });
                    html! {
                        <span class="inline-flex items-center gap-1 rounded-full bg-indigo-100
                                     text-indigo-800 text-sm px-3 py-1 font-medium">
                            { format!("Category: {}", category_label(&c2)) }
                            <button onclick={onclick} class="ml-1 text-indigo-600 hover:text-indigo-900">
                                {"×"}
                            </button>
                        </span>
                    }
                } }
            }
            if let Some(t) = tag {
                { {
                    let mut q = url_q.clone();
                    let nav2  = nav.clone();
                    let t2    = t.clone();
                    let onclick = Callback::from(move |_: MouseEvent| {
                        let mut qq = q.clone();
                        qq.tag  = None;
                        qq.page = None;
                        nav2.push_with_query(&Route::KioskSearch, &qq).ok();
                    });
                    html! {
                        <span class="inline-flex items-center gap-1 rounded-full bg-slate-100
                                     text-slate-700 text-sm px-3 py-1 font-medium">
                            { format!("Tag: #{}", t2) }
                            <button onclick={onclick} class="ml-1 hover:text-slate-900">
                                {"×"}
                            </button>
                        </span>
                    }
                } }
            }
            if let Some(ci) = city {
                { {
                    let mut q = url_q.clone();
                    let nav2  = nav.clone();
                    let ci2   = ci.clone();
                    let onclick = Callback::from(move |_: MouseEvent| {
                        let mut qq = q.clone();
                        qq.city = None;
                        qq.page = None;
                        nav2.push_with_query(&Route::KioskSearch, &qq).ok();
                    });
                    html! {
                        <span class="inline-flex items-center gap-1 rounded-full bg-sky-100
                                     text-sky-800 text-sm px-3 py-1 font-medium">
                            { format!("City: {}", ci2) }
                            <button onclick={onclick} class="ml-1 text-sky-600 hover:text-sky-900">
                                {"×"}
                            </button>
                        </span>
                    }
                } }
            }
        </div>
    }
}

fn results_panel(
    results:      &UseStateHandle<Option<SearchResponse>>,
    loading:      &UseStateHandle<bool>,
    error:        &UseStateHandle<Option<String>>,
    active_q:     &str,
    make_page_cb: &dyn Fn(i64) -> Callback<MouseEvent>,
) -> Html {
    if **loading {
        return html! {
            <div class="space-y-4">
                { for (0..6).map(|_| html! {
                    <div class="bg-white rounded-2xl border border-slate-200 p-5 animate-pulse">
                        <div class="h-4 bg-slate-200 rounded-full w-20 mb-3"></div>
                        <div class="h-5 bg-slate-200 rounded w-4/5 mb-2"></div>
                        <div class="h-3 bg-slate-100 rounded w-16"></div>
                    </div>
                }) }
            </div>
        };
    }

    if let Some(msg) = &**error {
        return html! {
            <div class="rounded-xl bg-red-50 border border-red-200 p-6 text-red-700">
                <p class="font-semibold mb-1">{"Unable to load results"}</p>
                <p class="text-sm">{ msg.clone() }</p>
            </div>
        };
    }

    if let Some(resp) = &**results {
        let heading = match resp.search_type.as_deref() {
            Some("fts")   => format!("{} results for \"{}\"", resp.total, active_q),
            Some("fuzzy") => format!(
                "Showing approximate matches for \"{}\" ({} results)",
                active_q, resp.total
            ),
            _ if !active_q.is_empty() => format!("No results for \"{}\"", active_q),
            _ => format!("{} articles", resp.total),
        };

        if resp.items.is_empty() {
            return html! {
                <div class="text-center py-16">
                    <p class="text-lg font-semibold text-slate-700 mb-2">
                        { format!("No results for \"{active_q}\"") }
                    </p>
                    <p class="text-slate-500 text-sm">
                        {"Try different keywords, or browse by category."}
                    </p>
                </div>
            };
        }

        let is_fuzzy = resp.search_type.as_deref() == Some("fuzzy");

        html! {
            <div>
                <div class="flex items-center justify-between mb-4">
                    <p class="text-sm text-slate-600">{ heading }</p>
                    if is_fuzzy {
                        <span class="text-xs text-amber-700 bg-amber-50 border border-amber-200
                                     rounded-full px-2.5 py-1 font-medium">
                            {"Approximate matches"}
                        </span>
                    }
                </div>

                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mb-6">
                    { for resp.items.iter().map(article_card) }
                </div>

                // Pagination
                if resp.total_pages > 1 {
                    { pagination(resp.page, resp.total_pages, make_page_cb) }
                }
            </div>
        }
    } else {
        html! {
            <div class="text-center py-16 text-slate-400">
                <p>{"Start typing to search, or browse by category."}</p>
            </div>
        }
    }
}

fn pagination(
    current:      i64,
    total:        i64,
    make_page_cb: &dyn Fn(i64) -> Callback<MouseEvent>,
) -> Html {
    html! {
        <div class="flex items-center justify-center gap-2">
            <button
                onclick={make_page_cb(1)}
                disabled={current == 1}
                class={page_btn_class(false, current == 1)}
            >{"«"}</button>
            <button
                onclick={make_page_cb((current - 1).max(1))}
                disabled={current == 1}
                class={page_btn_class(false, current == 1)}
            >{"‹"}</button>

            { for visible_pages(current, total).into_iter().map(|p| {
                let active = p == current;
                html! {
                    <button
                        key={p}
                        onclick={make_page_cb(p)}
                        class={page_btn_class(active, false)}
                    >
                        { p }
                    </button>
                }
            }) }

            <button
                onclick={make_page_cb((current + 1).min(total))}
                disabled={current == total}
                class={page_btn_class(false, current == total)}
            >{"›"}</button>
            <button
                onclick={make_page_cb(total)}
                disabled={current == total}
                class={page_btn_class(false, current == total)}
            >{"»"}</button>
        </div>
    }
}

fn visible_pages(current: i64, total: i64) -> Vec<i64> {
    let window = 2i64;
    let start  = (current - window).max(1);
    let end    = (current + window).min(total);
    (start..=end).collect()
}

fn page_btn_class(active: bool, disabled: bool) -> &'static str {
    if disabled      { "w-9 h-9 rounded-lg text-sm text-slate-300 cursor-not-allowed" }
    else if active   { "w-9 h-9 rounded-lg text-sm bg-indigo-700 text-white font-bold" }
    else             { "w-9 h-9 rounded-lg text-sm border border-slate-200 \
                        text-slate-700 hover:bg-slate-50" }
}

fn sidebar_btn_class(active: bool) -> &'static str {
    if active {
        "w-full flex items-center text-left px-3 py-2 rounded-lg \
         bg-indigo-50 text-indigo-700 font-semibold text-sm"
    } else {
        "w-full flex items-center text-left px-3 py-2 rounded-lg \
         text-slate-700 text-sm hover:bg-slate-50 transition"
    }
}
