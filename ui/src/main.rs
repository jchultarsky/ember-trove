mod api;
mod app;
mod auth;
mod components;
mod draft;
mod error;
mod focus_note;
mod focus_task;
mod markdown;
mod recent;
mod templates;
mod wikilink;

use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use tracing_web::MakeConsoleWriter;

use app::App;

fn init_tracing() {
    let filter = option_env!("RUST_LOG")
        .and_then(|s| s.parse::<EnvFilter>().ok())
        .unwrap_or_else(|| EnvFilter::new("warn"));

    let fmt_layer = fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(MakeConsoleWriter);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}

fn main() {
    init_tracing();
    leptos::mount::mount_to_body(App);
    // Remove the static HTML loading screen now that the Leptos tree is in the
    // DOM. Done after mount so there is no flash of empty content between the
    // two states.
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app-loader"))
    {
        el.remove();
    }
}
