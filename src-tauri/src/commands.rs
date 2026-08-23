use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::types::{ToSql, ToSqlOutput};
use rusqlite::{params, params_from_iter};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tokio::process::Command;
use tokio::task::spawn_blocking;

use crate::db::Db;
use crate::models::{
    AutomationScript, BackupPayload, EnvVar, NewResource, NewScript, NewTask, NewTimerLog,
    NewWorkspace, Resource, ResourcePatch, ScriptPatch, SearchResult, Task, TimerLog,
    Workspace, WorkspacePatch,
};

enum SqlValue {
    Text(String),
    Int(i64),
    Null,
}

impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            SqlValue::Text(s) => s.to_sql(),
            SqlValue::Int(i) => i.to_sql(),
            SqlValue::Null => Option::<String>::None.to_sql(),
        }
    }
}

const COLUMNS: &str = "id, name, color, icon, sort_order, created_at";

const RESOURCE_COLUMNS: &str =
    "id, workspace_id, type, title, target_path, preferred_app, profile_name, sort_order, created_at";

const VALID_RESOURCE_TYPES: &[&str] = &["link", "folder"];
const VALID_PREFERRED_APPS: &[&str] = &["default", "chrome", "edge", "firefox"];

const SCRIPT_COLUMNS: &str =
    "id, workspace_id, title, script_type, script_content, sort_order, created_at";
const VALID_SCRIPT_TYPES: &[&str] = &["powershell", "cmd", "python", "node", "wsl_bash"];
const VALID_SESSION_TYPES: &[&str] = &["stopwatch", "pomodoro"];

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_SCRIPT_BYTES: usize = 131_072;
const MAX_OUTPUT_CHARS: usize = 65_536;
const SCRIPT_TIMEOUT_SECS: u64 = 120;

fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        icon: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn lock_db(
    db: &Arc<Mutex<rusqlite::Connection>>,
) -> Result<MutexGuard<'_, rusqlite::Connection>, String> {
    db.lock().map_err(|_| "database mutex poisoned".to_string())
}

#[tauri::command]
pub async fn get_workspaces(db: State<'_, Db>) -> Result<Vec<Workspace>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let mut stmt = guard
            .prepare(&format!(
                "SELECT {COLUMNS} FROM workspaces ORDER BY sort_order ASC, name COLLATE NOCASE ASC"
            ))
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map([], row_to_workspace)
            .map_err(|e| format!("failed to query workspaces: {e}"))?;

        let mut workspaces = Vec::new();
        for row in rows {
            workspaces.push(row.map_err(|e| format!("failed to read workspace row: {e}"))?);
        }
        Ok(workspaces)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn create_workspace(db: State<'_, Db>, input: NewWorkspace) -> Result<Workspace, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Workspace name cannot be empty".into());
    }
    if name.len() > 100 {
        return Err("Workspace name is too long (max 100 characters)".into());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO workspaces (id, name, color, icon, sort_order)
                 VALUES (?1, ?2, ?3, ?4, COALESCE((SELECT MAX(sort_order) FROM workspaces), 0) + 1)",
                params![id, name, input.color, input.icon],
            )
            .map_err(|e| format!("failed to insert workspace: {e}"))?;

        let mut stmt = guard
            .prepare(&format!(
                "SELECT {COLUMNS} FROM workspaces WHERE id = ?1"
            ))
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_workspace)
            .map_err(|e| format!("failed to read back workspace: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn update_workspace(
    db: State<'_, Db>,
    id: String,
    patch: WorkspacePatch,
) -> Result<Workspace, String> {
    if let Some(name) = &patch.name {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("Workspace name cannot be empty".into());
        }
        if trimmed.len() > 100 {
            return Err("Workspace name is too long (max 100 characters)".into());
        }
    }

    let mut assignments: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(name) = &patch.name {
        assignments.push("name = ?");
        values.push(SqlValue::Text(name.trim().to_string()));
    }
    if let Some(color) = &patch.color {
        assignments.push("color = ?");
        values.push(SqlValue::Text(color.clone()));
    }
    if let Some(icon) = &patch.icon {
        assignments.push("icon = ?");
        values.push(SqlValue::Text(icon.clone()));
    }
    if let Some(sort_order) = patch.sort_order {
        assignments.push("sort_order = ?");
        values.push(SqlValue::Int(sort_order));
    }

    let has_changes = !assignments.is_empty();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        if has_changes {
            let sql = format!(
                "UPDATE workspaces SET {} WHERE id = ?",
                assignments.join(", ")
            );
            values.push(SqlValue::Text(id.clone()));

            let rows = guard
                .execute(sql.as_str(), params_from_iter(values))
                .map_err(|e| format!("failed to update workspace: {e}"))?;

            if rows == 0 {
                return Err(format!("Workspace '{id}' not found"));
            }
        }

        let mut stmt = guard
            .prepare(&format!("SELECT {COLUMNS} FROM workspaces WHERE id = ?1"))
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_workspace)
            .map_err(|e| format!("workspace not found: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn delete_workspace(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete workspace: {e}"))?;
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

