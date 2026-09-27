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
const SCHEMA_VERSION: i64 = 2;

/// Vector index backing semantic search. `sqlite-vec` stores the embeddings in
/// shadow tables inside the same `data.db` file, so a backup of that one file
/// still captures everything and there is no second store to keep in sync.
///
/// `item_id` mirrors the UUID of the workspace, resource or script the vector
/// describes; `item_type` is a metadata column so a query can be narrowed to
/// one kind of item the way the `/ws` and `/script` search scopes already do.
/// There is deliberately no foreign key — a virtual table cannot carry one, so
/// deletions have to clear the vector explicitly (Phase 2).
fn vector_index_ddl() -> String {
    format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(
            item_id TEXT PRIMARY KEY,
            item_type TEXT,
            embedding float[{}]
        );",
        crate::ai::EMBEDDING_DIM
    )
}

/// Makes the `vec0` module available to every connection opened afterwards.
///
/// `sqlite-vec` is compiled into the binary and linked against the same
/// bundled SQLite that rusqlite uses, so this registers a static extension —
/// there is no `.dll`/`.so`/`.dylib` to ship or load per platform.
fn register_vector_extension() -> Result<(), String> {
    use std::os::raw::{c_char, c_int};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Once;

    static REGISTERED: Once = Once::new();
    static RESULT: AtomicI32 = AtomicI32::new(rusqlite::ffi::SQLITE_OK);

    REGISTERED.call_once(|| {
        // SAFETY: `sqlite3_vec_init` has exactly the signature SQLite expects of
        // an extension entry point; the transmute only erases the pointer's
        // identity. `Once` guarantees this runs a single time per process.
        let code = unsafe {
            let init: unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> c_int = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            rusqlite::ffi::sqlite3_auto_extension(Some(init))
        };
        RESULT.store(code, Ordering::SeqCst);
    });

    let code = RESULT.load(Ordering::SeqCst);
    if code != rusqlite::ffi::SQLITE_OK {
        return Err(format!(
            "failed to register the sqlite-vec extension (code {code})"
        ));
    }
    Ok(())
}

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

    if version < 2 {
        conn.execute_batch(&format!(
            "{}
             PRAGMA user_version = 2;",
            vector_index_ddl()
        ))
        .map_err(|e| format!("migration to schema version 2 failed: {e}"))?;
        log::info!("Database migrated to schema version 2 (vec_items vector index)");
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
    debug_assert!(
        conn.query_row("SELECT vec_version()", [], |row| row.get::<_, String>(0))
            .is_ok(),
        "register_vector_extension() must run before opening the connection"
    );
    // Runs after the base schema so a brand-new database and an upgraded one
    // converge on the same shape.
    apply_versioned_migrations(conn)?;
    Ok(())
}

#[derive(Clone)]
pub struct Db(pub Arc<Mutex<Connection>>);

impl Db {
    pub fn initialize(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        // Must precede Connection::open: auto-extensions only apply to
        // connections opened after they are registered.
        register_vector_extension()?;

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

/// An in-memory database with the vector extension registered, matching what
/// `Db::initialize` sets up. Tests in other modules use this so they never open
/// a connection that is missing `vec0`.
#[cfg(test)]
pub(crate) fn open_test_connection() -> Connection {
    register_vector_extension().expect("vector extension must register");
    let conn = Connection::open_in_memory().expect("in-memory database must open");
    conn.pragma_update(None, "foreign_keys", "ON")
        .expect("foreign keys must enable");
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        open_test_connection()
    }

    fn schema_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    fn has_timer_reset_at(conn: &Connection) -> bool {
        conn.prepare("SELECT timer_reset_at FROM workspaces").is_ok()
    }

    #[test]
    fn fresh_database_reaches_the_current_schema_version() {
        let conn = test_conn();
        init_schema(&conn).unwrap();

        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        assert!(has_timer_reset_at(&conn));
    }

    #[test]
    fn upgrade_keeps_existing_rows_and_runs_once() {
        let conn = test_conn();

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

        // A version-0 database catches up through every step in one pass.
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        assert!(has_timer_reset_at(&conn));
        assert!(conn.prepare("SELECT item_id FROM vec_items").is_ok());

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
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn vector_extension_is_statically_available() {
        let conn = test_conn();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .expect("sqlite-vec must be linked into the binary");
        assert!(version.starts_with('v'), "unexpected version: {version}");
    }

    #[test]
    fn vector_index_accepts_and_ranks_embeddings() {
        let conn = test_conn();
        init_schema(&conn).unwrap();

        // Three unit vectors: two near each other, one orthogonal.
        let unit = |a: f32, b: f32| {
            let mut v = vec![0f32; crate::ai::EMBEDDING_DIM];
            v[0] = a;
            v[1] = b;
            crate::ai::embedding_to_blob(&v)
        };

        let mut insert = conn
            .prepare("INSERT INTO vec_items(item_id, item_type, embedding) VALUES (?1, ?2, ?3)")
            .unwrap();
        insert.execute(rusqlite::params!["a", "link", unit(1.0, 0.0)]).unwrap();
        insert.execute(rusqlite::params!["b", "script", unit(0.92, 0.39)]).unwrap();
        insert.execute(rusqlite::params!["c", "workspace", unit(0.0, 1.0)]).unwrap();
        drop(insert);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);

        // A KNN search against the first vector must rank it above the
        // orthogonal one — this is the query shape Phase 2 will use.
        let mut stmt = conn
            .prepare(
                "SELECT item_id FROM vec_items
                 WHERE embedding MATCH ?1 AND k = 2
                 ORDER BY distance",
            )
            .unwrap();
        let ranked: Vec<String> = stmt
            .query_map(rusqlite::params![unit(1.0, 0.0)], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ranked, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn item_type_is_filterable() {
        let conn = test_conn();
        init_schema(&conn).unwrap();

        let blob = crate::ai::embedding_to_blob(&vec![0.1f32; crate::ai::EMBEDDING_DIM]);
        conn.execute(
            "INSERT INTO vec_items(item_id, item_type, embedding) VALUES ('s1', 'script', ?1)",
            rusqlite::params![blob],
        )
        .unwrap();

        let stored: String = conn
            .query_row(
                "SELECT item_type FROM vec_items WHERE item_id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "script");
    }

    #[test]
    fn vector_index_rejects_the_wrong_dimension() {
        let conn = test_conn();
        init_schema(&conn).unwrap();

        let too_short = crate::ai::embedding_to_blob(&[0.5f32; 8]);
        let result = conn.execute(
            "INSERT INTO vec_items(item_id, item_type, embedding) VALUES ('x', 'link', ?1)",
            rusqlite::params![too_short],
        );
        assert!(result.is_err(), "a 8-dim vector must not enter a 384-dim index");
    }
}
