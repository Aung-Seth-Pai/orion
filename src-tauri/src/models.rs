use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    /// Unix seconds of the last "Total time" reset; 0 means never reset.
    /// Defaulted on deserialize so backups written before schema version 1
    /// still import.
    #[serde(default)]
    pub timer_reset_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWorkspace {
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: String,
    pub workspace_id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub title: String,
    pub target_path: String,
    pub preferred_app: String,
    pub profile_name: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewResource {
    pub workspace_id: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub title: String,
    pub target_path: String,
    #[serde(default = "default_preferred_app")]
    pub preferred_app: String,
    pub profile_name: Option<String>,
}

fn default_preferred_app() -> String {
    "default".to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePatch {
    pub title: Option<String>,
    pub target_path: Option<String>,
    #[serde(rename = "type")]
    pub resource_type: Option<String>,
    pub preferred_app: Option<String>,
    pub profile_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationScript {
    pub id: String,
    pub workspace_id: Option<String>,
    pub title: String,
    pub script_type: String,
    pub script_content: String,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewScript {
    pub workspace_id: Option<String>,
    pub title: String,
    pub script_type: String,
    pub script_content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptPatch {
    pub title: Option<String>,
    pub script_type: Option<String>,
    pub script_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerLog {
    pub id: String,
    pub workspace_id: String,
    pub duration_seconds: i64,
    pub session_type: String,
    pub started_at: String,
    pub ended_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTimerLog {
    pub workspace_id: String,
    pub duration_seconds: i64,
    #[serde(default = "default_session_type")]
    pub session_type: String,
}

fn default_session_type() -> String {
    "stopwatch".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub item_type: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPayload {
    pub version: i32,
    pub exported_at: String,
    pub workspaces: Vec<Workspace>,
    pub resources: Vec<Resource>,
    pub automation_scripts: Vec<AutomationScript>,
    pub timer_logs: Vec<TimerLog>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub workspace_env_vars: Vec<EnvVar>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub is_completed: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub workspace_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub id: String,
    pub workspace_id: String,
    pub env_key: String,
    pub env_value: String,
}