fn row_to_resource(row: &rusqlite::Row) -> rusqlite::Result<Resource> {
    Ok(Resource {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        resource_type: row.get(2)?,
        title: row.get(3)?,
        target_path: row.get(4)?,
        preferred_app: row.get(5)?,
        profile_name: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn validate_resource_fields(
    resource_type: &str,
    title: &str,
    target_path: &str,
    preferred_app: &str,
) -> Result<(), String> {
    if !VALID_RESOURCE_TYPES.contains(&resource_type) {
        return Err(format!(
            "Invalid resource type '{resource_type}' (expected 'link' or 'folder')"
        ));
    }
    if !VALID_PREFERRED_APPS.contains(&preferred_app) {
        return Err(format!(
            "Invalid preferred app '{preferred_app}' (expected default, chrome, edge or firefox)"
        ));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("Resource title cannot be empty".into());
    }
    if title.len() > 120 {
        return Err("Resource title is too long (max 120 characters)".into());
    }
    let target = target_path.trim();
    if target.is_empty() {
        return Err("Target path cannot be empty".into());
    }
    if target.len() > 2048 {
        return Err("Target path is too long (max 2048 characters)".into());
    }
    if target.contains('"') || target.contains('\0') {
        return Err("Target path contains invalid characters".into());
    }
    if resource_type == "link" && !(target.starts_with("http://") || target.starts_with("https://"))
    {
        return Err("Links must start with http:// or https://".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_resources(
    db: State<'_, Db>,
    workspace_id: String,
) -> Result<Vec<Resource>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let mut stmt = guard
            .prepare(&format!(
                "SELECT {RESOURCE_COLUMNS} FROM resources WHERE workspace_id = ?1
                 ORDER BY sort_order ASC, created_at ASC, rowid ASC"
            ))
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map(params![workspace_id], row_to_resource)
            .map_err(|e| format!("failed to query resources: {e}"))?;

        let mut resources = Vec::new();
        for row in rows {
            resources.push(row.map_err(|e| format!("failed to read resource row: {e}"))?);
        }
        Ok(resources)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn create_resource(db: State<'_, Db>, input: NewResource) -> Result<Resource, String> {
    let resource_type = input.resource_type.trim().to_lowercase();
    let preferred_app = input.preferred_app.trim().to_lowercase();

    validate_resource_fields(
        &resource_type,
        &input.title,
        &input.target_path,
        &preferred_app,
    )?;

    let id = uuid::Uuid::new_v4().to_string();
    let profile_name = input
        .profile_name
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from);
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO resources (
                    id, workspace_id, type, title, target_path,
                    preferred_app, profile_name, sort_order
                )
                VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7,
                    COALESCE(
                        (SELECT MAX(sort_order) FROM resources WHERE workspace_id = ?8), 0
                    ) + 1
                )",
                params![
                    id,
                    input.workspace_id,
                    resource_type,
                    input.title.trim(),
                    input.target_path.trim(),
                    preferred_app,
                    profile_name,
                    input.workspace_id
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("FOREIGN KEY constraint failed") {
                    "Workspace does not exist".to_string()
                } else if e.to_string().contains("CHECK constraint failed") {
                    "Invalid resource type".to_string()
                } else {
                    format!("failed to insert resource: {e}")
                }
            })?;

        let mut stmt = guard
            .prepare(&format!(
                "SELECT {RESOURCE_COLUMNS} FROM resources WHERE id = ?1"
            ))
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_resource)
            .map_err(|e| format!("failed to read back resource: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn update_resource(
    db: State<'_, Db>,
    id: String,
    patch: ResourcePatch,
) -> Result<Resource, String> {
    let has_changes = patch.title.is_some()
        || patch.target_path.is_some()
        || patch.resource_type.is_some()
        || patch.preferred_app.is_some()
        || patch.profile_name.is_some();

    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        let current: Resource = {
            let mut stmt = guard
                .prepare(&format!(
                    "SELECT {RESOURCE_COLUMNS} FROM resources WHERE id = ?1"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            stmt.query_row(params![id], row_to_resource)
                .map_err(|_| format!("Resource '{id}' not found"))?
        };

        if has_changes {
            let merged_type = patch
                .resource_type
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
                .unwrap_or(current.resource_type.clone());
            let merged_title = patch.title.as_deref().unwrap_or(&current.title);
            let merged_target = patch.target_path.as_deref().unwrap_or(&current.target_path);
            let merged_app = patch
                .preferred_app
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
                .unwrap_or(current.preferred_app.clone());

            validate_resource_fields(&merged_type, merged_title, merged_target, &merged_app)?;

            let mut assignments: Vec<&str> = Vec::new();
            let mut values: Vec<SqlValue> = Vec::new();

            if let Some(title) = &patch.title {
                assignments.push("title = ?");
                values.push(SqlValue::Text(title.trim().to_string()));
            }
            if let Some(target) = &patch.target_path {
                assignments.push("target_path = ?");
                values.push(SqlValue::Text(target.trim().to_string()));
            }
            if let Some(t) = &patch.resource_type {
                assignments.push("type = ?");
                values.push(SqlValue::Text(t.trim().to_lowercase()));
            }
            if let Some(app) = &patch.preferred_app {
                assignments.push("preferred_app = ?");
                values.push(SqlValue::Text(app.trim().to_lowercase()));
            }
            if let Some(profile) = &patch.profile_name {
                assignments.push("profile_name = ?");
                let trimmed = profile.trim();
                values.push(if trimmed.is_empty() {
                    SqlValue::Null
                } else {
                    SqlValue::Text(trimmed.to_string())
                });
            }

            let sql = format!(
                "UPDATE resources SET {} WHERE id = ?",
                assignments.join(", ")
            );
            values.push(SqlValue::Text(id.clone()));

            guard
                .execute(sql.as_str(), params_from_iter(values))
                .map_err(|e| format!("failed to update resource: {e}"))?;
        }

        let mut stmt = guard
            .prepare(&format!(
                "SELECT {RESOURCE_COLUMNS} FROM resources WHERE id = ?1"
            ))
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_resource)
            .map_err(|e| format!("resource not found: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn delete_resource(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM resources WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete resource: {e}"))?;
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

struct LaunchTarget {
    resource_type: String,
    target_path: String,
    preferred_app: String,
    profile_name: Option<String>,
    default_ide: Option<String>,
}

fn resolve_browser_exe(candidates: &[PathBuf], display_name: &str) -> Result<PathBuf, String> {
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "{display_name} was not found on this system. Install it or use the system default browser."
    ))
}

fn chrome_exe() -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
            PathBuf::from("C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe"),
            PathBuf::from(local).join("Google\\Chrome\\Application\\chrome.exe"),
        ],
        "Chrome",
    )
}

fn edge_exe() -> Result<PathBuf, String> {
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"),
            PathBuf::from("C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe"),
        ],
        "Microsoft Edge",
    )
}

fn firefox_exe() -> Result<PathBuf, String> {
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files\\Mozilla Firefox\\firefox.exe"),
            PathBuf::from("C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe"),
        ],
        "Firefox",
    )
}

/// Spawns the user-configured IDE command against a path. A direct spawn is
/// attempted first (covers absolute .exe paths and PATH executables); if the
/// OS cannot run it directly we fall back to `cmd /C`, which resolves
/// PATHEXT shims like VS Code's `code.cmd`.
fn spawn_ide(command_str: &str, target_path: &str) -> Result<(), String> {
    let trimmed = command_str.trim();
    if trimmed.is_empty() {
        return Err("Default IDE command is empty".into());
    }
    if trimmed.contains('"') || trimmed.contains('\0') || trimmed.contains('\n') {
        return Err("Default IDE command contains invalid characters".into());
    }

    match Command::new(trimmed).arg(target_path).spawn() {
        Ok(_) => Ok(()),
        Err(direct_err) => Command::new("cmd")
            .args(["/C", trimmed, target_path])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|fallback_err| {
                format!(
                    "failed to launch IDE '{trimmed}' (direct: {direct_err}; via cmd: {fallback_err})"
                )
            }),
    }
}

