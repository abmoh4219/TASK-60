//! Operations console — configurable business rules page (Admin only).
//!
//! Shows all rules grouped by category.  Clicking a row opens an inline
//! editor that PATCHes the new value via the rules API.  Changes are
//! immediately reflected in the table and take effect server-side without
//! a restart.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, HtmlInputElement};
use yew::prelude::*;

use crate::api::rules::{self as rules_api, BusinessRule, UpdateRuleBody};
use crate::app::Route;
use crate::auth::AuthContext;

use super::OpsLayout;

// ── Component ──────────────────────────────────────────────────────────────────

#[function_component(OpsRulesPage)]
pub fn ops_rules_page() -> Html {
    let auth  = use_context::<AuthContext>().expect("AuthContext missing");
    let token = auth.token.clone().unwrap_or_default();

    // ── Data state ────────────────────────────────────────────────────────
    let rules   = use_state(Vec::<BusinessRule>::new);
    let loading = use_state(|| true);

    // ── Edit state ────────────────────────────────────────────────────────
    let editing_key = use_state(|| None::<String>);
    let edit_value  = use_state(String::new);
    let save_err    = use_state(|| None::<String>);
    let save_ok_key = use_state(|| None::<String>);

    // ── Load rules on mount ───────────────────────────────────────────────
    {
        let rules   = rules.clone();
        let loading = loading.clone();
        let token   = token.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                match rules_api::list_rules(&token).await {
                    Ok(r)  => { rules.set(r); loading.set(false); }
                    Err(_) => { loading.set(false); }
                }
            });
            || ()
        });
    }

    // ── Save handler ──────────────────────────────────────────────────────
    let on_save = {
        let (token, rules, editing_key, edit_value) =
            (token.clone(), rules.clone(), editing_key.clone(), edit_value.clone());
        let (save_err, save_ok_key) = (save_err.clone(), save_ok_key.clone());
        Callback::from(move |key: String| {
            let value = (*edit_value).trim().to_owned();
            if value.is_empty() { return; }
            let body  = UpdateRuleBody { value: value.clone() };
            let (token, rules, editing_key, save_err, save_ok_key) =
                (token.clone(), rules.clone(), editing_key.clone(), save_err.clone(), save_ok_key.clone());
            spawn_local(async move {
                match rules_api::update_rule(&token, &key, &body).await {
                    Ok(updated) => {
                        // Replace the matching rule in the list.
                        rules.set(
                            (*rules).iter().map(|r| {
                                if r.rule_key == updated.rule_key { updated.clone() } else { r.clone() }
                            }).collect()
                        );
                        editing_key.set(None);
                        save_err.set(None);
                        save_ok_key.set(Some(key));
                    }
                    Err(e) => { save_err.set(Some(e.message)); }
                }
            });
        })
    };

    // ── Group rules by category ───────────────────────────────────────────
    let grouped = group_by_category(&rules);

    // ── Render ────────────────────────────────────────────────────────────
    html! {
        <OpsLayout active={Route::OpsRules}>
            <div class="space-y-6 max-w-4xl">
                <div class="flex items-center justify-between">
                    <h1 class="text-xl font-semibold text-gray-900">{"Business Rules"}</h1>
                    <p class="text-sm text-gray-500">
                        {"Changes take effect immediately — no restart required."}
                    </p>
                </div>

                if *loading {
                    <p class="text-sm text-gray-400 animate-pulse">{"Loading rules…"}</p>
                } else {
                    { for grouped.iter().map(|(category, rules)| {
                        let category = *category;
                        html! {
                            <div class="space-y-2">
                                <h2 class="text-xs font-semibold text-gray-500 uppercase tracking-wide">
                                    { category }
                                </h2>
                                <div class="bg-white rounded-lg border border-gray-200 divide-y divide-gray-100">
                                { for rules.iter().map(|rule| {
                                    let key        = rule.rule_key.clone();
                                    let is_editing = editing_key.as_deref() == Some(&rule.rule_key);
                                    let just_saved = save_ok_key.as_deref() == Some(&rule.rule_key);
                                    let on_save    = on_save.clone();
                                    let on_edit = {
                                        let editing_key = editing_key.clone();
                                        let edit_value  = edit_value.clone();
                                        let save_err    = save_err.clone();
                                        let save_ok_key = save_ok_key.clone();
                                        let v           = rule.rule_value.clone();
                                        let k           = rule.rule_key.clone();
                                        Callback::from(move |_: MouseEvent| {
                                            editing_key.set(Some(k.clone()));
                                            edit_value.set(v.clone());
                                            save_err.set(None);
                                            save_ok_key.set(None);
                                        })
                                    };
                                    let on_cancel = {
                                        let editing_key = editing_key.clone();
                                        let save_err    = save_err.clone();
                                        Callback::from(move |_: MouseEvent| {
                                            editing_key.set(None);
                                            save_err.set(None);
                                        })
                                    };
                                    html! {
                                        <div class="px-4 py-3 flex items-start gap-4">
                                            // Key + description
                                            <div class="flex-1 min-w-0">
                                                <p class="text-sm font-mono text-gray-800">
                                                    { &rule.rule_key }
                                                </p>
                                                if let Some(desc) = &rule.description {
                                                    <p class="text-xs text-gray-500 mt-0.5">{ desc }</p>
                                                }
                                                if let Some(msg) = save_err.as_ref() {
                                                    if is_editing {
                                                        <p class="text-xs text-red-600 mt-1">{ msg }</p>
                                                    }
                                                }
                                            </div>
                                            // Value / editor
                                            <div class="flex items-center gap-2 shrink-0">
                                                if is_editing {
                                                    <input
                                                        type="text"
                                                        class="rounded border border-blue-400 text-sm px-2 py-1 w-36
                                                               focus:outline-none focus:ring-2 focus:ring-blue-500"
                                                        value={(*edit_value).clone()}
                                                        onchange={{
                                                            let edit_value = edit_value.clone();
                                                            Callback::from(move |e: Event| {
                                                                if let Some(el) = e.target()
                                                                    .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                                                {
                                                                    edit_value.set(el.value());
                                                                }
                                                            })
                                                        }}
                                                        // Enter = save, Escape = discard.
                                                        onkeydown={{
                                                            let on_save     = on_save.clone();
                                                            let key         = key.clone();
                                                            let editing_key = editing_key.clone();
                                                            let save_err    = save_err.clone();
                                                            Callback::from(move |e: KeyboardEvent| {
                                                                match e.key().as_str() {
                                                                    "Enter"  => on_save.emit(key.clone()),
                                                                    "Escape" => {
                                                                        editing_key.set(None);
                                                                        save_err.set(None);
                                                                    }
                                                                    _ => {}
                                                                }
                                                            })
                                                        }}
                                                    />
                                                    <button
                                                        class="text-xs rounded bg-blue-600 px-2.5 py-1 text-white hover:bg-blue-700"
                                                        onclick={{ let on_save = on_save.clone(); let key = key.clone(); Callback::from(move |_| on_save.emit(key.clone())) }}>
                                                        {"Save"}
                                                    </button>
                                                    <button
                                                        class="text-xs rounded border border-gray-300 px-2.5 py-1 hover:bg-gray-50"
                                                        onclick={on_cancel}>
                                                        {"Cancel"}
                                                    </button>
                                                } else {
                                                    <span class={if just_saved {
                                                        "text-sm font-mono text-green-700 font-semibold"
                                                    } else {
                                                        "text-sm font-mono text-gray-700"
                                                    }}>
                                                        { &rule.rule_value }
                                                        if just_saved { {" ✓"} }
                                                    </span>
                                                    <button
                                                        class="text-xs text-blue-600 hover:underline"
                                                        onclick={on_edit}>
                                                        {"Edit"}
                                                    </button>
                                                }
                                            </div>
                                        </div>
                                    }
                                })}
                                </div>
                            </div>
                        }
                    })}
                }
            </div>
        </OpsLayout>
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Group rules by category label, preserving alphabetical key order within each group.
fn group_by_category(rules: &[BusinessRule]) -> Vec<(&'static str, Vec<&BusinessRule>)> {
    // Stable ordered category list.
    let order: &[&str] = &[
        "Orders & Refunds",
        "Security & Sessions",
        "Content Quality",
        "Data Ingestion",
        "General",
    ];
    let mut map: std::collections::HashMap<&'static str, Vec<&BusinessRule>> =
        std::collections::HashMap::new();
    for rule in rules {
        map.entry(rule.category()).or_default().push(rule);
    }
    order
        .iter()
        .filter_map(|&cat| map.remove(cat).map(|v| (cat, v)))
        .collect()
}
