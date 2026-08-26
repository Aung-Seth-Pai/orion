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

/// Schema revision recorded in `PRAGMA user_version`. Bump this and add a
/// matching block in [`apply_versioned_migrations`] whenever an existing
/// database needs altering — the base `MIGRATIONS` batch above only ever runs
/// `CREATE TABLE IF NOT EXISTS`, so it cannot change a table that already exists.
const SCHEMA_VERSION: i64 = 1;

/// Non-destructive, in-place upgrades for databases created by an older build.
/// Each step runs exactly once: `user_version` is advanced inside the same
/// batch as the DDL, so a crash mid-upgrade leaves the version untouched and
/// the step is retried on the next launch.
fn apply_versioned_migrations(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| format!("failed to read schema version: {e}"))?;

    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    if version < 1 {
        // Marks the point in time a workspace's accumulated time was zeroed.
        // 0 (the default) means "never reset" — count every session ever logged.
        conn.execute_batch(
            "ALTER TABLE workspaces ADD COLUMN timer_reset_at INTEGER DEFAULT 0;
             PRAGMA user_version = 1;",
        )
        .map_err(|e| format!("migration to schema version 1 failed: {e}"))?;
        log::info!("Database migrated to schema version 1 (workspaces.timer_reset_at)");
    }

    Ok(())
}

/// Brings an open connection up to the current schema: base tables first, then
/// the versioned upgrades. Split out of [`Db::initialize`] so tests can build
/// an equivalent database in memory.
pub(crate) fn init_schema(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    for migration in MIGRATIONS {
        conn.execute_batch(migration)?;
    }
    // Runs after the base schema so a brand-new database and an upgraded one
    // converge on the same shape.
    apply_versioned_migrations(conn)?;
    Ok(())
}

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

        init_schema(&conn)?;

        Ok(Self(Arc::new(Mutex::new(conn))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    fn has_timer_reset_at(conn: &Connection) -> bool {
        conn.prepare("SELECT timer_reset_at FROM workspaces").is_ok()
    }

    #[test]
    fn fresh_database_reaches_the_current_schema_version() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        assert!(has_timer_reset_at(&conn));
    }

    #[test]
    fn upgrade_keeps_existing_rows_and_runs_once() {
        let conn = Connection::open_in_memory().unwrap();

        // A database as an older build left it: base schema, version 0, data.
        for migration in MIGRATIONS {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'Existing')",
            [],
        )
        .unwrap();
        assert_eq!(schema_version(&conn), 0);
        assert!(!has_timer_reset_at(&conn));

        apply_versioned_migrations(&conn).unwrap();

        assert_eq!(schema_version(&conn), 1);
        let (name, reset_at): (String, i64) = conn
            .query_row(
                "SELECT name, timer_reset_at FROM workspaces WHERE id = 'w1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "Existing");
        assert_eq!(reset_at, 0, "existing workspaces count all logged time");

        // Re-running must not attempt the ALTER a second time.
        apply_versioned_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), 1);
    }
}
