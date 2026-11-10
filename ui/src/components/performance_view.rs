//! Admin → Performance (performance phase, 2026-11-02).
//!
//! The API's own timings since it started, read from the admin-only
//! `GET /api/metrics`, and the database's slowest statements from
//! `GET /api/metrics/db`. Both reset with every deploy (the database's on a
//! Postgres restart), so the page says "since the last deploy". It refreshes
//! only when asked: an open tab adds no load.
//!
//! Nothing shown is user data: routes are patterns (`/api/nodes/{id}`), and
//! statements are Postgres's normalised text (`$1` for every value).

use common::metrics::{
    DbStatements, Latency, Metrics, RouteLatency, RouteSort, format_ms, format_uptime, sort_routes,
};
use leptos::prelude::*;

use crate::api;
use crate::components::page_header::PageHeader;

/// Text colour for a latency: warm only when it's slow (heat tokens).
fn latency_class(ms: f64) -> &'static str {
    match Latency::of(ms) {
        Latency::Fine => "",
        Latency::Slow => "text-heat-high font-medium",
        Latency::VerySlow => "text-heat-max font-semibold",
    }
}

#[component]
pub fn PerformanceView() -> impl IntoView {
    let refresh = RwSignal::new(0u32);
    let metrics = LocalResource::new(move || {
        refresh.track();
        api::fetch_metrics()
    });
    let statements = LocalResource::new(move || {
        refresh.track();
        api::fetch_db_statements()
    });

    view! {
        <div class="flex flex-col h-full">
            <PageHeader
                icon="speed"
                title="Performance"
                subtitle="Server timings since the last deploy. Refresh to update.".into_any()
            >
                <button
                    class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm font-medium \
                        text-amber-700 dark:text-amber-400 border border-amber-300 dark:border-amber-800 \
                        hover:bg-amber-50 dark:hover:bg-amber-950/40 transition-colors"
                    on:click=move |_| refresh.update(|n| *n += 1)
                >
                    <span class="material-symbols-outlined" style="font-size: 16px;" aria-hidden="true">"refresh"</span>
                    "Refresh"
                </button>
            </PageHeader>
            <div class="flex-1 overflow-y-auto px-4 md:px-6 py-4 space-y-6">
                <Transition fallback=|| view! {
                    <p class="text-sm text-stone-400 animate-pulse">"Loading timings\u{2026}"</p>
                }>
                    {move || metrics.get().map(|result| match result {
                        Err(e) => view! {
                            <p class="text-sm text-red-600 dark:text-red-400">
                                {format!("Couldn't load the timings: {e}")}
                            </p>
                        }.into_any(),
                        Ok(m) => metrics_view(m).into_any(),
                    })}
                </Transition>
                <Transition fallback=|| view! {
                    <p class="text-sm text-stone-400 animate-pulse">"Loading statements\u{2026}"</p>
                }>
                    {move || statements.get().map(|result| match result {
                        Err(e) => view! {
                            <p class="text-sm text-red-600 dark:text-red-400">
                                {format!("Couldn't load the database statements: {e}")}
                            </p>
                        }.into_any(),
                        Ok(s) => statements_view(s).into_any(),
                    })}
                </Transition>
            </div>
        </div>
    }
}

/// One figure in the summary row.
fn stat(label: &'static str, value: String, class: &'static str) -> impl IntoView {
    view! {
        <div class="rounded-lg border border-stone-200 dark:border-stone-800 px-3 py-2">
            <div class="text-xs text-stone-500 dark:text-stone-400">{label}</div>
            <div class=format!("text-base tabular-nums text-stone-900 dark:text-stone-100 {class}")>
                {value}
            </div>
        </div>
    }
}

fn metrics_view(m: Metrics) -> impl IntoView {
    let r = &m.requests;
    let in_use = m.db.pool_size.saturating_sub(m.db.pool_idle);
    let errors_class = if r.status_5xx > 0 {
        "text-heat-max font-semibold"
    } else {
        ""
    };
    let c = &m.counts;
    let rows = format!(
        "{} nodes · {} tasks · {} notes · {} edges · {} tags · {} files",
        c.nodes, c.tasks, c.notes, c.edges, c.tags, c.attachments
    );
    view! {
        <section class="space-y-2" data-testid="perf-summary">
            <div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-2">
                {stat("Version", m.version.clone(), "")}
                {stat("Up", format_uptime(m.uptime_secs), "")}
                {stat("Requests", r.total.to_string(), "")}
                {stat("Client errors (4xx)", r.status_4xx.to_string(), "")}
                {stat("Server errors (5xx)", r.status_5xx.to_string(), errors_class)}
                {stat("DB connections", format!("{in_use} of {} in use", m.db.pool_size), "")}
            </div>
            <p class="text-xs text-stone-500 dark:text-stone-400">{rows}</p>
        </section>
        <RoutesTable routes=m.routes />
    }
}

/// Column headers that sort. Each is a word-only button; the active one
/// carries `aria-sort`.
const COLUMNS: [(&str, Option<RouteSort>); 7] = [
    ("Route", Some(RouteSort::Route)),
    ("Count", Some(RouteSort::Count)),
    ("Total", Some(RouteSort::Total)),
    ("p50", None),
    ("p95", Some(RouteSort::P95)),
    ("p99", None),
    ("Max", Some(RouteSort::Max)),
];

