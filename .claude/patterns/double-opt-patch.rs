//! Pattern: tri-state PATCH fields with `Option<Option<T>>`.
//!
//! A PATCH endpoint must distinguish three client intents:
//!   - field ABSENT       → leave unchanged   → None
//!   - field present, null → clear the value  → Some(None)
//!   - field present, value→ set the value    → Some(Some(v))
//!
//! serde alone can't express this: `#[serde(default)]` gives `None` for absent, but a
//! plain `Option<T>` can't tell "null" from "absent". The custom deserializer below,
//! combined with `#[serde(default)]`, yields the correct three-way mapping.
//!
//! Distilled from common/src/task.rs (`deser_double_opt`, used on UpdateTaskRequest).

use serde::{Deserialize, Deserializer};

/// Deserialises `Option<Option<T>>`:
/// - field absent        → `None`        (via `#[serde(default)]` on the field)
/// - field present/null  → `Some(None)`
/// - field present/value → `Some(Some(v))`
fn deser_double_opt<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    // If the field reached this fn at all it was present; deserialize the inner
    // Option (which captures null vs value) and wrap in Some.
    Ok(Some(Option::<T>::deserialize(d)?))
}

#[derive(Debug, Clone, Deserialize)]
struct UpdateTaskRequest {
    // Plain optional: absent or value (no "clear" semantics needed).
    pub title: Option<String>,

    // Tri-state: None = leave unchanged · Some(None) = clear · Some(Some(d)) = set.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deser_double_opt")]
    pub due_date: Option<Option<String>>,
}
