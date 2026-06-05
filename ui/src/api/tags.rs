use super::{delete_empty, get_json, post_action, post_json, put_json};
use crate::error::UiError;
use common::{
    id::{NodeId, TagId},
    tag::{CreateTagRequest, Tag, UpdateTagRequest},
};

pub async fn fetch_tags() -> Result<Vec<Tag>, UiError> {
    get_json("/tags").await
}

pub async fn create_tag(req: &CreateTagRequest) -> Result<Tag, UiError> {
    post_json("/tags", req).await
}

pub async fn update_tag(id: TagId, req: &UpdateTagRequest) -> Result<Tag, UiError> {
    put_json(&format!("/tags/{id}"), req).await
}

pub async fn delete_tag(id: TagId) -> Result<(), UiError> {
    delete_empty(&format!("/tags/{id}")).await
}

pub async fn fetch_tags_for_node(node_id: NodeId) -> Result<Vec<Tag>, UiError> {
    get_json(&format!("/nodes/{node_id}/tags")).await
}

pub async fn attach_tag(node_id: NodeId, tag_id: TagId) -> Result<(), UiError> {
    post_action(&format!("/nodes/{node_id}/tags/{tag_id}")).await
}

pub async fn detach_tag(node_id: NodeId, tag_id: TagId) -> Result<(), UiError> {
    delete_empty(&format!("/nodes/{node_id}/tags/{tag_id}")).await
}
