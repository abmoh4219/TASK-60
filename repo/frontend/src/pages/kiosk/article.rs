//! Kiosk article detail page.
//!
//! Features:
//! - Slug-based article fetch; re-fetches when slug changes (e.g. from related links)
//! - Body rendered as paragraphs split on `\n\n`
//! - Tag chips navigate to search page with tag filter
//! - Quality-score badge (High / Medium / Low)
//! - Related articles grid at the bottom of the page
//! - Full-page loading skeleton and error state

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::kiosk::{self, ArticleResponse};
use crate::app::Route;
use crate::pages::kiosk::home::{article_card, footer, nav_bar};
use crate::pages::kiosk::search::SearchQuery;

// ── Props ──────────────────────────────────────────────────────────────────────

#[derive(Properties, PartialEq)]
pub struct ArticleProps {
    pub slug: String,
}

// ── Component ─────────────────────────────────────────────────────────────────

#[function_component(KioskArticlePage)]
pub fn kiosk_article(props: &ArticleProps) -> Html {
    let navigator = use_navigator().unwrap();
    let data      = use_state(|| Option::<ArticleResponse>::None);
    let loading   = use_state(|| true);
    let error     = use_state(|| Option::<String>::None);

    // Fetch full article + related whenever the slug changes.
    {
        let data    = data.clone();
        let loading = loading.clone();
        let error   = error.clone();
        let slug    = props.slug.clone();
        use_effect_with(slug, move |slug| {
            let slug    = slug.clone();
            let data    = data.clone();
            let loading = loading.clone();
            let error   = error.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                match kiosk::get_article(&slug).await {
                    Ok(r)  => { data.set(Some(r)); loading.set(false); }
                    Err(e) => { error.set(Some(e.message)); loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Pre-build body HTML (needs navigator for tag callbacks) ───────────
    let body_html: Html = if *loading {
        article_skeleton()
    } else if let Some(msg) = &*error {
        html! {
            <div class="bg-red-50 border border-red-200 rounded-2xl p-8 text-center">
                <h2 class="text-lg font-semibold text-red-800 mb-2">{"Article not found"}</h2>
                <p class="text-red-600 text-sm mb-6">{ msg.clone() }</p>
                <Link<Route> to={Route::KioskSearch}
                    classes="inline-block rounded-lg bg-indigo-700 px-5 py-2.5 text-white \
                             text-sm font-medium hover:bg-indigo-800 transition">
                    {"Browse all articles"}
                </Link<Route>>
            </div>
        }
    } else if let Some(resp) = &*data {
        let a     = &resp.article;
        let color = a.category_color().to_owned();
        let label = a.category_label().to_owned();
        let date  = a.display_date();

        // ── Tag chips ─────────────────────────────────────────────────────
        let tags_html: Html = if a.tags.is_empty() {
            html! {}
        } else {
            let nav = navigator.clone();
            html! {
                <div class="flex flex-wrap gap-2 mb-6">
                    { for a.tags.iter().map(|tag| {
                        let nav2 = nav.clone();
                        let t    = tag.clone();
                        let onclick = Callback::from(move |_: MouseEvent| {
                            nav2.push_with_query(&Route::KioskSearch, &SearchQuery {
                                tag: Some(t.clone()),
                                ..Default::default()
                            }).ok();
                        });
                        html! {
                            <button
                                key={tag.clone()}
                                onclick={onclick}
                                class="text-xs rounded-full px-3 py-1 bg-slate-100 text-slate-700 \
                                       hover:bg-indigo-100 hover:text-indigo-700 transition font-medium"
                            >
                                { format!("#{tag}") }
                            </button>
                        }
                    }) }
                </div>
            }
        };

        // ── Body paragraphs ───────────────────────────────────────────────
        let paras: Vec<Html> = a.body
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| html! { <p class="text-slate-700 leading-relaxed mb-4">{ p }</p> })
            .collect();
        let body_content = if paras.is_empty() {
            html! {
                <p class="text-slate-700 leading-relaxed whitespace-pre-line">{ &a.body }</p>
            }
        } else {
            html! { <div>{ for paras.into_iter() }</div> }
        };

        // ── Related articles ──────────────────────────────────────────────
        let related_html: Html = if resp.related.is_empty() {
            html! {}
        } else {
            html! {
                <section class="mt-12">
                    <h2 class="text-xl font-bold text-slate-900 mb-5">{"Related Articles"}</h2>
                    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
                        { for resp.related.iter().map(article_card) }
                    </div>
                </section>
            }
        };

        html! {
            <div>
                // ── Breadcrumb ────────────────────────────────────────────
                <nav class="flex items-center gap-2 text-sm text-slate-500 mb-8"
                     aria-label="Breadcrumb">
                    <Link<Route> to={Route::Kiosk}
                        classes="hover:text-indigo-700 transition">{"Home"}</Link<Route>>
                    <span aria-hidden="true" class="text-slate-300">{"/"}</span>
                    <Link<Route> to={Route::KioskSearch}
                        classes="hover:text-indigo-700 transition">{"All Articles"}</Link<Route>>
                    <span aria-hidden="true" class="text-slate-300">{"/"}</span>
                    <span class="text-slate-800 truncate max-w-xs">{ &a.title }</span>
                </nav>

                // ── Article ───────────────────────────────────────────────
                <article class="bg-white rounded-2xl border border-slate-200 p-8 shadow-sm mb-4">
                    // Category badge
                    <div class={format!(
                        "inline-block text-xs font-semibold px-3 py-1 rounded-full mb-4 {color}"
                    )}>
                        { label }
                    </div>

                    // Title
                    <h1 class="text-3xl font-bold text-slate-900 leading-tight mb-4">
                        { &a.title }
                    </h1>

                    // Meta row
                    <div class="flex flex-wrap items-center gap-4 text-sm text-slate-500
                                mb-6 pb-6 border-b border-slate-100">
                        if !date.is_empty() {
                            <time>{ date }</time>
                        }
                        if let Some(qs) = &a.quality_score {
                            { quality_badge(qs) }
                        }
                        if let Some(rid) = &a.route_id {
                            <span class="font-medium text-indigo-700">
                                { format!("Route {rid}") }
                            </span>
                        }
                    </div>

                    // Tags
                    { tags_html }

                    // Body
                    { body_content }

                    // Source attribution
                    if let Some(url) = &a.source_url {
                        <div class="mt-8 pt-6 border-t border-slate-100 text-xs text-slate-400">
                            {"Source: "}
                            <span class="font-mono break-all">{ url }</span>
                        </div>
                    }
                </article>

                { related_html }
            </div>
        }
    } else {
        html! {}
    };

    html! {
        <div class="min-h-screen bg-slate-50">
            { nav_bar() }
            <div class="max-w-4xl mx-auto px-6 py-10">
                { body_html }
            </div>
            { footer() }
        </div>
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Coloured quality badge derived from the `"92.50"` string the backend sends.
fn quality_badge(qs: &str) -> Html {
    let score: f64 = qs.parse().unwrap_or(0.0);
    let (color, label) = if score >= 80.0 {
        ("bg-green-100 text-green-800", "High Quality")
    } else if score >= 50.0 {
        ("bg-yellow-100 text-yellow-800", "Medium Quality")
    } else {
        ("bg-red-100 text-red-800", "Low Quality")
    };
    html! {
        <span class={format!("text-xs font-medium px-2.5 py-0.5 rounded-full {color}")}>
            { label }
        </span>
    }
}

fn article_skeleton() -> Html {
    html! {
        <div class="animate-pulse">
            // Breadcrumb row
            <div class="flex items-center gap-3 mb-8">
                <div class="h-4 bg-slate-200 rounded w-12"></div>
                <div class="h-4 bg-slate-200 rounded w-3"></div>
                <div class="h-4 bg-slate-200 rounded w-20"></div>
                <div class="h-4 bg-slate-200 rounded w-3"></div>
                <div class="h-4 bg-slate-200 rounded w-44"></div>
            </div>
            // Card
            <div class="bg-white rounded-2xl border border-slate-200 p-8">
                <div class="h-5 bg-slate-200 rounded-full w-20 mb-5"></div>
                <div class="h-9 bg-slate-200 rounded w-3/4 mb-3"></div>
                <div class="h-9 bg-slate-200 rounded w-1/2 mb-6"></div>
                <div class="h-px bg-slate-100 mb-6"></div>
                { for (0..8).map(|_| html! {
                    <div class="h-4 bg-slate-100 rounded w-full mb-3"></div>
                }) }
            </div>
        </div>
    }
}
