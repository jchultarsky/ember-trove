use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_s3::{
    Client, Config,
    config::{BehaviorVersion, Credentials, Region},
    presigning::PresigningConfig,
    primitives::ByteStream,
};
use bytes::Bytes;
use common::EmberTroveError;

use super::ObjectStore;

pub struct S3ObjectStore {
    client: Client,
    bucket: String,
}

impl S3ObjectStore {
    /// Build an S3ObjectStore. When `endpoint` is provided the client uses path-style
    /// access (required for MinIO); otherwise the standard AWS region is used.
    pub fn new(
        bucket_name: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
        endpoint: Option<&str>,
    ) -> Result<Self, EmberTroveError> {
        let creds = Credentials::new(access_key, secret_key, None, None, "ember-trove");

        let mut builder = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region.to_string()))
            .credentials_provider(creds)
            .force_path_style(true);

        if let Some(ep) = endpoint {
            builder = builder.endpoint_url(ep);
        }

        let client = Client::from_conf(builder.build());
        Ok(Self {
            client,
            bucket: bucket_name.to_string(),
        })
    }
}

#[async_trait]
impl ObjectStore for S3ObjectStore {
    async fn put(&self, key: &str, data: Bytes, content_type: &str) -> Result<(), EmberTroveError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("S3 put error: {e}")))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Bytes, EmberTroveError> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("S3 get error: {e}")))?;
        let data = output
            .body
            .collect()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("S3 read body error: {e}")))?
            .into_bytes();
        Ok(data)
    }

    async fn delete(&self, key: &str) -> Result<(), EmberTroveError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| EmberTroveError::Internal(format!("S3 delete error: {e}")))?;
        Ok(())
    }

    async fn presigned_url(&self, key: &str, expires_secs: u32) -> Result<String, EmberTroveError> {
        let presign_config =
            PresigningConfig::expires_in(Duration::from_secs(u64::from(expires_secs)))
                .map_err(|e| EmberTroveError::Internal(format!("presign config error: {e}")))?;
        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presign_config)
            .await
            .map_err(|e| EmberTroveError::Internal(format!("S3 presign error: {e}")))?;
        Ok(presigned.uri().to_string())
    }
}
