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

/// Auto-dismiss delay for plain toasts.
const TOAST_MS: u32 = 3_500;
/// Auto-dismiss delay for toasts carrying an action (e.g. Undo) — longer so
/// the user has time to react.
const ACTION_TOAST_MS: u32 = 8_000;

/// An action button rendered inside a toast (e.g. "Undo").
///
/// `on_click` is a plain `Arc` closure, NOT a Leptos `Callback`: a `Callback`
/// is arena-allocated under the creating component's owner, and the deleting
/// row unmounts (list refetch) while the toast lives on — clicking Undo would
/// hit a disposed callback. An `Arc` closure has no reactive owner; it must
/// capture only owner-independent state (Copy signals from app-root contexts,
/// ids) — never `use_context` at call time. (`Arc + Send + Sync` rather than
/// `Rc` because `RwSignal` contents must be `Send + Sync`; on single-threaded
/// WASM the markers are vacuous.)
#[derive(Clone)]
pub struct ToastAction {
    pub label: &'static str,
    pub on_click: std::sync::Arc<dyn Fn() + Send + Sync>,
}

#[derive(Clone)]
pub struct Toast {
    pub id: u32,
    pub level: ToastLevel,
    pub message: String,
    pub action: Option<ToastAction>,
}

// Manual impl: `Callback` has no `PartialEq`; the `id` uniquely identifies a
// toast anyway.
impl PartialEq for Toast {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.level == other.level && self.message == other.message
    }
}

// ── State (held in context) ────────────────────────────────────────────────────

thread_local! {
    /// Global handle to the app's single ToastState (WASM is single-threaded).
    ///
    /// Needed because `use_context` only works under a reactive owner — and
    /// code resumed after an `.await` inside `wasm_bindgen_futures::spawn_local`
    /// has none, so context lookups there return `None` and the toast is
    /// silently dropped (v2.21.0: undo toasts never rendered in production).
    /// `push_toast` / `push_undo_toast` fall back to this handle.
    static GLOBAL_TOAST_STATE: std::cell::Cell<Option<ToastState>> =
        const { std::cell::Cell::new(None) };
}

/// Shared toast state — placed in context at app root.
#[derive(Clone, Copy)]
pub struct ToastState {
    pub toasts: RwSignal<Vec<Toast>>,
    next_id: RwSignal<u32>,
}

impl ToastState {
    pub fn new() -> Self {
        let state = Self {
            toasts: RwSignal::new(Vec::new()),
            next_id: RwSignal::new(0),
        };
        GLOBAL_TOAST_STATE.with(|g| g.set(Some(state)));
        state
    }

    pub fn push(&self, level: ToastLevel, message: impl Into<String>) {
        self.push_inner(level, message.into(), None, TOAST_MS);
    }

    /// Push a toast with an action button (e.g. "Undo"); stays visible
    /// [`ACTION_TOAST_MS`] so the user has time to react.
    pub fn push_with_action(
        &self,
        level: ToastLevel,
        message: impl Into<String>,
        action: ToastAction,
    ) {
        self.push_inner(level, message.into(), Some(action), ACTION_TOAST_MS);
    }

    fn push_inner(
        &self,
        level: ToastLevel,
        message: String,
        action: Option<ToastAction>,
        dismiss_after_ms: u32,
    ) {
        let id = self.next_id.get_untracked();
        self.next_id.update(|n| *n += 1);
        let toast = Toast {
            id,
            level,
            message,
            action,
        };
        self.toasts.update(|ts| ts.push(toast));
        let toasts = self.toasts;
        spawn_local(async move {
            TimeoutFuture::new(dismiss_after_ms).await;
            toasts.update(|ts| ts.retain(|t| t.id != id));
        });
    }

    pub fn dismiss(&self, id: u32) {
        self.toasts.update(|ts| ts.retain(|t| t.id != id));
    }
}

// ── Free helpers (callable from spawn_local / event handlers) ─────────────────

/// Resolve the toast state: reactive context when an owner is present,
/// otherwise the global handle (post-`.await` continuations have no owner).
fn resolve_state() -> Option<ToastState> {
    use_context::<ToastState>().or_else(|| GLOBAL_TOAST_STATE.with(std::cell::Cell::get))
}

/// Push a toast. Callable from anywhere, including `spawn_local`
/// continuations after an `.await`.
pub fn push_toast(level: ToastLevel, message: impl Into<String>) {
    if let Some(state) = resolve_state() {
        state.push(level, message);
    }
}

/// Push a success toast with an "Undo" button. Clicking it runs `on_undo`
/// and dismisses the toast. Callable from anywhere; `on_undo` must not rely
/// on a reactive owner (see [`ToastAction`]).
pub fn push_undo_toast(
    message: impl Into<String>,
    on_undo: std::sync::Arc<dyn Fn() + Send + Sync>,
) {
    if let Some(state) = resolve_state() {
        state.push_with_action(
            ToastLevel::Success,
            message,
            ToastAction {
                label: "Undo",
                on_click: on_undo,
            },
        );
    }
}

// ── Overlay component ──────────────────────────────────────────────────────────

#[component]
pub fn ToastOverlay() -> impl IntoView {
    let state = expect_context::<ToastState>();

    view! {
        <div
            class="fixed bottom-24 right-6 z-50 flex flex-col gap-2 pointer-events-none"
            role="status"
            aria-live="polite"
        >
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
                            {toast.action.clone().map(|a| {
                                let ToastAction { label, on_click } = a;
                                view! {
                                    <button
                                        class="ml-1 px-2 py-0.5 rounded-lg font-semibold flex-shrink-0
                                               underline underline-offset-2 opacity-90 hover:opacity-100
                                               transition-opacity"
                                        on:click=move |_| {
                                            on_click();
                                            state.dismiss(id);
                                        }
                                    >
                                        {label}
                                    </button>
                                }
                            })}
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
