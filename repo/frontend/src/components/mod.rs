//! Shared UI component library.
//!
//! Sub-modules:
//!   `icons`  — inline Heroicons SVG functions
//!   `toast`  — `use_toasts()` hook + `ToastKind` enum
//!
//! Plus small helpers (`status_badge`, `metric_card`, `empty_state`, etc.)
//! used consistently across every ops and kiosk page.

pub mod icons;
pub mod toast;

pub use toast::{use_toasts, ToastKind};

use yew::prelude::*;

// ── Status badge ───────────────────────────────────────────────────────────────

/// Coloured pill badge — auto-maps lower-case DB value to Tailwind colours.
pub fn status_badge(status: &str) -> Html {
    let (bg, text) = match status {
        "approved" | "confirmed" | "active" | "on_time" | "accepted" | "completed" =>
            ("bg-emerald-100", "text-emerald-800"),
        "pending" | "proposed" | "open" | "held" =>
            ("bg-amber-100", "text-amber-800"),
        "rejected" | "cancelled" | "expired" | "failed" =>
            ("bg-red-100", "text-red-700"),
        "delayed" =>
            ("bg-orange-100", "text-orange-800"),
        "assigned" =>
            ("bg-indigo-100", "text-indigo-800"),
        "running" | "in_progress" =>
            ("bg-sky-100", "text-sky-800"),
        _ =>
            ("bg-slate-100", "text-slate-700"),
    };
    html! {
        <span class={format!(
            "inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium {bg} {text}"
        )}>
            { status.replace('_', " ") }
        </span>
    }
}

// ── Metric card ───────────────────────────────────────────────────────────────

pub struct MetricCard {
    pub label:   &'static str,
    pub value:   String,
    pub sub:     Option<&'static str>,
    pub icon:    Html,
    pub accent:  &'static str,
    pub bg_icon: &'static str,
}

pub fn metric_card(c: MetricCard) -> Html {
    html! {
        <div class="bg-white rounded-xl border border-slate-200/80 shadow-card p-5 flex items-start gap-4">
            <div class={format!("rounded-xl p-2.5 shrink-0 {}", c.bg_icon)}>
                <span class={c.accent}>{ c.icon }</span>
            </div>
            <div class="min-w-0">
                <p class="text-[11px] font-semibold text-slate-500 uppercase tracking-wide truncate">
                    { c.label }
                </p>
                <p class="mt-1 text-2xl font-bold text-slate-900 leading-none tabular-nums">
                    { c.value }
                </p>
                if let Some(sub) = c.sub {
                    <p class="mt-1 text-xs text-slate-400">{ sub }</p>
                }
            </div>
        </div>
    }
}

// ── Empty state ───────────────────────────────────────────────────────────────

pub fn empty_state(icon: Html, title: &str, body: &str) -> Html {
    html! {
        <div class="flex flex-col items-center justify-center py-16 text-center">
            <span class="text-slate-200 mb-4">{ icon }</span>
            <p class="text-sm font-semibold text-slate-500">{ title }</p>
            <p class="mt-1 text-xs text-slate-400 max-w-xs">{ body }</p>
        </div>
    }
}

// ── Skeleton rows (for table loading state) ───────────────────────────────────

pub fn skeleton_rows(n: usize) -> Html {
    (0..n)
        .map(|_| {
            html! {
                <tr class="border-b border-slate-100">
                    <td class="px-4 py-3.5" colspan="6">
                        <div class="skeleton h-4 w-3/4"></div>
                    </td>
                </tr>
            }
        })
        .collect()
}

// ── Consistent class strings ──────────────────────────────────────────────────

pub fn input_cls() -> &'static str {
    "block w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm \
     text-slate-900 placeholder-slate-400 shadow-sm \
     focus:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/20 \
     disabled:bg-slate-50 disabled:text-slate-400"
}

pub fn select_cls() -> &'static str {
    "block rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm \
     text-slate-900 shadow-sm focus:border-indigo-500 focus:outline-none \
     focus:ring-2 focus:ring-indigo-500/20"
}

pub fn btn_primary() -> &'static str {
    "inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3.5 py-2 text-sm \
     font-semibold text-white shadow-sm hover:bg-indigo-700 active:bg-indigo-800 \
     focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 \
     focus-visible:outline-indigo-600 disabled:opacity-50 transition-colors"
}

pub fn btn_secondary() -> &'static str {
    "inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-white \
     px-3.5 py-2 text-sm font-semibold text-slate-700 shadow-sm \
     hover:bg-slate-50 active:bg-slate-100 transition-colors disabled:opacity-50"
}

pub fn btn_danger() -> &'static str {
    "inline-flex items-center gap-1.5 rounded-lg bg-red-600 px-3.5 py-2 text-sm \
     font-semibold text-white shadow-sm hover:bg-red-700 active:bg-red-800 \
     disabled:opacity-50 transition-colors"
}

pub fn btn_ghost() -> &'static str {
    "inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm \
     font-medium text-slate-600 hover:bg-slate-100 active:bg-slate-200 transition-colors"
}
