//! Kiosk archive page.
//!
//! Two modes, driven entirely by URL query params:
//!
//! • **Index mode** — no `year`+`month` in URL.
//!   Shows a year/month grid with article counts.  Clicking a cell
//!   navigates to month mode.  Optionally filtered by `category`.
//!
//! • **Month mode** — `year` and `month` both present.
//!   Shows a paginated article grid for that calendar month with
//!   an optional `category` filter in the sidebar.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::kiosk::{
    self, category_label, month_name,
    ArchiveArticlesResponse, ArchiveEntry, ArchiveIndexResponse, CategoryCount,
};
use crate::app::Route;
use crate::pages::kiosk::home::{article_card, footer, nav_bar};

// ── URL query params ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, Default)]
pub struct ArchiveQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year:     Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month:    Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page:     Option<i64>,
}

// ── Internal data state ────────────────────────────────────────────────────────

enum ArchiveData {
    Index(ArchiveIndexResponse),
    Articles(ArchiveArticlesResponse),
}

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component(KioskArchivePage)]
pub fn kiosk_archive() -> Html {
    let location  = use_location().unwrap();
    let navigator = use_navigator().unwrap();

    let params: ArchiveQuery = location.query().unwrap_or_default();

    let data       = use_state(|| Option::<ArchiveData>::None);
    let loading    = use_state(|| true);
    let error      = use_state(|| Option::<String>::None);
    let cat_counts = use_state(|| Vec::<CategoryCount>::new());

    // Load sidebar category counts once on mount.
    {
        let cat_counts = cat_counts.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                if let Ok(c) = kiosk::list_categories().await {
                    cat_counts.set(c);
                }
            });
            || ()
        });
    }

    // Re-fetch archive data whenever URL params change.
    {
        let data    = data.clone();
        let loading = loading.clone();
        let error   = error.clone();
        use_effect_with(params.clone(), move |p| {
            let p       = p.clone();
            let data    = data.clone();
            let loading = loading.clone();
            let error   = error.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                match (p.year, p.month) {
                    (Some(y), Some(m)) => {
                        match kiosk::get_archive_articles(
                            y, m, p.category.as_deref(), p.page.unwrap_or(1),
                        ).await {
                            Ok(r)  => { data.set(Some(ArchiveData::Articles(r))); loading.set(false); }
                            Err(e) => { error.set(Some(e.message)); loading.set(false); }
                        }
                    }
                    _ => {
                        match kiosk::get_archive_index(p.year, p.category.as_deref()).await {
                            Ok(r)  => { data.set(Some(ArchiveData::Index(r))); loading.set(false); }
                            Err(e) => { error.set(Some(e.message)); loading.set(false); }
                        }
                    }
                }
            });
            || ()
        });
    }

    // ── Navigation callbacks ───────────────────────────────────────────────

    // Navigate to month view (preserves category filter).
    let make_month_cb = |year: i32, month: i32| {
        let nav = navigator.clone();
        let cat = params.category.clone();
        Callback::from(move |_: MouseEvent| {
            nav.push_with_query(&Route::KioskArchive, &ArchiveQuery {
                year: Some(year), month: Some(month),
                category: cat.clone(), page: None,
            }).ok();
        })
    };

    // Switch category filter (preserves year/month selection).
    let make_category_cb = |cat: Option<String>| {
        let nav   = navigator.clone();
        let year  = params.year;
        let month = params.month;
        Callback::from(move |_: MouseEvent| {
            nav.push_with_query(&Route::KioskArchive, &ArchiveQuery {
                year, month, category: cat.clone(), page: None,
            }).ok();
        })
    };

    // Navigate to a specific page (preserves all other params).
    let make_page_cb = |page: i64| {
        let nav    = navigator.clone();
        let params = params.clone();
        Callback::from(move |_: MouseEvent| {
            let mut q = params.clone();
            q.page = Some(page);
            nav.push_with_query(&Route::KioskArchive, &q).ok();
        })
    };

    // ── Pre-compute main content ───────────────────────────────────────────
    let content: Html = if *loading {
        archive_skeleton()
    } else if let Some(msg) = &*error {
        html! {
            <div class="bg-red-50 border border-red-200 rounded-2xl p-6 text-red-700">
                <p class="font-semibold mb-1">{"Unable to load archive"}</p>
                <p class="text-sm">{ msg.clone() }</p>
            </div>
        }
    } else if let Some(d) = &*data {
        match d {
            ArchiveData::Index(idx)    => index_view(idx, &make_month_cb),
            ArchiveData::Articles(arts) => articles_view(arts, &make_page_cb),
        }
    } else {
        html! {}
    };

    // ── Breadcrumb (needs params, not borrowed by content) ─────────────────
    let crumb_year  = params.year;
    let crumb_month = params.month;
    let breadcrumb = html! {
        <nav class="flex items-center gap-2 text-sm text-gray-500 mb-8"
             aria-label="Breadcrumb">
            <Link<Route> to={Route::Kiosk}
                classes="hover:text-blue-700 transition">{"Home"}</Link<Route>>
            <span aria-hidden="true">{"/"}</span>
            <Link<Route> to={Route::KioskArchive}
                classes="hover:text-blue-700 transition">{"Archive"}</Link<Route>>
            {
                if let Some(y) = crumb_year {
                    html! {
                        <>
                            <span aria-hidden="true">{"/"}</span>
                            <span class="text-gray-700">{ y.to_string() }</span>
                        </>
                    }
                } else { html! {} }
            }
            {
                if let (Some(_), Some(m)) = (crumb_year, crumb_month) {
                    html! {
                        <>
                            <span aria-hidden="true">{"/"}</span>
                            <span class="text-gray-700">{ month_name(m) }</span>
                        </>
                    }
                } else { html! {} }
            }
        </nav>
    };

    html! {
        <div class="min-h-screen bg-slate-50">
            { nav_bar() }
            <div class="max-w-5xl mx-auto px-6 py-10">
                { breadcrumb }

                <div class="flex gap-8">
                    // ── Sidebar ───────────────────────────────────────────
                    <aside class="w-52 shrink-0 hidden lg:block">
                        <div class="bg-white rounded-2xl border border-gray-200 p-4">
                            <h3 class="text-xs font-semibold text-gray-500 uppercase
                                       tracking-wider mb-3">
                                {"Category"}
                            </h3>
                            <ul class="space-y-1">
                                <li>
                                    <button
                                        onclick={make_category_cb(None)}
                                        class={sidebar_btn_class(params.category.is_none())}
                                    >
                                        {"All topics"}
                                    </button>
                                </li>
                                { for (*cat_counts).iter().map(|cc| {
                                    let active = params.category.as_deref() == Some(&cc.category);
                                    let cat    = cc.category.clone();
                                    let cnt    = cc.count;
                                    html! {
                                        <li key={cat.clone()}>
                                            <button
                                                onclick={make_category_cb(Some(cat))}
                                                class={sidebar_btn_class(active)}
                                            >
                                                <span>{ category_label(&cc.category) }</span>
                                                <span class="ml-auto text-xs text-gray-400">
                                                    { cnt }
                                                </span>
                                            </button>
                                        </li>
                                    }
                                }) }
                            </ul>
                        </div>
                    </aside>

                    // ── Main content ──────────────────────────────────────
                    <div class="flex-1 min-w-0">
                        { content }
                    </div>
                </div>
            </div>
            { footer() }
        </div>
    }
}

