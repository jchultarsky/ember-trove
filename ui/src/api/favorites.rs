use common::{
    favorite::{CreateFavoriteRequest, Favorite, ReorderFavoritesRequest},
    id::FavoriteId,
};

use super::{delete_empty, get_json, patch_json, post_json};
use crate::error::UiError;

pub async fn fetch_favorites() -> Result<Vec<Favorite>, UiError> {
    get_json("/favorites").await
}

pub async fn create_favorite(req: &CreateFavoriteRequest) -> Result<Favorite, UiError> {
    post_json("/favorites", req).await
}

pub async fn delete_favorite(id: FavoriteId) -> Result<(), UiError> {
    delete_empty(&format!("/favorites/{id}")).await
}

pub async fn reorder_favorites(req: &ReorderFavoritesRequest) -> Result<Vec<Favorite>, UiError> {
    patch_json("/favorites/reorder", req).await
}