#[component]
fn RoutesTable(routes: Vec<RouteLatency>) -> impl IntoView {
    let sort = RwSignal::new(RouteSort::default());
    let routes = StoredValue::new(routes);
    let sorted = Memo::new(move |_| {
        let mut rs = routes.get_value();
        sort_routes(&mut rs, sort.get());
        rs
    });
    let th = "px-2 py-1.5 font-medium text-stone-500 dark:text-stone-400 whitespace-nowrap";
    // Decided once from the fixed list, not from `sorted`: only the rows
    // react to a sort, so the header buttons (and keyboard focus) stay.
    let body = if routes.with_value(Vec::is_empty) {
        view! {
                    <p class="text-sm text-stone-500 dark:text-stone-400">"No requests yet since the deploy."</p>
                }.into_any()
    } else {
        view! {
                    <div class="overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800">
                        <table class="w-full text-xs tabular-nums" data-testid="perf-routes">
                            <thead class="bg-stone-50 dark:bg-stone-900">
                                <tr>
                                    {COLUMNS.into_iter().map(|(label, key)| {
                                        let align = if label == "Route" { "text-left" } else { "text-right" };
                                        match key {
                                            None => view! { <th class=format!("{th} {align}")>{label}</th> }.into_any(),
                                            Some(k) => view! {
                                                <th
                                                    class=format!("{th} {align}")
                                                    aria-sort=move || if sort.get() == k {
                                                        if k == RouteSort::Route { "ascending" } else { "descending" }
                                                    } else { "none" }
                                                >
                                                    <button
                                                        class=move || if sort.get() == k {
                                                            "text-stone-900 dark:text-stone-100 underline underline-offset-2"
                                                        } else {
                                                            "hover:text-stone-900 dark:hover:text-stone-100"
                                                        }
                                                        on:click=move |_| sort.set(k)
                                                    >
                                                        {label}
                                                    </button>
                                                </th>
                                            }.into_any(),
                                        }
                                    }).collect_view()}
                                </tr>
                            </thead>
                            <tbody class="divide-y divide-stone-100 dark:divide-stone-800">
                                {move || sorted.get().into_iter().map(|r| view! {
                                    <tr>
                                        <td class="px-2 py-1.5 font-mono text-stone-800 dark:text-stone-200 break-all">{r.route}</td>
                                        <td class="px-2 py-1.5 text-right">{r.count}</td>
                                        <td class="px-2 py-1.5 text-right">{format_ms(r.total_ms)}</td>
                                        <td class=format!("px-2 py-1.5 text-right {}", latency_class(r.p50_ms))>{format_ms(r.p50_ms)}</td>
                                        <td class=format!("px-2 py-1.5 text-right {}", latency_class(r.p95_ms))>{format_ms(r.p95_ms)}</td>
                                        <td class=format!("px-2 py-1.5 text-right {}", latency_class(r.p99_ms))>{format_ms(r.p99_ms)}</td>
                                        <td class=format!("px-2 py-1.5 text-right {}", latency_class(r.max_ms))>{format_ms(r.max_ms)}</td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                    <p class="text-xs text-stone-500 dark:text-stone-400">
                        "Percentiles are bucket bounds, accurate to one bucket. Times run to the response head; a streamed download keeps going after it."
                    </p>
                }.into_any()
    };

    view! {
        <section class="space-y-2">
            <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">"API routes"</h2>
            {body}
        </section>
    }
}

fn statements_view(s: DbStatements) -> impl IntoView {
    let th =
        "px-2 py-1.5 font-medium text-stone-500 dark:text-stone-400 whitespace-nowrap text-right";
    let body = if !s.enabled {
        view! {
            <p class="text-sm text-stone-500 dark:text-stone-400">
                "Database statistics aren't enabled on this server (pg_stat_statements)."
            </p>
        }
        .into_any()
    } else if s.statements.is_empty() {
        view! {
            <p class="text-sm text-stone-500 dark:text-stone-400">"No statements recorded yet."</p>
        }
        .into_any()
    } else {
        view! {
            <div class="overflow-x-auto rounded-lg border border-stone-200 dark:border-stone-800">
                <table class="w-full text-xs tabular-nums" data-testid="perf-statements">
                    <thead class="bg-stone-50 dark:bg-stone-900">
                        <tr>
                            <th class="px-2 py-1.5 font-medium text-stone-500 dark:text-stone-400 text-left">"Statement"</th>
                            <th class=th>"Calls"</th>
                            <th class=th>"Total"</th>
                            <th class=th>"Mean"</th>
                            <th class=th>"Max"</th>
                            <th class=th>"Rows"</th>
                            <th class=th>"Disk reads"</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-stone-100 dark:divide-stone-800">
                        {s.statements.into_iter().map(|q| view! {
                            <tr class="align-top">
                                <td class="px-2 py-1.5 font-mono text-stone-800 dark:text-stone-200 break-all min-w-[16rem]">{q.query}</td>
                                <td class="px-2 py-1.5 text-right">{q.calls}</td>
                                <td class="px-2 py-1.5 text-right">{format_ms(q.total_ms)}</td>
                                <td class=format!("px-2 py-1.5 text-right {}", latency_class(q.mean_ms))>{format_ms(q.mean_ms)}</td>
                                <td class=format!("px-2 py-1.5 text-right {}", latency_class(q.max_ms))>{format_ms(q.max_ms)}</td>
                                <td class="px-2 py-1.5 text-right">{q.rows}</td>
                                <td class="px-2 py-1.5 text-right">{q.blks_read}</td>
                            </tr>
                        }).collect_view()}
                    </tbody>
                </table>
            </div>
        }
        .into_any()
    };
    view! {
        <section class="space-y-2">
            <h2 class="text-sm font-semibold text-stone-700 dark:text-stone-300">"Slowest database statements"</h2>
            {body}
            <p class="text-xs text-stone-500 dark:text-stone-400">
                "Reading and writing statements only, as Postgres normalised them: values become placeholders ($1) and comments are removed. Table and column names stay."
            </p>
        </section>
    }
}