// ── View functions ─────────────────────────────────────────────────────────────

/// Archive index: year/month grid with article counts.
fn index_view(
    idx:           &ArchiveIndexResponse,
    make_month_cb: &dyn Fn(i32, i32) -> Callback<MouseEvent>,
) -> Html {
    if idx.entries.is_empty() {
        return html! {
            <div class="text-center py-20">
                <div class="text-5xl mb-4">{"📅"}</div>
                <p class="text-gray-500">{"No archived articles yet."}</p>
            </div>
        };
    }

    // Group consecutive entries by year (already sorted DESC by the backend).
    let mut by_year: Vec<(i32, Vec<&ArchiveEntry>)> = Vec::new();
    for entry in &idx.entries {
        if let Some(last) = by_year.last_mut() {
            if last.0 == entry.year {
                last.1.push(entry);
                continue;
            }
        }
        by_year.push((entry.year, vec![entry]));
    }

    html! {
        <div>
            <h1 class="text-2xl font-bold text-gray-900 mb-8">{"Article Archive"}</h1>
            { for by_year.iter().map(|(year, entries)| {
                let y = *year;
                html! {
                    <div key={y} class="mb-10">
                        <h2 class="text-lg font-semibold text-gray-500 mb-4">{ y }</h2>
                        <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
                            { for entries.iter().map(|e| {
                                html! {
                                    <button
                                        key={e.month}
                                        onclick={make_month_cb(e.year, e.month)}
                                        class="bg-white rounded-xl border border-gray-200 p-4 \
                                               text-left hover:shadow-md hover:border-blue-300 \
                                               active:scale-95 transition group"
                                    >
                                        <div class="font-semibold text-gray-800
                                                    group-hover:text-blue-700 transition">
                                            { month_name(e.month) }
                                        </div>
                                        <div class="text-xs text-gray-400 mt-1">
                                            { format!("{} articles", e.count) }
                                        </div>
                                    </button>
                                }
                            }) }
                        </div>
                    </div>
                }
            }) }
        </div>
    }
}

