use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ToastLevel {
    Success,
    Error,
    #[allow(dead_code)]
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u32,
    pub level: ToastLevel,
    pub message: String,
}

// ── State (held in context) ────────────────────────────────────────────────────

/// Shared toast state — placed in context at app root.
#[derive(Clone, Copy)]
pub struct ToastState {
    pub toasts: RwSignal<Vec<Toast>>,
    next_id: RwSignal<u32>,
}

impl ToastState {
    pub fn new() -> Self {
        Self {
            toasts: RwSignal::new(Vec::new()),
            next_id: RwSignal::new(0),
        }
    }

    pub fn push(&self, level: ToastLevel, message: impl Into<String>) {
        let id = self.next_id.get_untracked();
        self.next_id.update(|n| *n += 1);
        let toast = Toast {
            id,
            level,
            message: message.into(),
        };
        self.toasts.update(|ts| ts.push(toast));
        let toasts = self.toasts;
        spawn_local(async move {
            TimeoutFuture::new(3_500).await;
            toasts.update(|ts| ts.retain(|t| t.id != id));
        });
    }

    pub fn dismiss(&self, id: u32) {
        self.toasts.update(|ts| ts.retain(|t| t.id != id));
    }
}

// ── Free helper (callable from spawn_local / event handlers) ──────────────────

/// Push a toast. Must be called within a Leptos reactive owner.
pub fn push_toast(level: ToastLevel, message: impl Into<String>) {
    if let Some(state) = use_context::<ToastState>() {
        state.push(level, message);
    }
}

// ── Overlay component ──────────────────────────────────────────────────────────

#[component]
pub fn ToastOverlay() -> impl IntoView {
    let state = use_context::<ToastState>().expect("ToastState must be provided");

    view! {
        <div class="fixed bottom-24 right-6 z-50 flex flex-col gap-2 pointer-events-none">
            <For
                each=move || state.toasts.get()
                key=|t| t.id
                children=move |toast| {
                    let id = toast.id;
                    let (bg, icon) = match toast.level {
                        ToastLevel::Success => (
                            "bg-stone-900 dark:bg-stone-100 text-stone-50 dark:text-stone-900",
                            "check_circle",
                        ),
                        ToastLevel::Error => ("bg-red-600 text-white", "error"),
                        ToastLevel::Info  => ("bg-amber-600 text-white", "info"),
                    };
                    view! {
                        <div class=format!(
                            "toast-in flex items-center gap-2 pl-3 pr-2 py-2.5 rounded-xl                              shadow-xl text-sm font-medium pointer-events-auto {bg}"
                        )>
                            <span class="material-symbols-outlined flex-shrink-0"
                                  style="font-size: 16px;">{icon}</span>
                            <span class="flex-1">{toast.message.clone()}</span>
                            <button
                                class="ml-1 opacity-60 hover:opacity-100 transition-opacity flex-shrink-0"
                                on:click=move |_| state.dismiss(id)
                            >
                                <span class="material-symbols-outlined"
                                      style="font-size: 14px;">"close"</span>
                            </button>
                        </div>
                    }
                }
            />
        </div>
    }
}
