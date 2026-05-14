use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ToolError {
    #[error("stale handle: ref no longer valid")]
    StaleHandle { ref_id: String },

    #[error("not actionable")]
    NotActionable {
        available_actions: Vec<String>,
        fallback_suggestion: Option<String>,
    },

    #[error("target not found")]
    TargetNotFound { closest_match: Option<String> },

    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },

    #[error("tier restricted")]
    TierRestricted {
        app_class: String,
        current_tier: String,
        allowed_actions: Vec<String>,
    },

    #[error("approval required")]
    AppApprovalRequired {
        app_class: String,
        notification_id: Option<u32>,
        still_pending: bool,
    },

    #[error("cancelled at {cancelled_at_stage}")]
    Cancelled { cancelled_at_stage: String },

    #[error("lock unavailable; held by pid {holder_pid}")]
    LockUnavailable { holder_pid: u32, lock_path: String },

    #[error("timeout")]
    Timeout {
        closest_match: Option<String>,
        current_windows: Vec<String>,
    },

    #[error("input dispatch failed: {detail}")]
    InputDispatchFailed {
        detail: String,
        libei_version: Option<String>,
        kwin_version: Option<String>,
    },

    #[error("external command failed: {stderr}")]
    ExternalCommandFailed { stderr: String },

    #[error("bus disconnected: {detail}")]
    BusDisconnected { detail: String },

    #[error("not implemented: this tool is a Phase 0 stub")]
    NotImplemented,
}

impl ToolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::StaleHandle { .. } => "stale_handle",
            Self::NotActionable { .. } => "not_actionable",
            Self::TargetNotFound { .. } => "target_not_found",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::TierRestricted { .. } => "tier_restricted",
            Self::AppApprovalRequired { .. } => "app_approval_required",
            Self::Cancelled { .. } => "cancelled",
            Self::LockUnavailable { .. } => "lock_unavailable",
            Self::Timeout { .. } => "timeout",
            Self::InputDispatchFailed { .. } => "input_dispatch_failed",
            Self::ExternalCommandFailed { .. } => "external_command_failed",
            Self::BusDisconnected { .. } => "bus_disconnected",
            Self::NotImplemented => "not_implemented",
        }
    }
}

/// Boundary conversion for the `?` operator at the rmcp handler edge.
///
/// Semantic tool failures should NOT pass through this conversion — they ride
/// on `CallToolResult::error(...)` carrying the structured `{ code, details }`
/// payload from `tool_result()` in `src/server.rs`. This `From` exists so an
/// internal error (e.g. an `anyhow::Result` returned through `?`) at the rmcp
/// boundary doesn't lose its message.
impl From<ToolError> for rmcp::ErrorData {
    fn from(e: ToolError) -> Self {
        let message = e.to_string();
        let data = serde_json::to_value(&e).ok();
        rmcp::ErrorData::internal_error(message, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_has_distinct_code() {
        let codes = [
            ToolError::StaleHandle { ref_id: "x".into() }.code(),
            ToolError::NotActionable {
                available_actions: vec![],
                fallback_suggestion: None,
            }
            .code(),
            ToolError::TargetNotFound { closest_match: None }.code(),
            ToolError::PermissionDenied { reason: "x".into() }.code(),
            ToolError::TierRestricted {
                app_class: "x".into(),
                current_tier: "y".into(),
                allowed_actions: vec![],
            }
            .code(),
            ToolError::AppApprovalRequired {
                app_class: "x".into(),
                notification_id: None,
                still_pending: false,
            }
            .code(),
            ToolError::Cancelled {
                cancelled_at_stage: "x".into(),
            }
            .code(),
            ToolError::LockUnavailable {
                holder_pid: 1,
                lock_path: "/x".into(),
            }
            .code(),
            ToolError::Timeout {
                closest_match: None,
                current_windows: vec![],
            }
            .code(),
            ToolError::InputDispatchFailed {
                detail: "x".into(),
                libei_version: None,
                kwin_version: None,
            }
            .code(),
            ToolError::ExternalCommandFailed { stderr: "x".into() }.code(),
            ToolError::BusDisconnected { detail: "x".into() }.code(),
            ToolError::NotImplemented.code(),
        ];
        let unique: std::collections::HashSet<_> = codes.iter().copied().collect();
        assert_eq!(
            unique.len(),
            codes.len(),
            "duplicate codes detected: {codes:?}"
        );
    }

    #[test]
    fn codes_match_spec_section_8_table() {
        assert_eq!(ToolError::NotImplemented.code(), "not_implemented");
        assert_eq!(
            ToolError::LockUnavailable {
                holder_pid: 0,
                lock_path: "/x".into()
            }
            .code(),
            "lock_unavailable"
        );
        assert_eq!(
            ToolError::Cancelled {
                cancelled_at_stage: "before_dispatch".into()
            }
            .code(),
            "cancelled"
        );
        assert_eq!(
            ToolError::AppApprovalRequired {
                app_class: "konsole".into(),
                notification_id: None,
                still_pending: true,
            }
            .code(),
            "app_approval_required"
        );
    }

    #[test]
    fn serializes_with_code_tag() {
        let e = ToolError::NotImplemented;
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "not_implemented");
    }

    #[test]
    fn converts_to_rmcp_error_data_internal() {
        let e = ToolError::NotImplemented;
        let ed: rmcp::ErrorData = e.into();
        assert_eq!(ed.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(ed.message.contains("not implemented"));
    }
}
