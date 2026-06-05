pub mod s3;

use async_trait::async_trait;
use bytes::Bytes;
use common::EmberTroveError;

/// Abstraction over S3-compatible object storage.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Upload raw bytes at the given key.
    async fn put(&self, key: &str, data: Bytes, content_type: &str) -> Result<(), EmberTroveError>;

    /// Download bytes from the given key.
    async fn get(&self, key: &str) -> Result<Bytes, EmberTroveError>;

    /// Delete the object at the given key.
    async fn delete(&self, key: &str) -> Result<(), EmberTroveError>;

    /// Generate a presigned download URL valid for `expires_secs`.
    async fn presigned_url(&self, key: &str, expires_secs: u32) -> Result<String, EmberTroveError>;
}

/// No-op store used when S3 credentials are not configured.
pub struct NullObjectStore;

#[async_trait]
impl ObjectStore for NullObjectStore {
    async fn put(
        &self,
        _key: &str,
        _data: Bytes,
        _content_type: &str,
    ) -> Result<(), EmberTroveError> {
        Err(EmberTroveError::Internal(
            "object storage is not configured".to_string(),
        ))
    }
    async fn get(&self, _key: &str) -> Result<Bytes, EmberTroveError> {
        Err(EmberTroveError::Internal(
            "object storage is not configured".to_string(),
        ))
    }
    async fn delete(&self, _key: &str) -> Result<(), EmberTroveError> {
        Err(EmberTroveError::Internal(
            "object storage is not configured".to_string(),
        ))
    }
    async fn presigned_url(
        &self,
        _key: &str,
        _expires_secs: u32,
    ) -> Result<String, EmberTroveError> {
        Err(EmberTroveError::Internal(
            "object storage is not configured".to_string(),
        ))
    }
}
