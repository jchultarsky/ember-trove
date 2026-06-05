use common::{
    id::SearchPresetId,
    search::{CreateSearchPresetRequest, SearchPreset, SearchResponse},
};
use gloo_net::http::Request;

use super::{api_url, delete_empty, get_json, parse_json, post_json};
use crate::error::UiError;

/// Search nodes.
///
/// - `status`: optional status filter (`Some("published")` etc.)
/// - `node_type`: optional node-type filter (`Some("article")` etc.)
/// - `tag_ids`: zero or more tag UUIDs to filter by
/// - `tag_op`: `"or"` (default) or `"and"` — how to combine multiple tags
/// - `sort`: optional sort order (`"relevance"`, `"updated_desc"`, `"updated_asc"`,
///   `"title_asc"`, `"title_desc"`)
/// - `updated_after` / `updated_before`: optional date bounds in `YYYY-MM-DD` format
#[allow(clippy::too_many_arguments)]
pub async fn search_nodes(
    q: &str,
    fuzzy: bool,
    status: Option<&str>,
    node_type: Option<&str>,
    tag_ids: &[uuid::Uuid],
    tag_op: &str,
    sort: Option<&str>,
    updated_after: Option<&str>,
    updated_before: Option<&str>,
    page: u32,
    per_page: u32,
) -> Result<SearchResponse, UiError> {
    let encoded_q: String = js_sys::encode_uri_component(q).into();
    let mut url = format!(
        "{}?q={encoded_q}&fuzzy={fuzzy}&page={page}&per_page={per_page}",
        api_url("/search")
    );
    if let Some(s) = status {
        url.push_str(&format!("&status={}", js_sys::encode_uri_component(s)));
    }
    if let Some(nt) = node_type {
        url.push_str(&format!("&node_type={nt}"));
    }
    if !tag_ids.is_empty() {
        let ids_str = tag_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        url.push_str(&format!("&tag_ids={ids_str}&tag_op={tag_op}"));
    }
    if let Some(s) = sort {
        url.push_str(&format!("&sort={s}"));
    }
    if let Some(d) = updated_after {
        url.push_str(&format!("&updated_after={d}"));
    }
    if let Some(d) = updated_before {
        url.push_str(&format!("&updated_before={d}"));
    }
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| UiError::Network(e.to_string()))?;
    parse_json(resp).await
}

/// Lightweight wrapper for the node picker — returns up to 8 results for `q`.
/// Returns an empty list when `q` is blank (callers should skip the call in
/// that case anyway, but this is a safe fallback).
pub async fn node_picker_search(q: &str) -> Result<Vec<common::search::SearchResult>, UiError> {
    if q.trim().is_empty() {
        return Ok(vec![]);
    }
    search_nodes(
        q,
        false,
        None,
        None,
        &[],
        "or",
        Some("title_asc"),
        None,
        None,
        1,
        8,
    )
    .await
    .map(|r| r.results)
}

// ── Search presets ─────────────────────────────────────────────────────────────

pub async fn fetch_search_presets() -> Result<Vec<SearchPreset>, UiError> {
    get_json("/search-presets").await
}

pub async fn create_search_preset(
    req: &CreateSearchPresetRequest,
) -> Result<SearchPreset, UiError> {
    post_json("/search-presets", req).await
}

pub async fn delete_search_preset(id: SearchPresetId) -> Result<(), UiError> {
    delete_empty(&format!("/search-presets/{id}")).await
}
