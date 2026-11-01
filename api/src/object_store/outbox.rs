//! Draining the S3 deletion outbox (#239; `repo::object_deletion`).

use super::ObjectStore;
use crate::repo::object_deletion::ObjectDeletionRepo;

/// How many queued objects one batch tries.
pub const DRAIN_LIMIT: i64 = 500;

/// Batches one drain runs at most (a bound, not a target).
const MAX_BATCHES: usize = 100;

/// Delete queued objects, a batch at a time, until the queue is empty or a
/// batch deletes nothing (everything left is failing). A deleted object
/// (S3 treats an already-missing one as deleted) drops its row; a failure
/// keeps it for the next drain. Returns (deleted, failed). Logs counts
/// only: keys contain file names.
pub async fn drain(queue: &dyn ObjectDeletionRepo, store: &dyn ObjectStore) -> (usize, usize) {
    let (mut deleted, mut failed) = (0, 0);
    for _ in 0..MAX_BATCHES {
        let (d, f, full) = drain_batch(queue, store).await;
        deleted += d;
        failed += f;
        if !full || d == 0 {
            break;
        }
    }
    if failed > 0 {
        tracing::warn!(deleted, failed, "some stored files couldn't be deleted yet");
    } else if deleted > 0 {
        tracing::info!(deleted, "deleted stored files");
    }
    (deleted, failed)
}

/// One batch: (deleted, failed, whether the batch was full).
async fn drain_batch(
    queue: &dyn ObjectDeletionRepo,
    store: &dyn ObjectStore,
) -> (usize, usize, bool) {
    let keys = match queue.due(DRAIN_LIMIT).await {
        Ok(keys) => keys,
        Err(e) => {
            tracing::warn!(?e, "reading the object deletion queue failed");
            return (0, 0, false);
        }
    };
    let full = i64::try_from(keys.len()).is_ok_and(|n| n >= DRAIN_LIMIT);
    let (mut deleted, mut failed) = (0, 0);
    for key in &keys {
        let outcome = match store.delete(key).await {
            Ok(()) => queue.done(key).await.map(|()| true),
            Err(_) => queue.failed(key).await.map(|()| false),
        };
        match outcome {
            Ok(true) => deleted += 1,
            Ok(false) => failed += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!(?e, "updating the object deletion queue failed");
            }
        }
    }
    (deleted, failed, full)
}
