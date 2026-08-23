use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

const MIGRATIONS: &[&str] = &[
    "
    CREATE TABLE IF NOT EXISTS workspaces (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        color TEXT,
        icon TEXT,
        sort_order INTEGER DEFAULT 0,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS resources (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        type TEXT NOT NULL CHECK(type IN ('link', 'folder')),
        title TEXT NOT NULL,
        target_path TEXT NOT NULL,
        preferred_app TEXT DEFAULT 'default',
        profile_name TEXT,
        sort_order INTEGER DEFAULT 0,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_resources_workspace ON resources(workspace_id);
    ",
    "
    CREATE TABLE IF NOT EXISTS automation_scripts (
        id TEXT PRIMARY KEY,
        workspace_id TEXT,
        title TEXT NOT NULL,
        script_type TEXT NOT NULL CHECK(script_type IN ('powershell', 'cmd', 'python', 'node', 'wsl_bash')),
        script_content TEXT NOT NULL,
        sort_order INTEGER DEFAULT 0,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS timer_logs (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        duration_seconds INTEGER NOT NULL,
        session_type TEXT DEFAULT 'stopwatch' CHECK(session_type IN ('stopwatch', 'pomodoro')),
        started_at DATETIME NOT NULL,
        ended_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS app_settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        title TEXT NOT NULL,
        is_completed INTEGER DEFAULT 0,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );
    ",
    "
    CREATE TABLE IF NOT EXISTS workspace_env_vars (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        env_key TEXT NOT NULL,
        env_value TEXT NOT NULL,
        FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
    );
    CREATE UNIQUE INDEX IF NOT EXISTS idx_env_vars_workspace_key
        ON workspace_env_vars(workspace_id, env_key);
    ",
];

#[derive(Clone)]
pub struct Db(pub Arc<Mutex<Connection>>);

impl Db {
    pub fn initialize(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let dir: PathBuf = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create app data dir {}: {e}", dir.display()))?;

        let db_path = dir.join("data.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("failed to open database at {}: {e}", db_path.display()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("failed to enable WAL mode: {e}"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| format!("failed to enforce foreign keys: {e}"))?;
        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(|e| format!("failed to set busy timeout: {e}"))?;

        for migration in MIGRATIONS {
            conn.execute_batch(migration)?;
        }

        Ok(Self(Arc::new(Mutex::new(conn))))
    }
}