/// Month view: paginated article list for a specific year+month.
fn articles_view(
    arts:         &ArchiveArticlesResponse,
    make_page_cb: &dyn Fn(i64) -> Callback<MouseEvent>,
) -> Html {
    let heading = format!("{} {}", month_name(arts.month), arts.year);

    html! {
        <div>
            <div class="flex items-center justify-between mb-6">
                <h1 class="text-2xl font-bold text-gray-900">{ heading }</h1>
                <span class="text-sm text-gray-500">
                    { format!("{} articles", arts.total) }
                </span>
            </div>

            if arts.items.is_empty() {
                <div class="text-center py-20">
                    <div class="text-5xl mb-4">{"📭"}</div>
                    <p class="text-gray-500">{"No articles for this period."}</p>
                </div>
            } else {
                <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mb-6">
                    { for arts.items.iter().map(article_card) }
                </div>
                if arts.total_pages > 1 {
                    { archive_pagination(arts.page, arts.total_pages, make_page_cb) }
                }
            }
        </div>
    }
}

// ── Pagination ─────────────────────────────────────────────────────────────────

fn archive_pagination(
    current:      i64,
    total:        i64,
    make_page_cb: &dyn Fn(i64) -> Callback<MouseEvent>,
) -> Html {
    let window: i64 = 2;
    let pages: Vec<i64> = ((current - window).max(1)..=(current + window).min(total)).collect();

    html! {
        <div class="flex items-center justify-center gap-2 mt-4">
            <button onclick={make_page_cb(1)} disabled={current == 1}
                class={page_btn_class(false, current == 1)}>{"«"}</button>
            <button onclick={make_page_cb((current - 1).max(1))} disabled={current == 1}
                class={page_btn_class(false, current == 1)}>{"‹"}</button>

            { for pages.into_iter().map(|p| html! {
                <button key={p} onclick={make_page_cb(p)}
                    class={page_btn_class(p == current, false)}>{ p }</button>
            }) }

            <button onclick={make_page_cb((current + 1).min(total))} disabled={current == total}
                class={page_btn_class(false, current == total)}>{"›"}</button>
            <button onclick={make_page_cb(total)} disabled={current == total}
                class={page_btn_class(false, current == total)}>{"»"}</button>
        </div>
    }
}

// ── Skeletons & style helpers ──────────────────────────────────────────────────

fn archive_skeleton() -> Html {
    html! {
        <div class="animate-pulse">
            <div class="h-8 bg-gray-200 rounded w-48 mb-8"></div>
            <div class="mb-10">
                <div class="h-5 bg-gray-200 rounded w-12 mb-4"></div>
                <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
                    { for (0..8).map(|_| html! {
                        <div class="bg-white rounded-xl border border-gray-200 p-4 h-16"></div>
                    }) }
                </div>
            </div>
        </div>
    }
}

fn sidebar_btn_class(active: bool) -> &'static str {
    if active {
        "w-full flex items-center text-left px-3 py-2 rounded-lg \
         bg-blue-50 text-blue-700 font-semibold text-sm"
    } else {
        "w-full flex items-center text-left px-3 py-2 rounded-lg \
         text-gray-700 text-sm hover:bg-gray-50 transition"
    }
}

fn page_btn_class(active: bool, disabled: bool) -> &'static str {
    if disabled      { "w-9 h-9 rounded-lg text-sm text-gray-300 cursor-not-allowed" }
    else if active   { "w-9 h-9 rounded-lg text-sm bg-blue-700 text-white font-bold" }
    else             { "w-9 h-9 rounded-lg text-sm border border-gray-200 \
                        text-gray-700 hover:bg-gray-50" }
}
