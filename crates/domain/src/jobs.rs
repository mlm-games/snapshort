//! Job types (persisted background work)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    AnalyzeAsset,
    GenerateProxy,
    ExportTimeline,
}

impl JobKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobKind::AnalyzeAsset => "analyze_asset",
            JobKind::GenerateProxy => "generate_proxy",
            JobKind::ExportTimeline => "export_timeline",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "analyze_asset" => JobKind::AnalyzeAsset,
            "generate_proxy" => JobKind::GenerateProxy,
            "export_timeline" => JobKind::ExportTimeline,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "canceled" => JobStatus::Canceled,
            _ => return None,
        })
    }
}
