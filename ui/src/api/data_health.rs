use common::data_health::{DataHealthReport, RuleCount};

use super::get_json;
use crate::error::UiError;

/// The caller's data-health report (#208): their own data that breaks the
/// data model, per rule.
pub async fn fetch_data_health() -> Result<DataHealthReport, UiError> {
    get_json("/data-health").await
}

/// Admin only: each rule's count across every account. Counts only.
pub async fn fetch_admin_data_health() -> Result<Vec<RuleCount>, UiError> {
    get_json("/admin/data-health").await
}
