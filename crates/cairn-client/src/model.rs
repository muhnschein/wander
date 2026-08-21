use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Status {
    pub version: String,
    #[serde(default)]
    pub uptime_seconds: u64,
    #[serde(default)]
    pub listener: String,
    #[serde(default)]
    pub archives: u32,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub sandbox: SandboxStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SandboxStatus {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub layers: Vec<SandboxLayer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SandboxLayer {
    pub name: String,
    pub state: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchiveSummary {
    pub uuid: String,
    pub title: String,
    #[serde(default)]
    pub entry_count: u64,
    #[serde(default)]
    pub cluster_count: Option<u64>,
    #[serde(default)]
    pub main_page: Option<String>,
    #[serde(default)]
    pub format_version: Option<String>,
    #[serde(default)]
    pub content_namespace: Option<String>,
    #[serde(default)]
    pub suggest: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchiveDetail {
    #[serde(flatten)]
    pub summary: ArchiveSummary,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub binary_metadata: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Suggestion {
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ArchivesResponse {
    pub(crate) archives: Vec<ArchiveSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct SuggestionsResponse {
    pub(crate) suggestions: Vec<Suggestion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct RandomResponse {
    pub(crate) path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ErrorBody {
    #[serde(default)]
    pub(crate) code: String,
    #[serde(default)]
    pub(crate) message: String,
}
