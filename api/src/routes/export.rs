//! Full-text export — streams a ZIP archive of all nodes owned by (or visible
//! to) the authenticated user as individual Markdown files with YAML
//! front-matter.
//!
//! `GET /export` → `application/zip`
//! Filename: `ember-trove-export-<YYYY-MM-DD>.zip`

use axum::{
    Extension, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use common::auth::AuthClaims;
use std::io::Write;

use crate::{auth::permissions::is_admin, error::ApiError, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(export_all))
}

async fn export_all(
    State(state): State<AppState>,
    Extension(claims): Extension<AuthClaims>,
) -> Result<impl IntoResponse, ApiError> {
    // Fetch all nodes visible to this user.
    // Admins get everything; regular users get only nodes they own or can see.
    let nodes = if is_admin(&claims) {
        state.nodes.list_all().await.map_err(ApiError::from)?
    } else {
        state
            .nodes
            .list_all_for_owner(&claims.sub)
            .await
            .map_err(ApiError::from)?
    };

    // Build ZIP in memory.
    let buf = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        for node in &nodes {
            // Build YAML front-matter block.
            let tags: Vec<String> = node.tags.iter().map(|t| t.name.clone()).collect();
            let tags_yaml = if tags.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{}]",
                    tags.iter()
                        .map(|t| format!("\"{t}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let node_type = format!("{:?}", node.node_type).to_lowercase();
            let status = format!("{:?}", node.status).to_lowercase();
            let front_matter = format!(
                "---\ntitle: \"{}\"\nid: {}\ntype: {}\nstatus: {}\nslug: {}\n\
                 tags: {}\ncreated_at: {}\nupdated_at: {}\n---\n\n",
                node.title.replace('"', "\\\""),
                node.id,
                node_type,
                status,
                node.slug,
                tags_yaml,
                node.created_at.format("%Y-%m-%dT%H:%M:%SZ"),
                node.updated_at.format("%Y-%m-%dT%H:%M:%SZ"),
            );

            let body = node.body.as_deref().unwrap_or("");
            let content = format!("{front_matter}{body}");

            // Sanitise slug for the filename (already URL-safe, but clamp length).
            let safe_slug: String = node
                .slug
                .chars()
                .filter(|c: &char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .take(120)
                .collect();
            let filename = format!("{safe_slug}.md");

            zip.start_file(&filename, options)
                .map_err(|e| ApiError::Internal(format!("zip start_file failed: {e}")))?;
            zip.write_all(content.as_bytes())
                .map_err(|e| ApiError::Internal(format!("zip write failed: {e}")))?;
        }

        let cursor = zip
            .finish()
            .map_err(|e| ApiError::Internal(format!("zip finish failed: {e}")))?;
        Ok(cursor.into_inner())
    })
    .await
    .map_err(|e| ApiError::Internal(format!("spawn_blocking failed: {e}")))??;

    let date = Utc::now().format("%Y-%m-%d");
    let disposition = format!("attachment; filename=\"ember-trove-export-{date}.zip\"");

    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition)
            .map_err(|e| ApiError::Internal(format!("invalid header: {e}")))?,
    );

    Ok((StatusCode::OK, headers, buf))
}