#[tauri::command]
pub async fn launch_resource(
    db: State<'_, Db>,
    id: String,
    action_override: Option<String>,
) -> Result<String, String> {
    let conn = db.0.clone();
    let log_id = id.clone();

    let target: LaunchTarget = spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .query_row(
                "SELECT r.type, r.target_path, r.preferred_app, r.profile_name,
                        (SELECT value FROM app_settings WHERE key = 'default_ide')
                 FROM resources r WHERE r.id = ?1",
                params![id],
                |row| {
                    Ok(LaunchTarget {
                        resource_type: row.get(0)?,
                        target_path: row.get(1)?,
                        preferred_app: row.get(2)?,
                        profile_name: row.get(3)?,
                        default_ide: row.get(4)?,
                    })
                },
            )
            .map_err(|e| format!("resource not found: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))??;

    log::info!(
        "Launching resource '{log_id}' (type={}, override={:?})",
        target.resource_type,
        action_override
    );

    match target.resource_type.as_str() {
        "folder" => {
            let path = PathBuf::from(&target.target_path);
            if !path.is_dir() {
                log::error!("Launch failed: folder does not exist: {}", target.target_path);
                return Err(format!(
                    "Folder does not exist: {}",
                    target.target_path
                ));
            }

            match action_override.as_deref() {
                Some("ide") => {
                    let ide_command = target
                        .default_ide
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("code");
                    if let Err(e) = spawn_ide(ide_command, &target.target_path) {
                        log::error!("IDE launch failed for '{log_id}': {e}");
                        return Err(format!("failed to open folder in IDE: {e}"));
                    }
                    log::info!("Resource '{log_id}' opened in IDE '{ide_command}'");
                    Ok(format!("Opened in {ide_command}"))
                }
                Some("terminal") => {
                    if let Err(e) = Command::new("wt")
                        .args(["-d", &target.target_path])
                        .spawn()
                    {
                        let msg = format!(
                            "failed to open terminal here (is Windows Terminal installed?): {e}"
                        );
                        log::error!("Terminal launch failed for '{log_id}': {msg}");
                        return Err(msg);
                    }
                    log::info!("Resource '{log_id}' opened in Windows Terminal");
                    Ok("Opened in terminal".to_string())
                }
                _ => {
                    if let Err(e) = Command::new("explorer.exe")
                        .arg(&target.target_path)
                        .spawn()
                    {
                        log::error!("Explorer launch failed for '{log_id}': {e}");
                        return Err(format!("failed to launch explorer: {e}"));
                    }
                    log::info!("Resource '{log_id}' opened in Explorer");
                    Ok("Opened folder".to_string())
                }
            }
        }
        "link" => {
            let url = target.target_path.trim();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err("Refusing to launch: links must be http(s) URLs".into());
            }
            if url.contains('"') || url.contains('\0') {
                return Err("Refusing to launch: URL contains invalid characters".into());
            }

            match target.preferred_app.as_str() {
                "default" => {
                    Command::new("cmd")
                        .args(["/C", "start", "", url])
                        .creation_flags(0x08000000)
                        .spawn()
                        .map_err(|e| format!("failed to open URL in default browser: {e}"))?;
                }
                "chrome" => {
                    let exe = chrome_exe()?;
                    let mut cmd = Command::new(exe);
                    if let Some(profile) = target.profile_name.as_deref() {
                        cmd.arg(format!("--profile-directory={profile}"));
                    }
                    cmd.arg(url)
                        .spawn()
                        .map_err(|e| format!("failed to launch Chrome: {e}"))?;
                }
                "edge" => {
                    let exe = edge_exe()?;
                    let mut cmd = Command::new(exe);
                    if let Some(profile) = target.profile_name.as_deref() {
                        cmd.arg(format!("--profile-directory={profile}"));
                    }
                    cmd.arg(url)
                        .spawn()
                        .map_err(|e| format!("failed to launch Edge: {e}"))?;
                }
                "firefox" => {
                    let exe = firefox_exe()?;
                    let mut cmd = Command::new(exe);
                    if let Some(profile) = target.profile_name.as_deref() {
                        cmd.args(["-P", profile]);
                    }
                    cmd.arg(url)
                        .spawn()
                        .map_err(|e| format!("failed to launch Firefox: {e}"))?;
                }
                other => {
                    log::error!(
                        "Launch failed for '{log_id}': unknown preferred app '{other}'"
                    );
                    return Err(format!("Unknown preferred app '{other}'"));
                }
            }
            log::info!("Resource '{log_id}' launched in {}", target.preferred_app);
            Ok("Launched".to_string())
        }
        other => {
            log::error!("Launch failed for '{log_id}': unknown resource type '{other}'");
            Err(format!("Unknown resource type '{other}'"))
        }
    }
}

fn row_to_script(row: &rusqlite::Row) -> rusqlite::Result<AutomationScript> {
    Ok(AutomationScript {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        title: row.get(2)?,
        script_type: row.get(3)?,
        script_content: row.get(4)?,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn validate_script_fields(
    script_type: &str,
    title: &str,
    script_content: &str,
) -> Result<(), String> {
    if !VALID_SCRIPT_TYPES.contains(&script_type) {
        return Err(format!(
            "Invalid script type '{script_type}' (expected one of: powershell, cmd, python, node, wsl_bash)"
        ));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err("Script title cannot be empty".into());
    }
    if title.len() > 120 {
        return Err("Script title is too long (max 120 characters)".into());
    }
    let content = script_content.trim();
    if content.is_empty() {
        return Err("Script content cannot be empty".into());
    }
    if content.len() > MAX_SCRIPT_BYTES {
        return Err("Script is too large (max 128 KB)".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_scripts(
    db: State<'_, Db>,
    workspace_id: Option<String>,
) -> Result<Vec<AutomationScript>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let (sql, param): (&str, Vec<SqlValue>) = match workspace_id.as_deref() {
            Some(ws_id) => (
                &format!("SELECT {SCRIPT_COLUMNS} FROM automation_scripts
                          WHERE workspace_id = ?1 ORDER BY sort_order ASC, created_at ASC, rowid ASC"),
                vec![SqlValue::Text(ws_id.to_string())],
            ),
            None => (
                "SELECT id, workspace_id, title, script_type, script_content, sort_order, created_at
                 FROM automation_scripts WHERE workspace_id IS NULL
                 ORDER BY sort_order ASC, created_at ASC, rowid ASC",
                Vec::new(),
            ),
        };

        let mut stmt = guard
            .prepare(sql)
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        let rows = stmt
            .query_map(params_from_iter(param), row_to_script)
            .map_err(|e| format!("failed to query scripts: {e}"))?;

        let mut scripts = Vec::new();
        for row in rows {
            scripts.push(row.map_err(|e| format!("failed to read script row: {e}"))?);
        }
        Ok(scripts)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn create_script(
    db: State<'_, Db>,
    input: NewScript,
) -> Result<AutomationScript, String> {
    let script_type = input.script_type.trim().to_lowercase();
    validate_script_fields(&script_type, &input.title, &input.script_content)?;

    let workspace_id = input
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(String::from);
    let id = uuid::Uuid::new_v4().to_string();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO automation_scripts (
                    id, workspace_id, title, script_type, script_content, sort_order
                )
                VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    COALESCE((
                        SELECT MAX(sort_order) FROM automation_scripts
                        WHERE workspace_id IS ?6
                    ), 0) + 1
                )",
                params![
                    id,
                    workspace_id,
                    input.title.trim(),
                    script_type,
                    input.script_content,
                    workspace_id
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("FOREIGN KEY constraint failed") {
                    "Workspace does not exist".to_string()
                } else if e.to_string().contains("CHECK constraint failed") {
                    "Invalid script type".to_string()
                } else {
                    format!("failed to insert script: {e}")
                }
            })?;

        let mut stmt = guard
            .prepare("SELECT id, workspace_id, title, script_type, script_content, sort_order, created_at FROM automation_scripts WHERE id = ?1")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_script)
            .map_err(|e| format!("failed to read back script: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn update_script(
    db: State<'_, Db>,
    id: String,
    patch: ScriptPatch,
) -> Result<AutomationScript, String> {
    if let Some(script_type) = &patch.script_type {
        let t = script_type.trim().to_lowercase();
        if !VALID_SCRIPT_TYPES.contains(&t.as_str()) {
            return Err(format!("Invalid script type '{t}'"));
        }
    }

    let mut assignments: Vec<&str> = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(title) = &patch.title {
        assignments.push("title = ?");
        values.push(SqlValue::Text(title.trim().to_string()));
    }
    if let Some(script_type) = &patch.script_type {
        assignments.push("script_type = ?");
        values.push(SqlValue::Text(script_type.trim().to_lowercase()));
    }
    if let Some(content) = &patch.script_content {
        assignments.push("script_content = ?");
        values.push(SqlValue::Text(content.clone()));
    }

    let has_changes = !assignments.is_empty();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        if has_changes {
            if let Some(title) = &patch.title {
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    return Err("Script title cannot be empty".into());
                }
                if trimmed.len() > 120 {
                    return Err("Script title is too long (max 120 characters)".into());
                }
            }
            if let Some(content) = &patch.script_content {
                if content.trim().is_empty() {
                    return Err("Script content cannot be empty".into());
                }
                if content.len() > MAX_SCRIPT_BYTES {
                    return Err("Script is too large (max 128 KB)".into());
                }
            }

            let sql = format!(
                "UPDATE automation_scripts SET {} WHERE id = ?",
                assignments.join(", ")
            );
            values.push(SqlValue::Text(id.clone()));

            let rows = guard
                .execute(sql.as_str(), params_from_iter(values))
                .map_err(|e| format!("failed to update script: {e}"))?;
            if rows == 0 {
                return Err(format!("Script '{id}' not found"));
            }
        }

        let mut stmt = guard
            .prepare("SELECT id, workspace_id, title, script_type, script_content, sort_order, created_at FROM automation_scripts WHERE id = ?1")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_script)
            .map_err(|e| format!("script not found: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn delete_script(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM automation_scripts WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete script: {e}"))?;
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

struct FetchedScript {
    script_type: String,
    script_content: String,
    env_vars: HashMap<String, String>,
    python_path: Option<String>,
    node_path: Option<String>,
}

fn windows_to_wsl_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        format!("/mnt/{drive}{}", &text[2..])
    } else {
        text
    }
}

fn build_interpreter_command(
    script_type: &str,
    temp_path: &PathBuf,
    custom_python: Option<&str>,
    custom_node: Option<&str>,
) -> Command {
    let mut command = match script_type {
        "powershell" => {
            let mut c = Command::new("powershell.exe");
            c.args(["-ExecutionPolicy", "Bypass", "-File"])
                .arg(temp_path);
            c
        }
        "cmd" => {
            let mut c = Command::new("cmd.exe");
            c.arg("/C").arg(temp_path);
            c
        }
        "python" => {
            let python_exe = custom_python.unwrap_or("python.exe");
            log::info!("Using Python interpreter: {python_exe}");
            let mut c = Command::new(python_exe);
            c.arg(temp_path);
            c
        }
        "node" => {
            let node_exe = custom_node.unwrap_or("node.exe");
            log::info!("Using Node.js interpreter: {node_exe}");
            let mut c = Command::new(node_exe);
            c.arg(temp_path);
            c
        }
        "wsl_bash" => {
            let wsl_path = windows_to_wsl_path(temp_path);
            let mut c = Command::new("wsl.exe");
            c.args(["--exec", "bash", &wsl_path]);
            c
        }
        other => unreachable!("script type '{other}' should have been validated"),
    };
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

async fn run_temp_script(fetched: FetchedScript, temp_path: PathBuf) -> Result<String, String> {
    let write_result = tokio::fs::write(&temp_path, fetched.script_content.as_bytes()).await;
    if let Err(e) = write_result {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(format!("failed to write temporary script file: {e}"));
    }

    // wsl.exe does not forward custom environment variables into the Linux
    // guest unless they are listed in WSLENV. The /u flag marks each variable
    // as "pass from Windows into WSL only".
    let wslenv = if fetched.script_type == "wsl_bash" && !fetched.env_vars.is_empty() {
        let ours = fetched
            .env_vars
            .keys()
            .map(|key| format!("{key}/u"))
            .collect::<Vec<_>>()
            .join(":");
        match std::env::var("WSLENV") {
            Ok(existing) if !existing.trim().is_empty() => format!("{ours}:{existing}"),
            _ => ours,
        }
    } else {
        String::new()
    };

    let mut command = build_interpreter_command(
        &fetched.script_type,
        &temp_path,
        fetched.python_path.as_deref(),
        fetched.node_path.as_deref(),
    );
    command
        .envs(&fetched.env_vars)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Never touch a pre-existing WSLENV unless we actually have vars to pass.
    if !wslenv.is_empty() {
        command.env("WSLENV", wslenv);
    }

    let child = command
        .spawn()
        .map_err(|e| {
            let msg = format!(
                "failed to start interpreter for '{}': {e}",
                fetched.script_type
            );
            log::error!("{msg}");
            msg
        })?;

    let wait = child.wait_with_output();
    let output = match tokio::time::timeout(Duration::from_secs(SCRIPT_TIMEOUT_SECS), wait).await {
        Ok(result) => result.map_err(|e| format!("script execution failed: {e}"))?,
        Err(_) => {
            return Err(format!(
                "script timed out after {SCRIPT_TIMEOUT_SECS}s and was terminated"
            ));
        }
    };

    let mut combined = String::with_capacity(output.stdout.len() + output.stderr.len() + 64);
    append_capped(&mut combined, &output.stdout);
    if !output.stderr.is_empty() {
        combined.push('\n');
        append_capped(&mut combined, &output.stderr);
    }

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        combined.push_str(&format!("\n[process exited with code {code}]"));
    } else if combined.trim().is_empty() {
        combined.push_str("[no output]");
    }

    Ok(combined)
}

fn append_capped(target: &mut String, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_end();
    if target.len() >= MAX_OUTPUT_CHARS {
        return;
    }
    if target.len() + trimmed.len() <= MAX_OUTPUT_CHARS {
        target.push_str(trimmed);
        return;
    }
    let budget = MAX_OUTPUT_CHARS - target.len();
    let mut end = 0;
    for (i, ch) in trimmed.char_indices() {
        if i + ch.len_utf8() > budget {
            break;
        }
        end = i + ch.len_utf8();
    }
    target.push_str(&trimmed[..end]);
    target.push_str("\n…[output truncated]");
}

#[tauri::command]
pub async fn execute_script(db: State<'_, Db>, id: String) -> Result<String, String> {
    let conn = db.0.clone();
    let log_id = id.clone();

    let fetched: FetchedScript = spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let (script_type, script_content, workspace_id): (String, String, Option<String>) =
            guard
                .query_row(
                    "SELECT script_type, script_content, workspace_id
                     FROM automation_scripts WHERE id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| format!("Script '{id}' not found"))?;

        let env_vars: HashMap<String, String> = match &workspace_id {
            Some(ws_id) => {
                let mut stmt = guard
                    .prepare(
                        "SELECT env_key, env_value FROM workspace_env_vars WHERE workspace_id = ?1",
                    )
                    .map_err(|e| format!("failed to prepare query: {e}"))?;
                let rows = stmt
                    .query_map(params![ws_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|e| format!("failed to query environment variables: {e}"))?;
                let mut map = HashMap::new();
                for row in rows {
                    let (key, value) =
                        row.map_err(|e| format!("failed to read env var row: {e}"))?;
                    map.insert(key, value);
                }
                map
            }
            None => HashMap::new(),
        };

        // Custom interpreter overrides from app_settings (empty = system default).
        let read_setting = |key: &str| -> Option<String> {
            guard
                .query_row(
                    "SELECT value FROM app_settings WHERE key = ?1",
                    params![key],
                    |row| row.get::<_, String>(0),
                )
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let python_path = read_setting("python_path");
        let node_path = read_setting("node_path");

        Ok::<FetchedScript, String>(FetchedScript {
            script_type,
            script_content,
            env_vars,
            python_path,
            node_path,
        })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))??;

    if !VALID_SCRIPT_TYPES.contains(&fetched.script_type.as_str()) {
        log::error!("Script execution rejected: unknown type '{}'", fetched.script_type);
        return Err(format!("Unknown script type '{}'", fetched.script_type));
    }

    let started = std::time::Instant::now();
    let extension = match fetched.script_type.as_str() {
        "powershell" => "ps1",
        "cmd" => "bat",
        "python" => "py",
        "node" => "js",
        _ => "sh",
    };

    let temp_path =
        std::env::temp_dir().join(format!("orion-cmd-{}.{}", uuid::Uuid::new_v4(), extension));

    log::info!(
        "Executing {} script '{log_id}' ({} env vars injected)",
        fetched.script_type,
        fetched.env_vars.len()
    );

    // Always remove the temporary file, whether execution succeeded or failed.
    let result = run_temp_script(fetched, temp_path.clone()).await;
    let _ = tokio::fs::remove_file(&temp_path).await;

    match &result {
        Ok(output) => log::info!(
            "Script '{log_id}' finished in {:.1}s ({} chars of output)",
            started.elapsed().as_secs_f32(),
            output.len()
        ),
        Err(e) => log::error!(
            "Script '{log_id}' failed after {:.1}s: {e}",
            started.elapsed().as_secs_f32()
        ),
    }
    result
}

#[tauri::command]
pub async fn log_timer_session(db: State<'_, Db>, input: NewTimerLog) -> Result<TimerLog, String> {
    if input.duration_seconds <= 0 {
        return Err("Timer duration must be positive".into());
    }
    if input.duration_seconds > 86_400 {
        return Err("Timer duration exceeds 24 hours".into());
    }
    let session_type = input.session_type.trim().to_lowercase();
    if !VALID_SESSION_TYPES.contains(&session_type.as_str()) {
        return Err(format!(
            "Invalid session type '{session_type}' (expected 'stopwatch' or 'pomodoro')"
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO timer_logs (id, workspace_id, duration_seconds, session_type, started_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now', '-' || ?5 || ' seconds'))",
                params![
                    id,
                    input.workspace_id,
                    input.duration_seconds,
                    session_type,
                    input.duration_seconds
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("FOREIGN KEY constraint failed") {
                    "Workspace does not exist".to_string()
                } else {
                    format!("failed to insert timer log: {e}")
                }
            })?;

        let mut stmt = guard
            .prepare(
                "SELECT id, workspace_id, duration_seconds, session_type, started_at, ended_at
                 FROM timer_logs WHERE id = ?1",
            )
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], |row| {
            Ok(TimerLog {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                duration_seconds: row.get(2)?,
                session_type: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("failed to read back timer log: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

const MAX_QUERY_CHARS: usize = 100;
const SEARCH_LIMIT: i64 = 25;

fn build_like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

#[tauri::command]
pub async fn search_all(db: State<'_, Db>, query: String) -> Result<Vec<SearchResult>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.len() > MAX_QUERY_CHARS {
        return Err("Search query is too long".into());
    }

    let pattern = build_like_pattern(trimmed);
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let sql = "
            SELECT kind, id, title, subtitle, action FROM (
                SELECT 'workspace' AS kind, id, name AS title,
                       'Workspace' AS subtitle, 'open_workspace' AS action
                FROM workspaces WHERE name LIKE ?1 ESCAPE '\\'
                UNION ALL
                SELECT 'link', id, title, target_path, 'launch_resource'
                FROM resources
                WHERE type = 'link' AND (title LIKE ?1 ESCAPE '\\' OR target_path LIKE ?1 ESCAPE '\\')
                UNION ALL
                SELECT 'folder', id, title, target_path, 'launch_resource'
                FROM resources
                WHERE type = 'folder' AND (title LIKE ?1 ESCAPE '\\' OR target_path LIKE ?1 ESCAPE '\\')
                UNION ALL
                SELECT 'script', s.id, s.title,
                       CASE WHEN w.name IS NULL THEN 'Global script' ELSE w.name END,
                       'execute_script'
                FROM automation_scripts s
                LEFT JOIN workspaces w ON w.id = s.workspace_id
                WHERE s.title LIKE ?1 ESCAPE '\\'
            )
            ORDER BY title COLLATE NOCASE ASC
            LIMIT ?2";

        let mut stmt = guard
            .prepare(sql)
            .map_err(|e| format!("failed to prepare search query: {e}"))?;

        let rows = stmt
            .query_map(params![pattern, SEARCH_LIMIT], |row| {
                Ok(SearchResult {
                    item_type: row.get(0)?,
                    id: row.get(1)?,
                    title: row.get(2)?,
                    subtitle: row.get(3)?,
                    action: row.get(4)?,
                })
            })
            .map_err(|e| format!("failed to execute search: {e}"))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("failed to read search result: {e}"))?);
        }
        Ok(results)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

const MAX_SETTING_KEY: usize = 128;
const MAX_SETTING_VALUE: usize = 8192;
const MAX_IMPORT_BYTES: usize = 33_554_432;
const BACKUP_VERSION: i32 = 1;

#[tauri::command]
pub async fn get_settings(db: State<'_, Db>) -> Result<HashMap<String, String>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let mut stmt = guard
            .prepare("SELECT key, value FROM app_settings")
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| format!("failed to query settings: {e}"))?;

        let mut settings = HashMap::new();
        for row in rows {
            let (key, value) = row.map_err(|e| format!("failed to read setting row: {e}"))?;
            settings.insert(key, value);
        }
        Ok(settings)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn update_setting(
    app: AppHandle,
    db: State<'_, Db>,
    key: String,
    value: String,
) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Setting key cannot be empty".into());
    }
    if key.len() > MAX_SETTING_KEY {
        return Err(format!(
            "Setting key is too long (max {MAX_SETTING_KEY} characters)"
        ));
    }
    if value.len() > MAX_SETTING_VALUE {
        return Err(format!(
            "Setting value is too long (max {MAX_SETTING_VALUE} characters)"
        ));
    }

    let conn = db.0.clone();
    let key_for_effects = key.clone();
    let value_for_effects = value.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| format!("failed to update setting: {e}"))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))??;

    // Side effects for keys that control native behaviour.
    if key_for_effects == "launch_on_startup" {
        apply_autostart(&app, value_for_effects == "true")?;
    }
    Ok(())
}

fn apply_autostart(app: &AppHandle, enable: bool) -> Result<(), String> {
    let autostart = app.autolaunch();
    let result = if enable {
        autostart.enable()
    } else {
        match autostart.disable() {
            Ok(()) => Ok(()),
            Err(e) => {
                // Disabling while never registered surfaces as OS Error 2
                // ("The system cannot find the file specified") on Windows.
                // The desired end state is already true, so treat it as success.
                let msg = e.to_string();
                let lower = msg.to_lowercase();
                if lower.contains("os error 2") || lower.contains("cannot find the file") {
                    log::info!("Autostart disable skipped (not currently registered): {msg}");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    };
    result.map_err(|e| {
        log::error!("Autostart toggle failed (enable={enable}): {e}");
        format!(
            "failed to {} autostart: {e}",
            if enable { "enable" } else { "disable" }
        )
    })
}

#[tauri::command]
pub async fn toggle_autostart(app: AppHandle, enable: bool) -> Result<(), String> {
    apply_autostart(&app, enable)
}

#[tauri::command]
pub async fn open_log_folder(app: AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("failed to resolve log directory: {e}"))?;

    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create log directory: {e}"))?;

    Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("failed to open log folder: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn export_all_data(db: State<'_, Db>) -> Result<String, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        let exported_at: String = guard
            .query_row("SELECT datetime('now')", [], |row| row.get(0))
            .map_err(|e| format!("failed to read timestamp: {e}"))?;

        let workspaces = {
            let mut stmt = guard
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM workspaces ORDER BY sort_order ASC, name COLLATE NOCASE ASC"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], row_to_workspace)
                .map_err(|e| format!("failed to query workspaces: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read workspace row: {e}"))?
        };

        let resources = {
            let mut stmt = guard
                .prepare(&format!(
                    "SELECT {RESOURCE_COLUMNS} FROM resources ORDER BY sort_order ASC, created_at ASC, rowid ASC"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], row_to_resource)
                .map_err(|e| format!("failed to query resources: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read resource row: {e}"))?
        };

        let scripts = {
            let mut stmt = guard
                .prepare(&format!(
                    "SELECT {SCRIPT_COLUMNS} FROM automation_scripts ORDER BY sort_order ASC, created_at ASC, rowid ASC"
                ))
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], row_to_script)
                .map_err(|e| format!("failed to query scripts: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read script row: {e}"))?
        };

        let timer_logs = {
            let mut stmt = guard
                .prepare(
                    "SELECT id, workspace_id, duration_seconds, session_type, started_at, ended_at
                     FROM timer_logs ORDER BY started_at ASC",
                )
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(TimerLog {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        duration_seconds: row.get(2)?,
                        session_type: row.get(3)?,
                        started_at: row.get(4)?,
                        ended_at: row.get(5)?,
                    })
                })
                .map_err(|e| format!("failed to query timer logs: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read timer log row: {e}"))?
        };

        let tasks = {
            let mut stmt = guard
                .prepare(
                    "SELECT id, workspace_id, title, is_completed, created_at
                     FROM tasks ORDER BY created_at ASC, rowid ASC",
                )
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], row_to_task)
                .map_err(|e| format!("failed to query tasks: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read task row: {e}"))?
        };

        let env_vars = {
            let mut stmt = guard
                .prepare(
                    "SELECT id, workspace_id, env_key, env_value
                     FROM workspace_env_vars ORDER BY workspace_id ASC, env_key COLLATE NOCASE ASC",
                )
                .map_err(|e| format!("failed to prepare query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(EnvVar {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        env_key: row.get(2)?,
                        env_value: row.get(3)?,
                    })
                })
                .map_err(|e| format!("failed to query env vars: {e}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("failed to read env var row: {e}"))?
        };

        let payload = BackupPayload {
            version: BACKUP_VERSION + 1,
            exported_at,
            workspaces,
            resources,
            automation_scripts: scripts,
            timer_logs,
            tasks,
            workspace_env_vars: env_vars,
        };

        serde_json::to_string(&payload)
            .map_err(|e| format!("failed to serialize backup: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

fn insert_workspace(tx: &rusqlite::Transaction, w: &Workspace) -> Result<(), String> {
    tx.execute(
        "INSERT INTO workspaces (id, name, color, icon, sort_order, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![w.id, w.name, w.color, w.icon, w.sort_order, w.created_at],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import workspace '{}': {e}", w.name))
}

fn insert_resource(tx: &rusqlite::Transaction, r: &Resource) -> Result<(), String> {
    tx.execute(
        "INSERT INTO resources (
            id, workspace_id, type, title, target_path,
            preferred_app, profile_name, sort_order, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            r.id,
            r.workspace_id,
            r.resource_type,
            r.title,
            r.target_path,
            r.preferred_app,
            r.profile_name,
            r.sort_order,
            r.created_at
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import resource '{}': {e}", r.title))
}

fn insert_script(tx: &rusqlite::Transaction, s: &AutomationScript) -> Result<(), String> {
    tx.execute(
        "INSERT INTO automation_scripts (
            id, workspace_id, title, script_type, script_content, sort_order, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            s.id,
            s.workspace_id,
            s.title,
            s.script_type,
            s.script_content,
            s.sort_order,
            s.created_at
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import script '{}': {e}", s.title))
}

fn insert_timer_log(tx: &rusqlite::Transaction, t: &TimerLog) -> Result<(), String> {
    tx.execute(
        "INSERT INTO timer_logs (
            id, workspace_id, duration_seconds, session_type, started_at, ended_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            t.id,
            t.workspace_id,
            t.duration_seconds,
            t.session_type,
            t.started_at,
            t.ended_at
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import timer entry: {e}"))
}

fn insert_task(tx: &rusqlite::Transaction, task: &Task) -> Result<(), String> {
    tx.execute(
        "INSERT INTO tasks (id, workspace_id, title, is_completed, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            task.id,
            task.workspace_id,
            task.title,
            task.is_completed as i64,
            task.created_at
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import task '{}': {e}", task.title))
}

fn insert_env_var(tx: &rusqlite::Transaction, var: &EnvVar) -> Result<(), String> {
    tx.execute(
        "INSERT INTO workspace_env_vars (id, workspace_id, env_key, env_value)
         VALUES (?1, ?2, ?3, ?4)",
        params![var.id, var.workspace_id, var.env_key, var.env_value],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to import variable '{}': {e}", var.env_key))
}

#[tauri::command]
pub async fn import_data(db: State<'_, Db>, json_payload: String) -> Result<(), String> {
    if json_payload.len() > MAX_IMPORT_BYTES {
        return Err("Backup file is too large (max 32 MB)".into());
    }

    let conn = db.0.clone();

    spawn_blocking(move || {
        // Parsing happens here on the blocking thread pool so a large file
        // can never stall the async runtime or the UI.
        let payload: BackupPayload = serde_json::from_str(&json_payload)
            .map_err(|e| format!("Invalid backup file: {e}"))?;

        if payload.version != BACKUP_VERSION && payload.version != BACKUP_VERSION + 1 {
            return Err(format!(
                "Unsupported backup version {} (expected {BACKUP_VERSION} or {})",
                payload.version,
                BACKUP_VERSION + 1
            ));
        }

        let known_workspaces: HashSet<&str> =
            payload.workspaces.iter().map(|w| w.id.as_str()).collect();

        for resource in &payload.resources {
            if !known_workspaces.contains(resource.workspace_id.as_str()) {
                return Err(format!(
                    "Backup is inconsistent: resource '{}' references an unknown workspace",
                    resource.title
                ));
            }
        }
        for script in &payload.automation_scripts {
            if let Some(ws_id) = &script.workspace_id {
                if !known_workspaces.contains(ws_id.as_str()) {
                    return Err(format!(
                        "Backup is inconsistent: script '{}' references an unknown workspace",
                        script.title
                    ));
                }
            }
        }
        for timer in &payload.timer_logs {
            if !known_workspaces.contains(timer.workspace_id.as_str()) {
                return Err(
                    "Backup is inconsistent: a timer entry references an unknown workspace".to_string(),
                );
            }
        }
        for task in &payload.tasks {
            if !known_workspaces.contains(task.workspace_id.as_str()) {
                return Err(format!(
                    "Backup is inconsistent: task '{}' references an unknown workspace",
                    task.title
                ));
            }
        }
        for env_var in &payload.workspace_env_vars {
            if !known_workspaces.contains(env_var.workspace_id.as_str()) {
                return Err(format!(
                    "Backup is inconsistent: variable '{}' references an unknown workspace",
                    env_var.env_key
                ));
            }
        }

        let mut guard = lock_db(&conn)?;
        let tx = guard
            .transaction()
            .map_err(|e| format!("failed to begin transaction: {e}"))?;

        // Full replacement. Explicit deletes cover rows the cascade cannot
        // reach (global scripts have no parent workspace).
        tx.execute("DELETE FROM workspace_env_vars", [])
            .map_err(|e| format!("failed to clear variables: {e}"))?;
        tx.execute("DELETE FROM tasks", [])
            .map_err(|e| format!("failed to clear tasks: {e}"))?;
        tx.execute("DELETE FROM automation_scripts", [])
            .map_err(|e| format!("failed to clear scripts: {e}"))?;
        tx.execute("DELETE FROM timer_logs", [])
            .map_err(|e| format!("failed to clear timers: {e}"))?;
        tx.execute("DELETE FROM resources", [])
            .map_err(|e| format!("failed to clear resources: {e}"))?;
        tx.execute("DELETE FROM workspaces", [])
            .map_err(|e| format!("failed to clear workspaces: {e}"))?;

        for workspace in &payload.workspaces {
            insert_workspace(&tx, workspace)?;
        }
        for resource in &payload.resources {
            insert_resource(&tx, resource)?;
        }
        for script in &payload.automation_scripts {
            insert_script(&tx, script)?;
        }
        for timer in &payload.timer_logs {
            insert_timer_log(&tx, timer)?;
        }
        for task in &payload.tasks {
            insert_task(&tx, task)?;
        }
        for env_var in &payload.workspace_env_vars {
            insert_env_var(&tx, env_var)?;
        }

        // If any insert above failed, dropping `tx` rolls everything back.
        tx.commit()
            .map_err(|e| format!("failed to commit import: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

const MAX_ENV_KEY: usize = 128;
const MAX_ENV_VALUE: usize = 4096;

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        title: row.get(2)?,
        is_completed: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
    })
}

#[tauri::command]
pub async fn get_tasks(db: State<'_, Db>, workspace_id: String) -> Result<Vec<Task>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let mut stmt = guard
            .prepare(
                "SELECT id, workspace_id, title, is_completed, created_at
                 FROM tasks WHERE workspace_id = ?1
                 ORDER BY is_completed ASC, created_at ASC, rowid ASC",
            )
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map(params![workspace_id], row_to_task)
            .map_err(|e| format!("failed to query tasks: {e}"))?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|e| format!("failed to read task row: {e}"))?);
        }
        Ok(tasks)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn create_task(db: State<'_, Db>, input: NewTask) -> Result<Task, String> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err("Task title cannot be empty".into());
    }
    if title.len() > 120 {
        return Err("Task title is too long (max 120 characters)".into());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO tasks (id, workspace_id, title) VALUES (?1, ?2, ?3)",
                params![id, input.workspace_id, title],
            )
            .map_err(|e| {
                if e.to_string().contains("FOREIGN KEY constraint failed") {
                    "Workspace does not exist".to_string()
                } else {
                    format!("failed to insert task: {e}")
                }
            })?;

        let mut stmt = guard
            .prepare("SELECT id, workspace_id, title, is_completed, created_at FROM tasks WHERE id = ?1")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_task)
            .map_err(|e| format!("failed to read back task: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn update_task_status(
    db: State<'_, Db>,
    id: String,
    is_completed: bool,
) -> Result<Task, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute(
                "UPDATE tasks SET is_completed = ?2 WHERE id = ?1",
                params![id, is_completed],
            )
            .map_err(|e| format!("failed to update task: {e}"))?;
        if rows == 0 {
            return Err(format!("Task '{id}' not found"));
        }

        let mut stmt = guard
            .prepare("SELECT id, workspace_id, title, is_completed, created_at FROM tasks WHERE id = ?1")
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![id], row_to_task)
            .map_err(|e| format!("failed to read back task: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn delete_task(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM tasks WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete task: {e}"))?;
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

fn validate_env_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("Variable name cannot be empty".into());
    }
    if key.len() > MAX_ENV_KEY {
        return Err(format!(
            "Variable name is too long (max {MAX_ENV_KEY} characters)"
        ));
    }
    if key.contains('=') || key.contains('\0') || key.chars().any(char::is_whitespace) {
        return Err("Variable name cannot contain '=', whitespace or control characters".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_env_vars(
    db: State<'_, Db>,
    workspace_id: String,
) -> Result<Vec<EnvVar>, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let mut stmt = guard
            .prepare(
                "SELECT id, workspace_id, env_key, env_value
                 FROM workspace_env_vars WHERE workspace_id = ?1
                 ORDER BY env_key COLLATE NOCASE ASC",
            )
            .map_err(|e| format!("failed to prepare query: {e}"))?;

        let rows = stmt
            .query_map(params![workspace_id], |row| {
                Ok(EnvVar {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    env_key: row.get(2)?,
                    env_value: row.get(3)?,
                })
            })
            .map_err(|e| format!("failed to query environment variables: {e}"))?;

        let mut vars = Vec::new();
        for row in rows {
            vars.push(row.map_err(|e| format!("failed to read env var row: {e}"))?);
        }
        Ok(vars)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn set_env_var(
    db: State<'_, Db>,
    workspace_id: String,
    env_key: String,
    env_value: String,
) -> Result<EnvVar, String> {
    let key = env_key.trim().to_string();
    validate_env_key(&key)?;
    if env_value.contains('\0') {
        return Err("Variable value cannot contain control characters".into());
    }
    if env_value.len() > MAX_ENV_VALUE {
        return Err(format!(
            "Variable value is too long (max {MAX_ENV_VALUE} characters)"
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO workspace_env_vars (id, workspace_id, env_key, env_value)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(workspace_id, env_key)
                 DO UPDATE SET env_value = excluded.env_value",
                params![id, workspace_id, key, env_value],
            )
            .map_err(|e| {
                if e.to_string().contains("FOREIGN KEY constraint failed") {
                    "Workspace does not exist".to_string()
                } else {
                    format!("failed to save variable: {e}")
                }
            })?;

        let mut stmt = guard
            .prepare(
                "SELECT id, workspace_id, env_key, env_value
                 FROM workspace_env_vars WHERE workspace_id = ?1 AND env_key = ?2",
            )
            .map_err(|e| format!("failed to prepare query: {e}"))?;
        stmt.query_row(params![workspace_id, key], |row| {
            Ok(EnvVar {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                env_key: row.get(2)?,
                env_value: row.get(3)?,
            })
        })
        .map_err(|e| format!("failed to read back variable: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn delete_env_var(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM workspace_env_vars WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete variable: {e}"))?;
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}
