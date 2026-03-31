//! Toast notification hook.
//!
//! Usage inside any function component:
//! ```rust
//! let toasts = use_toasts();
//! toasts.push.emit(("Saved!".into(), ToastKind::Success));
//! // In html!:
//! { toasts.view }
//! ```
//!
//! Toasts auto-dismiss after 4 s and can also be closed manually.

use wasm_bindgen::JsCast;
use yew::prelude::*;

// ── Data types ─────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum ToastKind {
    Success,
    Error,
    Warning,
    Info,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ToastItem {
    pub id:      u32,
    pub message: String,
    pub kind:    ToastKind,
}

// ── Return value from use_toasts ───────────────────────────────────────────────

pub struct UseToasts {
    /// Emit to display a new toast.
    pub push: Callback<(String, ToastKind)>,
    /// Include this in your `html!` tree to render the toast stack.
    pub view: Html,
}

// ── Hook ───────────────────────────────────────────────────────────────────────

pub fn use_toasts() -> UseToasts {
    let items:   UseStateHandle<Vec<ToastItem>> = use_state(Vec::new);
    let next_id: UseStateHandle<u32>            = use_state(|| 0u32);

    let push = {
        let items   = items.clone();
        let next_id = next_id.clone();
        Callback::from(move |(message, kind): (String, ToastKind)| {
            let id = *next_id + 1;
            next_id.set(id);

            let mut v = (*items).clone();
            v.push(ToastItem { id, message, kind });
            items.set(v);

            // Auto-dismiss after 4 s.
            let items = items.clone();
            gloo_timers::callback::Timeout::new(4_000, move || {
                items.set(
                    (*items).iter().filter(|t| t.id != id).cloned().collect(),
                );
            })
            .forget();
        })
    };

    let view = render_stack(&items);
    UseToasts { push, view }
}

// ── Rendering ──────────────────────────────────────────────────────────────────

fn render_stack(items: &UseStateHandle<Vec<ToastItem>>) -> Html {
    if items.is_empty() {
        return html! {};
    }

    let toasts: Html = items
        .iter()
        .map(|t| render_one(t, items.clone()))
        .collect();

    html! {
        <div
            aria-live="polite"
            aria-atomic="false"
            class="fixed bottom-6 right-6 z-[9999] flex flex-col gap-2 pointer-events-none"
        >
            { toasts }
        </div>
    }
}

fn render_one(t: &ToastItem, items: UseStateHandle<Vec<ToastItem>>) -> Html {
    let id = t.id;
    let on_close = {
        let items = items.clone();
        Callback::from(move |_: MouseEvent| {
            items.set((*items).iter().filter(|t| t.id != id).cloned().collect());
        })
    };

    let (bg, border_l, icon_html, text_cls) = match t.kind {
        ToastKind::Success => (
            "bg-white",
            "border-l-emerald-500",
            html! { <span class="text-emerald-500">
                { crate::components::icons::check_circle("w-5 h-5") }
            </span> },
            "text-slate-800",
        ),
        ToastKind::Error => (
            "bg-white",
            "border-l-red-500",
            html! { <span class="text-red-500">
                { crate::components::icons::x_circle("w-5 h-5") }
            </span> },
            "text-slate-800",
        ),
        ToastKind::Warning => (
            "bg-white",
            "border-l-amber-500",
            html! { <span class="text-amber-500">
                { crate::components::icons::exclamation_triangle("w-5 h-5") }
            </span> },
            "text-slate-800",
        ),
        ToastKind::Info => (
            "bg-white",
            "border-l-sky-500",
            html! { <span class="text-sky-500">
                { crate::components::icons::information_circle("w-5 h-5") }
            </span> },
            "text-slate-800",
        ),
    };

    html! {
        <div
            role="alert"
            class={format!(
                "pointer-events-auto flex items-start gap-3 rounded-lg border border-slate-200 \
                 border-l-4 {border_l} {bg} pl-3 pr-4 py-3 shadow-panel \
                 min-w-[300px] max-w-sm animate-fade-in"
            )}
        >
            { icon_html }
            <p class={format!("flex-1 text-sm font-medium {text_cls}")}>
                { &t.message }
            </p>
            <button
                onclick={on_close}
                aria-label="Dismiss"
                class="shrink-0 text-slate-400 hover:text-slate-600 transition-colors mt-0.5"
            >
                { crate::components::icons::x_mark("w-4 h-4") }
            </button>
        </div>
    }
}
