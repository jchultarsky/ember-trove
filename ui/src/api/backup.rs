use gloo_net::http::Request;

use super::{api_url, delete_empty, get_json, parse_json, post_action, send_authed};
use crate::error::UiError;

pub async fn list_backups() -> Result<Vec<common::backup::BackupJob>, UiError> {
    get_json("/admin/backups").await
}

pub async fn create_backup_api(
    comment: Option<String>,
) -> Result<common::backup::BackupJob, UiError> {
    let url = api_url("/admin/backups");
    let resp = send_authed(|| {
        let builder = Request::post(&url);
        match &comment {
            Some(c) => builder
                .header("Content-Type", "application/json")
                .body(serde_json::json!({ "comment": c }).to_string()),
            None => builder.build(),
        }
        .map_err(|e| UiError::Parse(e.to_string()))
    })
    .await?;
    parse_json(resp).await
}

pub async fn delete_backup(id: uuid::Uuid) -> Result<(), UiError> {
    delete_empty(&format!("/admin/backups/{id}")).await
}

pub async fn preview_backup_restore(
    id: uuid::Uuid,
) -> Result<common::backup::BackupPreview, UiError> {
    get_json(&format!("/admin/backups/{id}/preview")).await
}

pub async fn restore_backup(id: uuid::Uuid) -> Result<(), UiError> {
    post_action(&format!("/admin/backups/{id}/restore")).await
}
