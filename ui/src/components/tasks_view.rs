//! Unified Tasks area: one route family (`/tasks/*`) with a tab bar above
//! the existing My Day, Inbox, and Calendar views.  Consolidates three
//! sidebar entries into one without changing the inner views.

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::components::{
    calendar_view::CalendarView, inbox_view::InboxView, my_day_view::MyDayView,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TasksTab {
    MyDay,
    Inbox,
    Calendar,
}

impl TasksTab {
    fn label(self) -> &'static str {
        match self {
            Self::MyDay => "My Day",
            Self::Inbox => "Inbox",
            Self::Calendar => "Calendar",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::MyDay => "wb_sunny",
            Self::Inbox => "inbox",
            Self::Calendar => "calendar_month",
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::MyDay => "/tasks/my-day",
            Self::Inbox => "/tasks/inbox",
            Self::Calendar => "/tasks/calendar",
        }
    }
    /// Tab to the right (wraps) — for ArrowRight on the tablist.
    fn next(self) -> Self {
        match self {
            Self::MyDay => Self::Inbox,
            Self::Inbox => Self::Calendar,
            Self::Calendar => Self::MyDay,
        }
    }
    /// Tab to the left (wraps) — for ArrowLeft on the tablist.
    fn prev(self) -> Self {
        match self {
            Self::MyDay => Self::Calendar,
            Self::Inbox => Self::MyDay,
            Self::Calendar => Self::Inbox,
        }
    }
}

#[component]
pub fn TasksView(active: TasksTab) -> impl IntoView {
    view! {
        <div class="flex flex-col h-full">
            <TasksTabBar active=active />
            <div class="flex-1 overflow-hidden flex flex-col min-h-0">
                {match active {
                    TasksTab::MyDay    => view! { <MyDayView    /> }.into_any(),
                    TasksTab::Inbox    => view! { <InboxView    /> }.into_any(),
                    TasksTab::Calendar => view! { <CalendarView /> }.into_any(),
                }}
            </div>
        </div>
    }
}

#[component]
fn TasksTabBar(active: TasksTab) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    // ARIA tabs keyboard support: left/right arrows switch tabs (automatic
    // activation — switching navigates the route).
    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let target = match ev.key().as_str() {
            "ArrowRight" => active.next(),
            "ArrowLeft" => active.prev(),
            _ => return,
        };
        ev.prevent_default();
        navigate.get_value()(target.path(), Default::default());
    };
    view! {
        <div class="flex items-center gap-1 px-4 sm:px-6 pt-2 border-b border-stone-200 dark:border-stone-800
                    bg-stone-50/50 dark:bg-stone-900/40"
             role="tablist" aria-label="Tasks views"
             on:keydown=on_keydown>
            <TabButton tab=TasksTab::MyDay    active=active />
            <TabButton tab=TasksTab::Inbox    active=active />
            <TabButton tab=TasksTab::Calendar active=active />
        </div>
    }
}

#[component]
fn TabButton(tab: TasksTab, active: TasksTab) -> impl IntoView {
    let navigate = StoredValue::new(use_navigate());
    let is_active = tab == active;
    let base = "flex items-center gap-1.5 px-3 py-2 text-sm font-medium cursor-pointer \
                transition-colors border-b-2 -mb-px whitespace-nowrap";
    let styling = if is_active {
        "border-amber-500 text-amber-700 dark:text-amber-400"
    } else {
        "border-transparent text-stone-500 dark:text-stone-400 \
         hover:text-stone-800 dark:hover:text-stone-200 \
         hover:border-stone-300 dark:hover:border-stone-700"
    };
    let class = format!("{base} {styling}");
    let path = tab.path();
    let label = tab.label();
    let icon = tab.icon();

    view! {
        <button
            type="button"
            class=class
            role="tab"
            aria-selected=if is_active { "true" } else { "false" }
            aria-current=if is_active { "page" } else { "false" }
            // Roving tabindex (ARIA APG): the active tab is the tab stop;
            // arrows on the tablist move between tabs.
            tabindex=if is_active { "0" } else { "-1" }
            on:click=move |_| {
                if !is_active {
                    navigate.get_value()(path, Default::default());
                }
            }
        >
            <span class="material-symbols-outlined" style="font-size: 16px;">{icon}</span>
            <span>{label}</span>
        </button>
    }
}
