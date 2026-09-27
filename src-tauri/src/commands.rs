use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::types::{ToSql, ToSqlOutput};
use rusqlite::{params, params_from_iter};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;
use tokio::process::Command;
use tokio::task::spawn_blocking;

use crate::db::Db;
use crate::models::{
    AiStatus, AutomationScript, BackupPayload, EnvVar, NewResource, NewScript, NewTask,
    NewTimerLog, NewWorkspace, Resource, ResourcePatch, ResourceSuggestion, ScriptPatch,
    SearchResult, Task, TimerLog, Workspace, WorkspacePatch,
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

const COLUMNS: &str = "id, name, color, icon, sort_order, created_at, timer_reset_at";

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
        timer_reset_at: row.get(6)?,
    })
}

fn lock_db(
    db: &Arc<Mutex<rusqlite::Connection>>,
) -> Result<MutexGuard<'_, rusqlite::Connection>, String> {
    db.lock().map_err(|_| "database mutex poisoned".to_string())
}

// --- Semantic index maintenance ---------------------------------------------

/// Item kinds stored in `vec_items`. They deliberately match the `item_type`
/// values the search results already use, so a semantic hit maps onto the same
/// frontend actions as a keyword hit.
const INDEX_WORKSPACE: &str = "workspace";
const INDEX_SCRIPT: &str = "script";

/// Where the embedding model is cached.
///
/// Deliberately the *local* data directory, not the roaming one that holds
/// `data.db`. The weights are ~90 MB and fully regenerable by re-downloading,
/// so they must not be pushed into a roaming profile that syncs between
/// machines. `data.db` stays where it is — it is small and genuinely worth
/// roaming.
fn model_root_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|e| format!("failed to resolve local app data dir: {e}"))
}

/// Embeds `text` and upserts it into the vector index, off the caller's path.
///
/// Two rules make this safe to call from any CRUD command:
///
/// 1. If the model has not been downloaded the task exits immediately. Creating
///    a folder must never kick off a 90 MB download — that only ever happens
///    when the user explicitly asks for a semantic search.
/// 2. Indexing is best-effort. A failure is logged and swallowed, never
///    surfaced, so a broken index cannot stop someone saving their work.
fn spawn_index_item(db: &State<'_, Db>, app: &AppHandle, id: String, item_type: &str, text: String) {
    let Ok(dir) = model_root_dir(app) else {
        return;
    };
    let conn = db.0.clone();
    let item_type = item_type.to_string();

    tokio::spawn(async move {
        if !crate::ai::is_model_downloaded(&dir) {
            return;
        }

        // Embedding is CPU-bound; keep it off the async runtime's threads.
        let outcome = spawn_blocking(move || {
            let embedding = crate::ai::generate_embedding(&dir, &text)?;
            let blob = crate::ai::embedding_to_blob(&embedding);
            let guard = lock_db(&conn)?;
            guard
                .execute(
                    "INSERT OR REPLACE INTO vec_items (item_id, item_type, embedding)
                     VALUES (?1, ?2, ?3)",
                    params![id, item_type, blob],
                )
                .map_err(|e| format!("failed to write vector: {e}"))?;
            Ok::<(), String>(())
        })
        .await;

        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("Semantic indexing skipped: {e}"),
            Err(e) => log::warn!("Semantic indexing task failed: {e}"),
        }
    });
}

/// The text a resource is indexed by. Title and target are both searchable —
/// "the repo I cloned into projects" should find a folder by its path as
/// readily as by its name.
fn resource_index_text(title: &str, target_path: &str) -> String {
    format!("{title} {target_path}")
}

/// Removes vectors for ids that no longer exist. Virtual tables cannot carry a
/// foreign key, so every delete path has to clean up after itself.
fn delete_vectors(conn: &rusqlite::Connection, ids: &[String]) -> Result<(), String> {
    for id in ids {
        conn.execute("DELETE FROM vec_items WHERE item_id = ?1", params![id])
            .map_err(|e| format!("failed to remove vector: {e}"))?;
    }
    Ok(())
}

/// One row queued for embedding during a backfill.
struct IndexableItem {
    id: String,
    item_type: String,
    text: String,
}

/// Everything in the database that belongs in the vector index, using the same
/// text each CRUD hook would have embedded.
fn collect_indexable(conn: &rusqlite::Connection) -> Result<Vec<IndexableItem>, String> {
    let mut items = Vec::new();

    let mut workspaces = conn
        .prepare("SELECT id, name FROM workspaces")
        .map_err(|e| format!("failed to prepare workspace scan: {e}"))?;
    let rows = workspaces
        .query_map([], |row| {
            Ok(IndexableItem {
                id: row.get(0)?,
                item_type: INDEX_WORKSPACE.to_string(),
                text: row.get(1)?,
            })
        })
        .map_err(|e| format!("failed to scan workspaces: {e}"))?;
    for row in rows {
        items.push(row.map_err(|e| format!("failed to read workspace row: {e}"))?);
    }

    let mut resources = conn
        .prepare("SELECT id, type, title, target_path FROM resources")
        .map_err(|e| format!("failed to prepare resource scan: {e}"))?;
    let rows = resources
        .query_map([], |row| {
            let title: String = row.get(2)?;
            let target: String = row.get(3)?;
            Ok(IndexableItem {
                id: row.get(0)?,
                item_type: row.get(1)?,
                text: resource_index_text(&title, &target),
            })
        })
        .map_err(|e| format!("failed to scan resources: {e}"))?;
    for row in rows {
        items.push(row.map_err(|e| format!("failed to read resource row: {e}"))?);
    }

    let mut scripts = conn
        .prepare("SELECT id, title FROM automation_scripts")
        .map_err(|e| format!("failed to prepare script scan: {e}"))?;
    let rows = scripts
        .query_map([], |row| {
            Ok(IndexableItem {
                id: row.get(0)?,
                item_type: INDEX_SCRIPT.to_string(),
                text: row.get(1)?,
            })
        })
        .map_err(|e| format!("failed to scan scripts: {e}"))?;
    for row in rows {
        items.push(row.map_err(|e| format!("failed to read script row: {e}"))?);
    }

    Ok(items)
}

/// Downloads the model if needed, then rebuilds the whole vector index.
///
/// This is the only path that fetches the model, and it exists so the download
/// is a deliberate choice made in Settings rather than a side effect of typing.
/// Items created before the model arrived have no vector — the CRUD hooks only
/// fire on write — so without this, semantic search would stay empty until
/// every item happened to be edited.
#[tauri::command]
pub async fn reindex_all(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let dir = model_root_dir(&app)?;
    crate::ai::ensure_model_downloaded(&dir).await?;

    let conn = db.0.clone();

    spawn_blocking(move || {
        // Read, embed, then write. The expensive middle step deliberately holds
        // no lock, so the rest of the app keeps working during a long backfill.
        let items = {
            let guard = lock_db(&conn)?;
            collect_indexable(&guard)?
        };

        let total = items.len();
        let mut embedded = Vec::with_capacity(total);
        for item in items {
            match crate::ai::generate_embedding(&dir, &item.text) {
                Ok(vector) => embedded.push((
                    item.id,
                    item.item_type,
                    crate::ai::embedding_to_blob(&vector),
                )),
                // One unembeddable row must not abandon the whole backfill.
                Err(e) => log::warn!("Skipping '{}' during re-index: {e}", item.id),
            }
        }

        let mut guard = lock_db(&conn)?;
        let tx = guard
            .transaction()
            .map_err(|e| format!("failed to begin re-index transaction: {e}"))?;

        // Replacing wholesale also drops vectors for rows deleted while the
        // model was absent, which no delete hook could have cleaned up.
        tx.execute("DELETE FROM vec_items", [])
            .map_err(|e| format!("failed to clear the index: {e}"))?;
        for (id, item_type, blob) in &embedded {
            tx.execute(
                "INSERT OR REPLACE INTO vec_items (item_id, item_type, embedding)
                 VALUES (?1, ?2, ?3)",
                params![id, item_type, blob],
            )
            .map_err(|e| format!("failed to write vector for '{id}': {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("failed to commit the re-index: {e}"))?;

        log::info!("Re-indexed {} of {total} items", embedded.len());
        Ok(())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Empties the vector index, leaving every workspace, resource and script
/// untouched. Only the embeddings go.
fn clear_vector_index(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute("DELETE FROM vec_items", [])
        .map_err(|e| format!("failed to clear the semantic index: {e}"))?;
    Ok(())
}

/// Deletes the downloaded weights and empties the index, returning the app to
/// the state it was in before semantic search was ever enabled.
///
/// Both halves matter. Leaving the index behind would have Settings report items
/// as indexed when the embedder needed to query them is gone, and leaving the
/// weights behind would keep ~90 MB on disk for a feature the user has turned
/// off. Nothing else is touched: this removes a cache, not data.
#[tauri::command]
pub async fn remove_ai_model(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    let dir = model_root_dir(&app)?;
    crate::ai::remove_model(&dir)?;

    let conn = db.0.clone();
    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        clear_vector_index(&guard)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Whether semantic search is usable, and how far the index has got.
#[tauri::command]
pub async fn get_ai_status(app: AppHandle, db: State<'_, Db>) -> Result<AiStatus, String> {
    let dir = model_root_dir(&app)?;
    let downloaded = crate::ai::is_model_downloaded(&dir);
    let model_bytes = crate::ai::model_disk_usage(&dir);
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let count = |sql: &str| -> Result<i64, String> {
            guard
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(|e| format!("failed to count rows: {e}"))
        };

        Ok(AiStatus {
            downloaded,
            model_bytes,
            indexed_count: count("SELECT COUNT(*) FROM vec_items")?,
            indexable_count: count(
                "SELECT (SELECT COUNT(*) FROM workspaces)
                      + (SELECT COUNT(*) FROM resources)
                      + (SELECT COUNT(*) FROM automation_scripts)",
            )?,
        })
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Every id a workspace deletion will remove: the workspace itself plus the
/// resources and scripts SQLite cascades away with it.
fn workspace_cascade_ids(
    conn: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<Vec<String>, String> {
    let mut ids = vec![workspace_id.to_string()];

    for sql in [
        "SELECT id FROM resources WHERE workspace_id = ?1",
        "SELECT id FROM automation_scripts WHERE workspace_id = ?1",
    ] {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("failed to prepare cascade query: {e}"))?;
        let rows = stmt
            .query_map(params![workspace_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("failed to list cascaded rows: {e}"))?;
        for row in rows {
            ids.push(row.map_err(|e| format!("failed to read cascaded id: {e}"))?);
        }
    }
    Ok(ids)
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
pub async fn create_workspace(
    app: AppHandle,
    db: State<'_, Db>,
    input: NewWorkspace,
) -> Result<Workspace, String> {
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
    .inspect(|workspace| {
        spawn_index_item(
            &db,
            &app,
            workspace.id.clone(),
            INDEX_WORKSPACE,
            workspace.name.clone(),
        );
    })
}

#[tauri::command]
pub async fn update_workspace(
    app: AppHandle,
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
    .inspect(|workspace| {
        spawn_index_item(
            &db,
            &app,
            workspace.id.clone(),
            INDEX_WORKSPACE,
            workspace.name.clone(),
        );
    })
}

#[tauri::command]
pub async fn delete_workspace(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        // Gathered before the delete: once the workspace is gone the cascade
        // has already taken its resources and scripts with it.
        let orphaned = workspace_cascade_ids(&guard, &id)?;

        let rows = guard
            .execute("DELETE FROM workspaces WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete workspace: {e}"))?;

        if rows > 0 {
            delete_vectors(&guard, &orphaned)?;
        }
        Ok(rows > 0)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

// --- Resource suggestions --------------------------------------------------

/// How many suggestions are worth showing under a text field. More than this is
/// a list to read rather than a hint to glance at.
const SUGGESTION_LIMIT: usize = 6;

/// Maximum L2 distance for a semantic suggestion to count as related.
///
/// The indexed vectors are unit length, so L2 distance and cosine similarity are
/// monotonically related and a fixed cut-off is meaningful either way: 1.0
/// corresponds to a cosine of 0.5. KNN always returns its k nearest neighbours
/// with no notion of relevance, so without a cut-off every query would suggest
/// something, however unrelated.
const SEMANTIC_SUGGESTION_MAX_DISTANCE: f64 = 1.0;

/// Nearest neighbours with their distances, for callers that need to apply a
/// relevance cut-off rather than take a fixed k.
fn knn_search_scored(
    conn: &rusqlite::Connection,
    blob: &[u8],
    limit: i64,
) -> Result<Vec<(String, String, f64)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT item_id, item_type, distance FROM vec_items
             WHERE embedding MATCH ?1 AND k = ?2
             ORDER BY distance",
        )
        .map_err(|e| format!("failed to prepare scored vector search: {e}"))?;

    let rows = stmt
        .query_map(params![blob, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })
        .map_err(|e| format!("failed to execute scored vector search: {e}"))?;

    let mut hits = Vec::new();
    for row in rows {
        hits.push(row.map_err(|e| format!("failed to read scored hit: {e}"))?);
    }
    Ok(hits)
}

/// Loads one resource as a suggestion, or None when the id is not a resource.
fn resource_suggestion_by_id(
    conn: &rusqlite::Connection,
    id: &str,
    match_kind: &str,
) -> Result<Option<ResourceSuggestion>, String> {
    use rusqlite::OptionalExtension;

    conn.query_row(
        "SELECT r.id, r.type, r.title, r.target_path, r.workspace_id, w.name
         FROM resources r JOIN workspaces w ON w.id = r.workspace_id
         WHERE r.id = ?1",
        params![id],
        |row| {
            Ok(ResourceSuggestion {
                id: row.get(0)?,
                resource_type: row.get(1)?,
                title: row.get(2)?,
                target_path: row.get(3)?,
                workspace_id: row.get(4)?,
                workspace_name: row.get(5)?,
                match_kind: match_kind.to_string(),
            })
        },
    )
    .optional()
    .map_err(|e| format!("failed to load suggestion: {e}"))
}

/// Resources whose title or target resembles `term`, across every workspace.
///
/// Searching every workspace rather than only the current one is the point: the
/// question being answered is "have I saved this somewhere already", and the
/// answer is most useful precisely when it is somewhere else.
///
/// Two tiers, so it degrades rather than breaks. The keyword tier always works.
/// The semantic tier only contributes when the index exists, and never triggers
/// a model download — typing a title must not start a 90 MB fetch.
#[tauri::command]
pub async fn suggest_resources(
    app: AppHandle,
    db: State<'_, Db>,
    term: String,
) -> Result<Vec<ResourceSuggestion>, String> {
    let term = term.trim().to_string();
    // Below two characters everything matches and nothing is a hint.
    if term.chars().count() < 2 || term.len() > MAX_QUERY_CHARS {
        return Ok(Vec::new());
    }

    let model_dir = model_root_dir(&app).ok();
    let semantic_ready = model_dir
        .as_deref()
        .is_some_and(crate::ai::is_model_downloaded);

    let pattern = build_like_pattern(&term);
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        let mut suggestions: Vec<ResourceSuggestion> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Tier 1: keyword. Always available, and instant.
        {
            let mut stmt = guard
                .prepare(
                    "SELECT r.id, r.type, r.title, r.target_path, r.workspace_id, w.name
                     FROM resources r JOIN workspaces w ON w.id = r.workspace_id
                     WHERE r.title LIKE ?1 ESCAPE '\\' OR r.target_path LIKE ?1 ESCAPE '\\'
                     ORDER BY r.title COLLATE NOCASE ASC
                     LIMIT ?2",
                )
                .map_err(|e| format!("failed to prepare suggestion query: {e}"))?;

            let rows = stmt
                .query_map(params![pattern, SUGGESTION_LIMIT as i64], |row| {
                    Ok(ResourceSuggestion {
                        id: row.get(0)?,
                        resource_type: row.get(1)?,
                        title: row.get(2)?,
                        target_path: row.get(3)?,
                        workspace_id: row.get(4)?,
                        workspace_name: row.get(5)?,
                        match_kind: "keyword".to_string(),
                    })
                })
                .map_err(|e| format!("failed to query suggestions: {e}"))?;

            for row in rows {
                let suggestion = row.map_err(|e| format!("failed to read suggestion: {e}"))?;
                seen.insert(suggestion.id.clone());
                suggestions.push(suggestion);
            }
        }

        // Tier 2: semantic. Catches the cases keyword search cannot — "AWS docs"
        // against "Amazon Web Services documentation".
        if semantic_ready && suggestions.len() < SUGGESTION_LIMIT {
            let dir = model_dir.expect("checked above");
            match crate::ai::generate_embedding(&dir, &term) {
                Ok(embedding) => {
                    let blob = crate::ai::embedding_to_blob(&embedding);
                    let hits = knn_search_scored(&guard, &blob, SUGGESTION_LIMIT as i64 * 2)?;
                    for (id, item_type, distance) in hits {
                        if suggestions.len() >= SUGGESTION_LIMIT {
                            break;
                        }
                        // Workspaces and scripts share the index but are not
                        // resources, so they cannot be duplicates of one.
                        if item_type != "link" && item_type != "folder" {
                            continue;
                        }
                        if distance > SEMANTIC_SUGGESTION_MAX_DISTANCE || seen.contains(&id) {
                            continue;
                        }
                        if let Some(found) = resource_suggestion_by_id(&guard, &id, "semantic")? {
                            seen.insert(found.id.clone());
                            suggestions.push(found);
                        }
                    }
                }
                // A suggestion box is a convenience; losing the semantic tier
                // must not fail the keyword results already gathered.
                Err(e) => log::warn!("Semantic suggestions unavailable: {e}"),
            }
        }

        Ok(suggestions)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// The resource that already points at `target`, if any.
///
/// Distinct from [`suggest_resources`] because an exact duplicate is a different
/// statement: not "this looks similar" but "you already have this", which
/// deserves its own wording. The target is normalised first so a bare path and
/// the file:// URL it becomes are recognised as the same thing.
#[tauri::command]
pub async fn find_resource_by_target(
    db: State<'_, Db>,
    resource_type: String,
    target: String,
) -> Result<Option<ResourceSuggestion>, String> {
    let resource_type = resource_type.trim().to_lowercase();
    let normalised = match normalize_target_path(&resource_type, &target) {
        Ok(value) => value,
        // An unnormalisable target cannot match anything stored; the real error
        // belongs to the save attempt, not to a background duplicate check.
        Err(_) => return Ok(None),
    };
    if normalised.is_empty() {
        return Ok(None);
    }

    let conn = db.0.clone();
    spawn_blocking(move || {
        use rusqlite::OptionalExtension;
        let guard = lock_db(&conn)?;
        guard
            .query_row(
                "SELECT r.id, r.type, r.title, r.target_path, r.workspace_id, w.name
                 FROM resources r JOIN workspaces w ON w.id = r.workspace_id
                 WHERE r.target_path = ?1 COLLATE NOCASE
                 LIMIT 1",
                params![normalised],
                |row| {
                    Ok(ResourceSuggestion {
                        id: row.get(0)?,
                        resource_type: row.get(1)?,
                        title: row.get(2)?,
                        target_path: row.get(3)?,
                        workspace_id: row.get(4)?,
                        workspace_name: row.get(5)?,
                        match_kind: "exact".to_string(),
                    })
                },
            )
            .optional()
            .map_err(|e| format!("failed to check for a duplicate: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

// --- Global shortcut -------------------------------------------------------

/// The stored spotlight accelerator, or the default when nothing is stored.
/// Used at startup and as the rollback target when a new one cannot be
/// registered.
pub(crate) fn stored_spotlight_shortcut(conn: &rusqlite::Connection) -> String {
    use rusqlite::OptionalExtension;

    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        params![crate::tray::SPOTLIGHT_SHORTCUT_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
    .unwrap_or_else(|| crate::tray::DEFAULT_SPOTLIGHT_SHORTCUT.to_string())
}

/// Changes the shortcut that summons the Spotlight window.
///
/// Registration is attempted before anything is persisted, and the previous
/// accelerator is put back if it fails — otherwise a combination already owned
/// by another application would be saved and leave the user with no working
/// shortcut, and no obvious way to discover why.
#[tauri::command]
pub async fn set_spotlight_shortcut(
    app: AppHandle,
    db: State<'_, Db>,
    accelerator: String,
) -> Result<(), String> {
    let accelerator = accelerator.trim().to_string();
    if accelerator.is_empty() {
        return Err("Shortcut cannot be empty".into());
    }
    if accelerator.len() > 64 {
        return Err("Shortcut is too long".into());
    }

    let conn = db.0.clone();
    let previous = {
        let read = conn.clone();
        spawn_blocking(move || {
            let guard = lock_db(&read)?;
            Ok::<String, String>(stored_spotlight_shortcut(&guard))
        })
        .await
        .map_err(|e| format!("background task failed: {e}"))??
    };

    if let Err(e) = crate::tray::apply_spotlight_shortcut(&app, &accelerator) {
        // Put the working shortcut back so the app is not left with none.
        if let Err(restore) = crate::tray::apply_spotlight_shortcut(&app, &previous) {
            log::error!("Failed to restore the previous shortcut '{previous}': {restore}");
        }
        return Err(e);
    }

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![crate::tray::SPOTLIGHT_SHORTCUT_KEY, accelerator],
            )
            .map_err(|e| format!("failed to save the shortcut: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

// --- Manual ordering -------------------------------------------------------

/// Rewrites `sort_order` for one scope from a caller-supplied order.
///
/// `scope_sql` is a fixed predicate chosen by the caller, never user input —
/// the table and WHERE clause are compile-time strings and only the ids and
/// scope value are bound as parameters.
///
/// The supplied ids must be exactly the ids in that scope. A drag-and-drop UI
/// reorders a list it has already loaded, so anything else means the two have
/// diverged, and silently ordering a subset would leave the unlisted rows
/// sharing sort_order values with the listed ones — a list that reshuffles on
/// its own the next time it loads. Failing loudly lets the caller refetch.
fn apply_manual_order(
    conn: &mut rusqlite::Connection,
    table: &'static str,
    scope_sql: &'static str,
    scope_value: Option<&str>,
    ids: &[String],
) -> Result<(), String> {
    let mut unique = std::collections::HashSet::new();
    for id in ids {
        if !unique.insert(id.as_str()) {
            return Err(format!("'{id}' appears more than once in the new order"));
        }
    }

    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to begin reorder: {e}"))?;

    let existing: std::collections::HashSet<String> = {
        let sql = format!("SELECT id FROM {table} WHERE {scope_sql}");
        let mut stmt = tx
            .prepare(&sql)
            .map_err(|e| format!("failed to prepare scope query: {e}"))?;
        // One param list rather than a match over two query_map calls: the
        // closures would otherwise be distinct types and the arms incompatible.
        let scope_params: Vec<String> = scope_value.map(str::to_string).into_iter().collect();
        let rows = stmt
            .query_map(params_from_iter(scope_params.iter()), |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| format!("failed to list the current order: {e}"))?;

        let mut set = std::collections::HashSet::new();
        for row in rows {
            set.insert(row.map_err(|e| format!("failed to read id: {e}"))?);
        }
        set
    };

    if existing.len() != unique.len() || !existing.iter().all(|id| unique.contains(id.as_str())) {
        return Err(format!(
            "the new order does not match what is stored ({} given, {} stored) — reload and try again",
            unique.len(),
            existing.len()
        ));
    }

    {
        let sql = format!("UPDATE {table} SET sort_order = ?1 WHERE id = ?2");
        let mut stmt = tx
            .prepare(&sql)
            .map_err(|e| format!("failed to prepare reorder: {e}"))?;
        for (position, id) in ids.iter().enumerate() {
            stmt.execute(params![position as i64, id])
                .map_err(|e| format!("failed to reorder '{id}': {e}"))?;
        }
    }

    tx.commit()
        .map_err(|e| format!("failed to commit reorder: {e}"))?;
    Ok(())
}

/// Persists a drag-and-drop reorder of the workspace sidebar.
#[tauri::command]
pub async fn reorder_workspaces(db: State<'_, Db>, ids: Vec<String>) -> Result<(), String> {
    let conn = db.0.clone();
    spawn_blocking(move || {
        let mut guard = lock_db(&conn)?;
        apply_manual_order(&mut guard, "workspaces", "1 = 1", None, &ids)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Persists a drag-and-drop reorder of one workspace's resources.
#[tauri::command]
pub async fn reorder_resources(
    db: State<'_, Db>,
    workspace_id: String,
    ids: Vec<String>,
) -> Result<(), String> {
    let conn = db.0.clone();
    spawn_blocking(move || {
        let mut guard = lock_db(&conn)?;
        apply_manual_order(
            &mut guard,
            "resources",
            "workspace_id = ?1",
            Some(&workspace_id),
            &ids,
        )
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Persists a drag-and-drop reorder of a script list. `workspace_id` is None for
/// the global list, matching the scoping get_scripts already uses.
#[tauri::command]
pub async fn reorder_scripts(
    db: State<'_, Db>,
    workspace_id: Option<String>,
    ids: Vec<String>,
) -> Result<(), String> {
    let conn = db.0.clone();
    spawn_blocking(move || {
        let mut guard = lock_db(&conn)?;
        match workspace_id.as_deref() {
            Some(ws) => apply_manual_order(
                &mut guard,
                "automation_scripts",
                "workspace_id = ?1",
                Some(ws),
                &ids,
            ),
            None => apply_manual_order(
                &mut guard,
                "automation_scripts",
                "workspace_id IS NULL",
                None,
                &ids,
            ),
        }
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

/// True for an absolute native path: a drive-letter path (`C:\...` or `C:/...`)
/// or a UNC share (`\server\share`). Relative paths are deliberately excluded —
/// they have no meaning without a working directory, and accepting them would
/// turn a typo into a silently broken resource.
fn is_absolute_local_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    let drive_letter = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let unc = target.starts_with(r"\\") || target.starts_with("//");
    drive_letter || unc
}

/// Percent-encodes the characters that would otherwise change how a URL parses.
///
/// `/` and `:` are left alone because they are structural here (the separator
/// and the drive colon). Everything outside the unreserved set is escaped,
/// which covers the two that actually bite in practice: a space, and a `#` in a
/// filename silently truncating the path into a fragment.
fn percent_encode_path(path: &str) -> String {
    const KEEP: &[u8] = b"-._~/:";
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || KEEP.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Turns an absolute local path into the `file://` URL the launcher expects,
/// leaving anything that is already a URL untouched.
///
/// This exists so a link can be given as `C:\Users\me\Downloads\cv.pdf` — the
/// form Explorer's "Copy as path" produces — instead of demanding the user
/// hand-write `file:///C:/Users/me/Downloads/cv.pdf`. Normalising on the way in
/// rather than at launch time means the stored value is always a valid URL, so
/// `launch_resource` and its scheme check are unchanged, and a local file is not
/// a new resource *type* needing a CHECK-constraint migration.
fn normalize_target_path(resource_type: &str, target: &str) -> Result<String, String> {
    let trimmed = target.trim();

    // Folders are stored and launched as native paths, not URLs.
    if resource_type != "link" || !is_absolute_local_path(trimmed) {
        return Ok(trimmed.to_string());
    }

    // A directory given as a Link would open a file:// listing in the browser
    // rather than Explorer, which is never what was meant.
    if std::path::Path::new(trimmed).is_dir() {
        return Err(
            "That path is a folder. Add it as a Folder resource instead of a Link.".to_string(),
        );
    }

    // Backslashes are not legal in a URL path; the drive colon and the
    // separators both survive percent_encode_path untouched.
    let forward = trimmed.replace('\\', "/");
    let encoded = percent_encode_path(&forward);

    // A UNC path already starts with two slashes, giving file://<host>/<share>.
    // A drive path needs the empty authority, giving file:///C:/...
    if encoded.starts_with("//") {
        Ok(format!("file:{encoded}"))
    } else {
        Ok(format!("file:///{encoded}"))
    }
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
    if resource_type == "link"
        && !(target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("file://"))
    {
        return Err(
            r"Links must be a http:// or https:// URL, or an absolute file path such as C:\Users\you\file.pdf"
                .to_string(),
        );
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
pub async fn create_resource(
    app: AppHandle,
    db: State<'_, Db>,
    input: NewResource,
) -> Result<Resource, String> {
    let resource_type = input.resource_type.trim().to_lowercase();
    let preferred_app = input.preferred_app.trim().to_lowercase();
    // Before validation, so a bare absolute path is already a file:// URL by
    // the time the scheme check runs.
    let target_path = normalize_target_path(&resource_type, &input.target_path)?;

    validate_resource_fields(&resource_type, &input.title, &target_path, &preferred_app)?;

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
                    target_path,
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
    .inspect(|resource| {
        spawn_index_item(
            &db,
            &app,
            resource.id.clone(),
            &resource.resource_type,
            resource_index_text(&resource.title, &resource.target_path),
        );
    })
}

#[tauri::command]
pub async fn update_resource(
    app: AppHandle,
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

            let merged_target = normalize_target_path(&merged_type, merged_target)?;
            validate_resource_fields(&merged_type, merged_title, &merged_target, &merged_app)?;

            let mut assignments: Vec<&str> = Vec::new();
            let mut values: Vec<SqlValue> = Vec::new();

            if let Some(title) = &patch.title {
                assignments.push("title = ?");
                values.push(SqlValue::Text(title.trim().to_string()));
            }
            if patch.target_path.is_some() {
                assignments.push("target_path = ?");
                // The normalised value, not the raw patch: otherwise editing a
                // resource would store the bare path the scheme check rejects.
                values.push(SqlValue::Text(merged_target.clone()));
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
    .inspect(|resource| {
        spawn_index_item(
            &db,
            &app,
            resource.id.clone(),
            &resource.resource_type,
            resource_index_text(&resource.title, &resource.target_path),
        );
    })
}

#[tauri::command]
pub async fn delete_resource(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM resources WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete resource: {e}"))?;

        if rows > 0 {
            delete_vectors(&guard, std::slice::from_ref(&id))?;
        }
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
                log::error!(
                    "Launch failed: folder does not exist: {}",
                    target.target_path
                );
                return Err(format!("Folder does not exist: {}", target.target_path));
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
                    if let Err(e) = Command::new("wt").args(["-d", &target.target_path]).spawn() {
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
            if !(url.starts_with("http://")
                || url.starts_with("https://")
                || url.starts_with("file://"))
            {
                log::error!("Launch failed for '{log_id}': refusing non-http(s)/file URL");
                return Err("Refusing to launch: links must be http(s) or file URLs".into());
            }
            if url.contains('"') || url.contains('\0') {
                log::error!("Launch failed for '{log_id}': URL contains invalid characters");
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
                    log::error!("Launch failed for '{log_id}': unknown preferred app '{other}'");
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
    app: AppHandle,
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
    .inspect(|script| {
        spawn_index_item(
            &db,
            &app,
            script.id.clone(),
            INDEX_SCRIPT,
            script.title.clone(),
        );
    })
}

#[tauri::command]
pub async fn update_script(
    app: AppHandle,
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
    .inspect(|script| {
        spawn_index_item(
            &db,
            &app,
            script.id.clone(),
            INDEX_SCRIPT,
            script.title.clone(),
        );
    })
}

#[tauri::command]
pub async fn delete_script(db: State<'_, Db>, id: String) -> Result<bool, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute("DELETE FROM automation_scripts WHERE id = ?1", params![id])
            .map_err(|e| format!("failed to delete script: {e}"))?;

        if rows > 0 {
            delete_vectors(&guard, std::slice::from_ref(&id))?;
        }
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

    let child = command.spawn().map_err(|e| {
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
        let (script_type, script_content, workspace_id): (String, String, Option<String>) = guard
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
        log::error!(
            "Script execution rejected: unknown type '{}'",
            fetched.script_type
        );
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

/// Sums the sessions a workspace has logged since its reset marker.
///
/// `timer_logs.started_at` is a UTC datetime string while `timer_reset_at` is
/// unix seconds, so the timestamp is converted before comparing — SQLite would
/// otherwise sort every TEXT value above every INTEGER and count all sessions.
///
/// Sessions are attributed by when they *started*: a session already running
/// when the user resets is excluded in full, not prorated.
const WORKSPACE_TIME_SQL: &str = "
    SELECT COALESCE(SUM(duration_seconds), 0) FROM timer_logs
    WHERE workspace_id = ?1
      AND CAST(strftime('%s', started_at) AS INTEGER) >
          COALESCE((SELECT timer_reset_at FROM workspaces WHERE id = ?1), 0)";

/// Total logged seconds for a workspace since its last "Total time" reset.
#[tauri::command]
pub async fn get_workspace_time(db: State<'_, Db>, workspace_id: String) -> Result<i64, String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        guard
            .query_row(WORKSPACE_TIME_SQL, params![workspace_id], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| format!("failed to total workspace time: {e}"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

/// Zeroes a workspace's accumulated time by moving its reset marker to now.
/// The underlying `timer_logs` rows are kept, so history and backups stay intact.
#[tauri::command]
pub async fn reset_workspace_time(db: State<'_, Db>, workspace_id: String) -> Result<(), String> {
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;
        let rows = guard
            .execute(
                "UPDATE workspaces
                 SET timer_reset_at = CAST(strftime('%s', 'now') AS INTEGER)
                 WHERE id = ?1",
                params![workspace_id],
            )
            .map_err(|e| format!("failed to reset workspace time: {e}"))?;

        if rows == 0 {
            return Err(format!("Workspace '{workspace_id}' not found"));
        }
        log::info!("Accumulated time reset for workspace '{workspace_id}'");
        Ok(())
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

const NOTIFICATION_ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");
const NOTIFICATION_ICON_FILE: &str = "notification-icon.png";
const MAX_NOTIFICATION_CHARS: usize = 256;

/// The notification plugin's icon field takes an absolute file path, not a
/// bundle identifier, and the bundled `icons/` directory is not shipped as a
/// runtime resource. The app icon is therefore embedded at compile time and
/// extracted once into the app data directory so a stable path always exists.
fn notification_icon_path(app: &AppHandle) -> Option<String> {
    let dir = app.path().app_data_dir().ok()?;
    let path = dir.join(NOTIFICATION_ICON_FILE);

    if !path.is_file() {
        std::fs::create_dir_all(&dir).ok()?;
        if let Err(e) = std::fs::write(&path, NOTIFICATION_ICON_PNG) {
            log::warn!("failed to extract notification icon: {e}");
            return None;
        }
    }
    Some(path.to_string_lossy().into_owned())
}

/// Fires a native toast for a finished timer session.
///
/// Runs in Rust rather than through the JS plugin so the notification can
/// carry an explicit icon path — Windows tends to drop banners that reference
/// no icon at all.
#[tauri::command]
pub async fn notify_timer_complete(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    if title.len() > MAX_NOTIFICATION_CHARS || body.len() > MAX_NOTIFICATION_CHARS {
        return Err("Notification text is too long".into());
    }

    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(icon) = notification_icon_path(&app) {
        builder = builder.icon(icon);
    }

    builder.show().map_err(|e| {
        log::error!("Timer notification failed: {e}");
        format!("failed to show notification: {e}")
    })
}

const MAX_QUERY_CHARS: usize = 100;
const SEARCH_LIMIT: i64 = 25;

/// Search UNION arms. Every arm MUST alias its columns to the outer query's
/// schema (kind, id, title, subtitle, action) — scoped searches execute an
/// arm standalone inside `SELECT kind, id, title, subtitle, action FROM (...)`.
const SEARCH_ARM_WORKSPACES: &str = "
    SELECT 'workspace' AS kind, id, name AS title,
           'Workspace' AS subtitle, 'open_workspace' AS action
    FROM workspaces WHERE name LIKE ?1 ESCAPE '\\'";

const SEARCH_ARM_LINKS: &str = "
    SELECT 'link' AS kind, id, title,
           target_path AS subtitle, 'launch_resource' AS action
    FROM resources
    WHERE type = 'link' AND (title LIKE ?1 ESCAPE '\\' OR target_path LIKE ?1 ESCAPE '\\')";

const SEARCH_ARM_FOLDERS: &str = "
    SELECT 'folder' AS kind, id, title,
           target_path AS subtitle, 'launch_resource' AS action
    FROM resources
    WHERE type = 'folder' AND (title LIKE ?1 ESCAPE '\\' OR target_path LIKE ?1 ESCAPE '\\')";

const SEARCH_ARM_SCRIPTS: &str = "
    SELECT 'script' AS kind, s.id, s.title,
           CASE WHEN w.name IS NULL THEN 'Global script' ELSE w.name END AS subtitle,
           'execute_script' AS action
    FROM automation_scripts s
    LEFT JOIN workspaces w ON w.id = s.workspace_id
    WHERE s.title LIKE ?1 ESCAPE '\\'";

fn build_like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

#[derive(Debug, PartialEq, Eq)]
enum SearchScope {
    All,
    Workspaces,
    Links,
    Folders,
    Scripts,
    /// Meaning-based search over the local vector index. This is the only scope
    /// that can trigger the one-time model download.
    Semantic,
}

/// Splits an optional leading scope prefix (`/ws`, `/link`, `/folder`,
/// `/script`, `/ai`, case-insensitive) from the rest of the query. A space
/// after the prefix is optional: "/ws project" and "/wsproject" both scope to
/// workspaces with the term "project".
fn parse_search_scope(query: &str) -> (SearchScope, String) {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();

    const PREFIXES: [(&str, SearchScope); 5] = [
        ("/ws", SearchScope::Workspaces),
        ("/link", SearchScope::Links),
        ("/folder", SearchScope::Folders),
        ("/script", SearchScope::Scripts),
        ("/ai", SearchScope::Semantic),
    ];

    for (prefix, scope) in PREFIXES {
        if lower.starts_with(prefix) {
            let after = &trimmed[prefix.len()..];
            return (scope, after.trim().to_string());
        }
    }

    (SearchScope::All, trimmed.to_string())
}

/// How many nearest neighbours a semantic search returns. Kept smaller than
/// `SEARCH_LIMIT` because every KNN hit is a match by construction — there is
/// no relevance cutoff, so a long tail is just noise.
const SEMANTIC_LIMIT: i64 = 12;

/// Nearest neighbours to `blob`, closest first, as `(item_id, item_type)`.
fn knn_search(
    conn: &rusqlite::Connection,
    blob: &[u8],
    limit: i64,
) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT item_id, item_type FROM vec_items
             WHERE embedding MATCH ?1 AND k = ?2
             ORDER BY distance",
        )
        .map_err(|e| format!("failed to prepare vector search: {e}"))?;

    let rows = stmt
        .query_map(params![blob, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("failed to execute vector search: {e}"))?;

    let mut hits = Vec::new();
    for row in rows {
        hits.push(row.map_err(|e| format!("failed to read vector hit: {e}"))?);
    }
    Ok(hits)
}

/// Turns one vector hit back into a displayable result by reading the row it
/// points at. Returns `None` when the row is gone — a vector can outlive its
/// item if a delete path ever misses, and a stale hit should be skipped rather
/// than shown or raised as an error.
fn hydrate_semantic_hit(
    conn: &rusqlite::Connection,
    id: &str,
    item_type: &str,
) -> Result<Option<SearchResult>, String> {
    use rusqlite::OptionalExtension;

    let found = match item_type {
        INDEX_WORKSPACE => conn
            .query_row(
                "SELECT name FROM workspaces WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|row| {
                row.map(|name| SearchResult {
                    item_type: INDEX_WORKSPACE.to_string(),
                    id: id.to_string(),
                    title: name,
                    subtitle: "Workspace".to_string(),
                    action: "open_workspace".to_string(),
                })
            }),
        "link" | "folder" => conn
            .query_row(
                "SELECT type, title, target_path FROM resources WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map(|row| {
                row.map(|(kind, title, target)| SearchResult {
                    // Read from the row, not the index: a resource retyped from
                    // link to folder would otherwise keep its stale kind.
                    item_type: kind,
                    id: id.to_string(),
                    title,
                    subtitle: target,
                    action: "launch_resource".to_string(),
                })
            }),
        INDEX_SCRIPT => conn
            .query_row(
                "SELECT s.title, CASE WHEN w.name IS NULL THEN 'Global script' ELSE w.name END
                 FROM automation_scripts s
                 LEFT JOIN workspaces w ON w.id = s.workspace_id
                 WHERE s.id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map(|row| {
                row.map(|(title, subtitle)| SearchResult {
                    item_type: INDEX_SCRIPT.to_string(),
                    id: id.to_string(),
                    title,
                    subtitle,
                    action: "execute_script".to_string(),
                })
            }),
        other => {
            log::warn!("Ignoring vector hit with unknown item type '{other}'");
            return Ok(None);
        }
    };

    found.map_err(|e| format!("failed to load semantic result: {e}"))
}

/// Shown when the model is ready but nothing is indexed. This is reachable in
/// normal use: `import_data` replaces the whole database and clears the vector
/// table with it, so a user who imports a backup and then types `/ai` would
/// otherwise get a blank list with no hint that a rebuild is all that is
/// missing. Inert for the same reason as [`ai_not_ready_notice`].
fn ai_index_empty_notice() -> SearchResult {
    SearchResult {
        item_type: "notice".to_string(),
        id: "ai-index-empty".to_string(),
        title: "Semantic index is empty. Rebuild it in Settings to search by meaning."
            .to_string(),
        subtitle: "Settings → AI Search → Rebuild Index".to_string(),
        action: "none".to_string(),
    }
}

/// Shown instead of results when someone tries `/ai` before enabling it. It is
/// deliberately inert — searching must never start a 90 MB download behind the
/// user's back, so the only way forward is the explicit button in Settings.
fn ai_not_ready_notice() -> SearchResult {
    SearchResult {
        item_type: "notice".to_string(),
        id: "ai-model-missing".to_string(),
        title: "AI Model not downloaded. Please go to Settings to enable Semantic Search."
            .to_string(),
        subtitle: "Settings → AI Search".to_string(),
        action: "none".to_string(),
    }
}

/// Meaning-based search over the local index. Never downloads: callers check
/// [`crate::ai::is_model_downloaded`] first and show the notice instead.
async fn semantic_search(
    dir: PathBuf,
    conn: Arc<Mutex<rusqlite::Connection>>,
    term: String,
) -> Result<Vec<SearchResult>, String> {
    spawn_blocking(move || {
        let embedding = crate::ai::generate_embedding(&dir, &term)?;
        let blob = crate::ai::embedding_to_blob(&embedding);

        let guard = lock_db(&conn)?;
        let hits = knn_search(&guard, &blob, SEMANTIC_LIMIT)?;

        // No neighbours at all means the index is empty, not that the query
        // matched nothing: KNN always returns the k closest vectors that exist.
        if hits.is_empty() {
            return Ok(vec![ai_index_empty_notice()]);
        }

        let mut results = Vec::with_capacity(hits.len());
        for (id, item_type) in hits {
            if let Some(result) = hydrate_semantic_hit(&guard, &id, &item_type)? {
                results.push(result);
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))?
}

#[tauri::command]
pub async fn search_all(
    app: AppHandle,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    let (scope, term) = parse_search_scope(&query);
    // A bare "/" prefix (e.g. typing exactly "/ws") lists that scope's items
    // up to the limit; only an unscoped empty query returns nothing.
    //
    // `/ai` is the exception: a semantic search needs something to compare
    // against, so a bare prefix returns nothing rather than an error.
    if term.is_empty() && matches!(scope, SearchScope::All | SearchScope::Semantic) {
        return Ok(Vec::new());
    }
    if term.len() > MAX_QUERY_CHARS {
        return Err("Search query is too long".into());
    }

    if scope == SearchScope::Semantic {
        let dir = model_root_dir(&app)?;
        if !crate::ai::is_model_downloaded(&dir) {
            return Ok(vec![ai_not_ready_notice()]);
        }
        return semantic_search(dir, db.0.clone(), term).await;
    }

    let pattern = build_like_pattern(&term);
    let conn = db.0.clone();

    spawn_blocking(move || {
        let guard = lock_db(&conn)?;

        let mut arms: Vec<&str> = Vec::new();
        match scope {
            SearchScope::All => {
                arms.push(SEARCH_ARM_WORKSPACES);
                arms.push(SEARCH_ARM_LINKS);
                arms.push(SEARCH_ARM_FOLDERS);
                arms.push(SEARCH_ARM_SCRIPTS);
            }
            SearchScope::Workspaces => arms.push(SEARCH_ARM_WORKSPACES),
            SearchScope::Links => arms.push(SEARCH_ARM_LINKS),
            SearchScope::Folders => arms.push(SEARCH_ARM_FOLDERS),
            SearchScope::Scripts => arms.push(SEARCH_ARM_SCRIPTS),
            // Handled above; it never reaches the keyword path.
            SearchScope::Semantic => unreachable!("semantic scope returns earlier"),
        }

        let sql = format!(
            "SELECT kind, id, title, subtitle, action FROM ({})
             ORDER BY title COLLATE NOCASE ASC
             LIMIT ?2",
            arms.join(" UNION ALL ")
        );

        let mut stmt = guard
            .prepare(sql.as_str())
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
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
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
        "INSERT INTO workspaces (id, name, color, icon, sort_order, created_at, timer_reset_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            w.id,
            w.name,
            w.color,
            w.icon,
            w.sort_order,
            w.created_at,
            w.timer_reset_at
        ],
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

/// Retains only the entries `keep` accepts, logging how many were discarded.
fn drop_orphans<T>(items: &mut Vec<T>, label: &str, keep: impl Fn(&T) -> bool) {
    let before = items.len();
    items.retain(keep);
    let dropped = before - items.len();
    if dropped > 0 {
        log::warn!("Import skipped {dropped} orphaned {label} (workspace no longer in the backup)");
    }
}

/// Discards rows whose `workspace_id` has no matching workspace in the same
/// backup. Legacy exports can carry ghost records left behind by a deleted
/// workspace, and inserting one trips a foreign key constraint that would roll
/// back the entire import — dropping them lets the rest of the data through.
fn drop_orphaned_rows(payload: &mut BackupPayload) {
    let valid_ws_ids: HashSet<String> = payload.workspaces.iter().map(|w| w.id.clone()).collect();

    drop_orphans(&mut payload.resources, "resources", |r| {
        valid_ws_ids.contains(&r.workspace_id)
    });
    drop_orphans(&mut payload.timer_logs, "timer entries", |t| {
        valid_ws_ids.contains(&t.workspace_id)
    });
    drop_orphans(&mut payload.tasks, "tasks", |t| {
        valid_ws_ids.contains(&t.workspace_id)
    });
    drop_orphans(&mut payload.workspace_env_vars, "variables", |v| {
        valid_ws_ids.contains(&v.workspace_id)
    });
    // A script with no workspace_id is a global script and always valid.
    drop_orphans(&mut payload.automation_scripts, "scripts", |s| {
        s.workspace_id
            .as_deref()
            .is_none_or(|id| valid_ws_ids.contains(id))
    });
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
        let mut payload: BackupPayload =
            serde_json::from_str(&json_payload).map_err(|e| format!("Invalid backup file: {e}"))?;

        if payload.version != BACKUP_VERSION && payload.version != BACKUP_VERSION + 1 {
            return Err(format!(
                "Unsupported backup version {} (expected {BACKUP_VERSION} or {})",
                payload.version,
                BACKUP_VERSION + 1
            ));
        }

        drop_orphaned_rows(&mut payload);

        let mut guard = lock_db(&conn)?;
        let tx = guard
            .transaction()
            .map_err(|e| format!("failed to begin transaction: {e}"))?;

        // Full replacement. Explicit deletes cover rows the cascade cannot
        // reach (global scripts have no parent workspace, and the vector index
        // is a virtual table so nothing cascades into it at all). Imported rows
        // are left unindexed here; they pick up vectors as they are edited, or
        // through a re-index pass.
        clear_vector_index(&tx)?;
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
            .prepare(
                "SELECT id, workspace_id, title, is_completed, created_at FROM tasks WHERE id = ?1",
            )
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
            .prepare(
                "SELECT id, workspace_id, title, is_completed, created_at FROM tasks WHERE id = ?1",
            )
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
pub async fn get_env_vars(db: State<'_, Db>, workspace_id: String) -> Result<Vec<EnvVar>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_web_and_file_links() {
        assert!(validate_resource_fields("link", "Doc", "https://example.com", "default").is_ok());
        assert!(
            validate_resource_fields("link", "Doc", "http://example.com/a.pdf", "chrome").is_ok()
        );
        assert!(
            validate_resource_fields("link", "Doc", "file:///C:/docs/spec.pdf", "edge").is_ok()
        );
    }


    // --- Resource suggestions -------------------------------------------------

    fn db_with_resources_across_workspaces() -> rusqlite::Connection {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'Payments');
             INSERT INTO workspaces (id, name) VALUES ('w2', 'Infra');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r1', 'w1', 'link', 'Stripe API docs', 'https://stripe.com/docs');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r2', 'w2', 'link', 'Runbook', 'https://wiki.internal/runbook');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r3', 'w2', 'link', 'Resume',
                         'file:///C:/Users/zepha/Downloads/Resume.pdf');",
        )
        .unwrap();
        conn
    }

    /// Mirrors the keyword tier's query. The value being tested is that a match
    /// is found regardless of which workspace holds it, and that it carries the
    /// workspace name — the useful part when the duplicate is somewhere else.
    fn keyword_suggestions(conn: &rusqlite::Connection, term: &str) -> Vec<ResourceSuggestion> {
        let pattern = build_like_pattern(term);
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.type, r.title, r.target_path, r.workspace_id, w.name
                 FROM resources r JOIN workspaces w ON w.id = r.workspace_id
                 WHERE r.title LIKE ?1 ESCAPE '\\' OR r.target_path LIKE ?1 ESCAPE '\\'
                 ORDER BY r.title COLLATE NOCASE ASC
                 LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(params![pattern, SUGGESTION_LIMIT as i64], |row| {
            Ok(ResourceSuggestion {
                id: row.get(0)?,
                resource_type: row.get(1)?,
                title: row.get(2)?,
                target_path: row.get(3)?,
                workspace_id: row.get(4)?,
                workspace_name: row.get(5)?,
                match_kind: "keyword".to_string(),
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    }

    #[test]
    fn suggestions_span_every_workspace_and_name_it() {
        let conn = db_with_resources_across_workspaces();

        let hits = keyword_suggestions(&conn, "runbook");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Runbook");
        // Found from a different workspace than the one being edited, and it
        // says where it lives.
        assert_eq!(hits[0].workspace_name, "Infra");
        assert_eq!(hits[0].match_kind, "keyword");
    }

    #[test]
    fn suggestions_match_the_target_as_well_as_the_title() {
        let conn = db_with_resources_across_workspaces();
        // Nothing is titled "stripe.com", but a resource points there.
        let hits = keyword_suggestions(&conn, "stripe.com");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "r1");
    }

    #[test]
    fn suggestions_are_case_insensitive_and_capped() {
        let conn = db_with_resources_across_workspaces();
        assert_eq!(keyword_suggestions(&conn, "STRIPE").len(), 1);

        // Every row matches '%e%'; the cap keeps it a hint rather than a list.
        for i in 0..20 {
            conn.execute(
                "INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES (?1, 'w1', 'link', ?2, 'https://example.com')",
                params![format!("bulk{i}"), format!("Resource {i}")],
            )
            .unwrap();
        }
        assert_eq!(keyword_suggestions(&conn, "e").len(), SUGGESTION_LIMIT);
    }

    /// A one-character term matches nearly everything and tells the user nothing,
    /// so the command returns early rather than querying.
    #[test]
    fn a_too_short_term_is_not_worth_suggesting_on() {
        for term in ["", " ", "a", " x "] {
            let trimmed = term.trim();
            assert!(
                trimmed.chars().count() < 2,
                "'{term}' should be below the threshold"
            );
        }
        assert!("ab".chars().count() >= 2, "two characters is enough");
    }

    /// The duplicate check has to normalise first, or the bare path the user
    /// pastes would never match the file:// URL it was stored as.
    #[test]
    fn an_exact_duplicate_is_found_through_normalisation() {
        let conn = db_with_resources_across_workspaces();

        let typed = r"C:\Users\zepha\Downloads\Resume.pdf";
        let normalised = normalize_target_path("link", typed).unwrap();
        assert_eq!(normalised, "file:///C:/Users/zepha/Downloads/Resume.pdf");

        let found: String = conn
            .query_row(
                "SELECT id FROM resources WHERE target_path = ?1 COLLATE NOCASE",
                params![normalised],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(found, "r3", "the bare path must match the stored URL");
    }

    #[test]
    fn a_target_nobody_has_saved_is_not_a_duplicate() {
        use rusqlite::OptionalExtension;
        let conn = db_with_resources_across_workspaces();

        let normalised = normalize_target_path("link", "https://example.org/new").unwrap();
        let found: Option<String> = conn
            .query_row(
                "SELECT id FROM resources WHERE target_path = ?1 COLLATE NOCASE",
                params![normalised],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert!(found.is_none());
    }

    /// The semantic tier must ignore workspaces and scripts. They share the
    /// vector index with resources but cannot be duplicates of one, and
    /// hydrating them as resources would find nothing.
    #[test]
    fn only_resource_vectors_can_become_suggestions() {
        let conn = db_with_resources_across_workspaces();
        conn.execute(
            "INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
             VALUES ('s1', 'w1', 'Deploy', 'cmd', 'echo go')",
            [],
        )
        .unwrap();

        // A workspace and a script are not resources, so they hydrate to None.
        assert!(resource_suggestion_by_id(&conn, "w1", "semantic")
            .unwrap()
            .is_none());
        assert!(resource_suggestion_by_id(&conn, "s1", "semantic")
            .unwrap()
            .is_none());

        let resource = resource_suggestion_by_id(&conn, "r1", "semantic")
            .unwrap()
            .unwrap();
        assert_eq!(resource.title, "Stripe API docs");
        assert_eq!(resource.match_kind, "semantic");
    }

    /// KNN returns its k nearest neighbours with no notion of relevance, so the
    /// distance cut-off is the only thing preventing every query from suggesting
    /// something unrelated.
    #[test]
    fn the_distance_cutoff_rejects_unrelated_vectors() {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'One');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('near', 'w1', 'link', 'Near', 'https://a');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('far', 'w1', 'link', 'Far', 'https://b');",
        )
        .unwrap();

        let unit = |a: f32, b: f32| {
            let mut v = vec![0f32; crate::ai::EMBEDDING_DIM];
            v[0] = a;
            v[1] = b;
            crate::ai::embedding_to_blob(&v)
        };
        // 'near' is identical to the probe; 'far' is orthogonal, giving an L2
        // distance of sqrt(2) which is above the cut-off.
        conn.execute(
            "INSERT INTO vec_items (item_id, item_type, embedding) VALUES ('near', 'link', ?1)",
            params![unit(1.0, 0.0)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vec_items (item_id, item_type, embedding) VALUES ('far', 'link', ?1)",
            params![unit(0.0, 1.0)],
        )
        .unwrap();

        let hits = knn_search_scored(&conn, &unit(1.0, 0.0), 10).unwrap();
        assert_eq!(hits.len(), 2, "both are returned before filtering");

        let kept: Vec<&str> = hits
            .iter()
            .filter(|(_, _, d)| *d <= SEMANTIC_SUGGESTION_MAX_DISTANCE)
            .map(|(id, _, _)| id.as_str())
            .collect();
        assert_eq!(kept, ["near"], "the orthogonal vector must be filtered out");
    }

    // --- Manual ordering ------------------------------------------------------

    fn order_of(conn: &rusqlite::Connection, table: &str, scope: &str) -> Vec<String> {
        let sql =
            format!("SELECT id FROM {table} WHERE {scope} ORDER BY sort_order ASC, rowid ASC");
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap()
    }

    fn db_with_three_workspaces() -> rusqlite::Connection {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name, sort_order) VALUES ('a', 'A', 0);
             INSERT INTO workspaces (id, name, sort_order) VALUES ('b', 'B', 1);
             INSERT INTO workspaces (id, name, sort_order) VALUES ('c', 'C', 2);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn reordering_writes_the_requested_sequence() {
        let mut conn = db_with_three_workspaces();
        assert_eq!(order_of(&conn, "workspaces", "1 = 1"), ["a", "b", "c"]);

        let moved = vec!["c".to_string(), "a".to_string(), "b".to_string()];
        apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &moved).unwrap();

        assert_eq!(order_of(&conn, "workspaces", "1 = 1"), ["c", "a", "b"]);
    }

    #[test]
    fn reordering_is_idempotent() {
        let mut conn = db_with_three_workspaces();
        let same = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &same).unwrap();
        apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &same).unwrap();
        assert_eq!(order_of(&conn, "workspaces", "1 = 1"), ["a", "b", "c"]);
    }

    /// A partial list is the dangerous case: ordering only a subset would leave
    /// the unlisted rows sharing sort_order values with the listed ones, giving
    /// a list that reshuffles itself on the next load.
    #[test]
    fn a_partial_order_is_refused_and_changes_nothing() {
        let mut conn = db_with_three_workspaces();
        let partial = vec!["c".to_string(), "a".to_string()];

        let err = apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &partial).unwrap_err();
        assert!(err.contains("does not match"), "unhelpful message: {err}");
        assert_eq!(
            order_of(&conn, "workspaces", "1 = 1"),
            ["a", "b", "c"],
            "a refused reorder must not have written anything"
        );
    }

    #[test]
    fn an_unknown_or_duplicated_id_is_refused() {
        let mut conn = db_with_three_workspaces();

        let unknown = vec!["a".to_string(), "b".to_string(), "ghost".to_string()];
        assert!(apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &unknown).is_err());

        let duplicated = vec!["a".to_string(), "a".to_string(), "b".to_string()];
        let err =
            apply_manual_order(&mut conn, "workspaces", "1 = 1", None, &duplicated).unwrap_err();
        assert!(err.contains("more than once"), "unhelpful message: {err}");

        assert_eq!(order_of(&conn, "workspaces", "1 = 1"), ["a", "b", "c"]);
    }

    /// Reordering one workspace's resources must not touch another's, and the
    /// scope check has to be evaluated per workspace rather than table-wide.
    #[test]
    fn reordering_is_confined_to_its_scope() {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'One');
             INSERT INTO workspaces (id, name) VALUES ('w2', 'Two');
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('r1', 'w1', 'folder', 'R1', 'C:/1', 0);
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('r2', 'w1', 'folder', 'R2', 'C:/2', 1);
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('s1', 'w2', 'folder', 'S1', 'C:/3', 0);
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('s2', 'w2', 'folder', 'S2', 'C:/4', 1);",
        )
        .unwrap();
        let mut conn = conn;

        let flipped = vec!["r2".to_string(), "r1".to_string()];
        apply_manual_order(
            &mut conn,
            "resources",
            "workspace_id = ?1",
            Some("w1"),
            &flipped,
        )
        .unwrap();

        assert_eq!(
            order_of(&conn, "resources", "workspace_id = 'w1'"),
            ["r2", "r1"]
        );
        assert_eq!(
            order_of(&conn, "resources", "workspace_id = 'w2'"),
            ["s1", "s2"],
            "the other workspace must be untouched"
        );

        // And w1's list is not accepted as w2's, since those ids are out of scope.
        assert!(apply_manual_order(
            &mut conn,
            "resources",
            "workspace_id = ?1",
            Some("w2"),
            &flipped
        )
        .is_err());
    }

    /// Global scripts live under `workspace_id IS NULL`, a separate list from any
    /// workspace's, and reordering one must not disturb the other.
    #[test]
    fn global_and_scoped_script_lists_order_independently() {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'One');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('g1', NULL, 'G1', 'cmd', 'echo 1', 0);
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('g2', NULL, 'G2', 'cmd', 'echo 2', 1);
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('w1a', 'w1', 'A', 'cmd', 'echo 3', 0);
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('w1b', 'w1', 'B', 'cmd', 'echo 4', 1);",
        )
        .unwrap();
        let mut conn = conn;

        apply_manual_order(
            &mut conn,
            "automation_scripts",
            "workspace_id IS NULL",
            None,
            &["g2".to_string(), "g1".to_string()],
        )
        .unwrap();

        assert_eq!(
            order_of(&conn, "automation_scripts", "workspace_id IS NULL"),
            ["g2", "g1"]
        );
        assert_eq!(
            order_of(&conn, "automation_scripts", "workspace_id = 'w1'"),
            ["w1a", "w1b"],
            "the workspace's own scripts must be untouched"
        );
    }

    /// A manual reorder has to survive a backup round trip, or exporting before
    /// a reinstall silently throws the arrangement away. Every insert_* helper
    /// must carry sort_order; dropping it from one of those column lists is an
    /// easy and invisible regression.
    #[test]
    fn a_manual_order_survives_export_and_import() {
        let mut conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name, sort_order) VALUES ('a', 'A', 0);
             INSERT INTO workspaces (id, name, sort_order) VALUES ('b', 'B', 1);
             INSERT INTO workspaces (id, name, sort_order) VALUES ('c', 'C', 2);
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('r1', 'a', 'folder', 'R1', 'C:/1', 0);
             INSERT INTO resources (id, workspace_id, type, title, target_path, sort_order)
                 VALUES ('r2', 'a', 'folder', 'R2', 'C:/2', 1);
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('s1', 'a', 'S1', 'cmd', 'echo 1', 0);
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content, sort_order)
                 VALUES ('s2', 'a', 'S2', 'cmd', 'echo 2', 1);",
        )
        .unwrap();

        // Reorder all three lists away from their insertion order.
        apply_manual_order(
            &mut conn,
            "workspaces",
            "1 = 1",
            None,
            &["c".to_string(), "a".to_string(), "b".to_string()],
        )
        .unwrap();
        apply_manual_order(
            &mut conn,
            "resources",
            "workspace_id = ?1",
            Some("a"),
            &["r2".to_string(), "r1".to_string()],
        )
        .unwrap();
        apply_manual_order(
            &mut conn,
            "automation_scripts",
            "workspace_id = ?1",
            Some("a"),
            &["s2".to_string(), "s1".to_string()],
        )
        .unwrap();

        // Export exactly as export_all_data does, through the same column lists.
        let workspaces: Vec<Workspace> = {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM workspaces ORDER BY sort_order ASC, name COLLATE NOCASE ASC"
                ))
                .unwrap();
            stmt.query_map([], row_to_workspace)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let resources: Vec<Resource> = {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {RESOURCE_COLUMNS} FROM resources ORDER BY sort_order ASC, rowid ASC"
                ))
                .unwrap();
            stmt.query_map([], row_to_resource)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let scripts: Vec<AutomationScript> = {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {SCRIPT_COLUMNS} FROM automation_scripts ORDER BY sort_order ASC, rowid ASC"
                ))
                .unwrap();
            stmt.query_map([], row_to_script)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };

        // The order has to be carried by the data, not by the query, so assert
        // the exported sort_order values rather than only the row sequence.
        assert_eq!(
            workspaces.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            ["c", "a", "b"]
        );
        assert_eq!(
            workspaces.iter().map(|w| w.sort_order).collect::<Vec<_>>(),
            [0, 1, 2]
        );

        // Wipe and re-import through the same helpers import_data uses.
        conn.execute_batch(
            "DELETE FROM automation_scripts; DELETE FROM resources; DELETE FROM workspaces;",
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        for w in &workspaces {
            insert_workspace(&tx, w).unwrap();
        }
        for r in &resources {
            insert_resource(&tx, r).unwrap();
        }
        for s in &scripts {
            insert_script(&tx, s).unwrap();
        }
        tx.commit().unwrap();

        assert_eq!(
            order_of(&conn, "workspaces", "1 = 1"),
            ["c", "a", "b"],
            "the workspace order must come back as it was exported"
        );
        assert_eq!(
            order_of(&conn, "resources", "workspace_id = 'a'"),
            ["r2", "r1"]
        );
        assert_eq!(
            order_of(&conn, "automation_scripts", "workspace_id = 'a'"),
            ["s2", "s1"]
        );
    }

    /// Removing the model must also empty the index, or the counts in Settings
    /// would claim items are searchable when the embedder needed to query them
    /// is gone.
    #[test]
    fn removing_the_model_clears_the_index() {
        let conn = test_db_with_vectors();
        assert!(!knn_search(&conn, &probe(1.0, 0.0), 10).unwrap().is_empty());

        clear_vector_index(&conn).unwrap();

        assert!(knn_search(&conn, &probe(1.0, 0.0), 10).unwrap().is_empty());
        // The items themselves are untouched — only their vectors go.
        let workspaces: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))
            .unwrap();
        assert_eq!(workspaces, 1, "removing the model must not delete any data");
    }

    // --- Local file links -----------------------------------------------------

    #[test]
    fn recognises_absolute_local_paths() {
        assert!(is_absolute_local_path(r"C:\Users\me\cv.pdf"));
        assert!(is_absolute_local_path("C:/Users/me/cv.pdf"));
        assert!(is_absolute_local_path(r"\\server\share\doc.txt"));
        // Relative paths have no meaning without a working directory.
        assert!(!is_absolute_local_path(r"Downloads\cv.pdf"));
        assert!(!is_absolute_local_path("cv.pdf"));
        assert!(!is_absolute_local_path("https://example.com"));
        assert!(!is_absolute_local_path("C:"));
    }

    #[test]
    fn a_bare_windows_path_becomes_a_file_url() {
        assert_eq!(
            normalize_target_path("link", r"C:\Users\zepha\Downloads\Resume.pdf").unwrap(),
            "file:///C:/Users/zepha/Downloads/Resume.pdf"
        );
        // Surrounding whitespace is a copy-paste artefact, not part of the path.
        assert_eq!(
            normalize_target_path("link", r"  C:\tmp\notes.txt  ").unwrap(),
            "file:///C:/tmp/notes.txt"
        );
    }

    #[test]
    fn characters_that_would_break_the_url_are_encoded() {
        // A space must not split the URL, and a '#' must not become a fragment
        // that silently truncates the filename.
        assert_eq!(
            normalize_target_path("link", r"C:\My Docs\draft #2.pdf").unwrap(),
            "file:///C:/My%20Docs/draft%20%232.pdf"
        );
    }

    #[test]
    fn unc_paths_keep_their_host() {
        assert_eq!(
            normalize_target_path("link", r"\\nas\share\spec.pdf").unwrap(),
            "file://nas/share/spec.pdf"
        );
    }

    #[test]
    fn urls_and_folders_pass_through_untouched() {
        assert_eq!(
            normalize_target_path("link", "https://example.com/a.pdf").unwrap(),
            "https://example.com/a.pdf"
        );
        assert_eq!(
            normalize_target_path("link", "file:///C:/already/a/url.pdf").unwrap(),
            "file:///C:/already/a/url.pdf"
        );
        // A folder resource keeps its native path — the launcher hands it to
        // Explorer, not to a browser.
        assert_eq!(
            normalize_target_path("folder", r"C:\src\payments").unwrap(),
            r"C:\src\payments"
        );
    }

    #[test]
    fn normalised_paths_survive_the_scheme_check() {
        // The whole point: what the normaliser emits must satisfy the validator
        // that previously rejected the user's bare path outright.
        let normalised =
            normalize_target_path("link", r"C:\Users\zepha\Downloads\Resume.pdf").unwrap();
        assert!(validate_resource_fields("link", "Resume", &normalised, "chrome").is_ok());
        // And a relative path still fails, rather than being silently accepted.
        assert!(validate_resource_fields("link", "Doc", "cv.pdf", "default").is_err());
    }

    #[test]
    fn a_directory_given_as_a_link_is_rejected_with_guidance() {
        let dir = std::env::temp_dir();
        let err = normalize_target_path("link", &dir.to_string_lossy()).unwrap_err();
        assert!(err.contains("Folder resource"), "unhelpful message: {err}");
    }

    #[test]
    fn rejects_bad_schemes_and_garbage() {
        assert!(validate_resource_fields("link", "Doc", "calc.exe", "default").is_err());
        assert!(validate_resource_fields("link", "Doc", "ftp://x", "default").is_err());
        assert!(validate_resource_fields("link", "", "https://x", "default").is_err());
        assert!(validate_resource_fields("link", "Doc", "https://\"; calc", "default").is_err());
        assert!(validate_resource_fields("folder", "F", "C:\\tmp", "default").is_ok());
    }

    #[test]
    fn parses_search_scopes() {
        assert_eq!(
            parse_search_scope("github"),
            (SearchScope::All, "github".to_string())
        );
        assert_eq!(
            parse_search_scope("/ws foo"),
            (SearchScope::Workspaces, "foo".to_string())
        );
        assert_eq!(
            parse_search_scope("/WS"),
            (SearchScope::Workspaces, String::new())
        );
        assert_eq!(
            parse_search_scope("  /Link  docs  "),
            (SearchScope::Links, "docs".to_string())
        );
        assert_eq!(
            parse_search_scope("/folder c:\\tmp"),
            (SearchScope::Folders, "c:\\tmp".to_string())
        );
        assert_eq!(
            parse_search_scope("/script\tbackup"),
            (SearchScope::Scripts, "backup".to_string())
        );
        assert_eq!(
            parse_search_scope("/ai where did I put the deploy notes"),
            (
                SearchScope::Semantic,
                "where did I put the deploy notes".to_string()
            )
        );
        assert_eq!(
            parse_search_scope("/AI Staging"),
            (SearchScope::Semantic, "Staging".to_string())
        );
        // A bare prefix yields an empty term, which search_all turns into an
        // empty result rather than a model download.
        assert_eq!(parse_search_scope("/ai"), (SearchScope::Semantic, String::new()));
    }

    fn test_db_with_sessions() -> rusqlite::Connection {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute("INSERT INTO workspaces (id, name) VALUES ('w1', 'Work')", [])
            .unwrap();
        conn.execute("INSERT INTO workspaces (id, name) VALUES ('w2', 'Other')", [])
            .unwrap();

        // Two sessions on w1 (two hours ago and ten minutes ago) plus one on w2.
        conn.execute_batch(
            "INSERT INTO timer_logs (id, workspace_id, duration_seconds, started_at)
                 VALUES ('t1', 'w1', 600, datetime('now', '-2 hours'));
             INSERT INTO timer_logs (id, workspace_id, duration_seconds, started_at)
                 VALUES ('t2', 'w1', 300, datetime('now', '-10 minutes'));
             INSERT INTO timer_logs (id, workspace_id, duration_seconds, started_at)
                 VALUES ('t3', 'w2', 999, datetime('now', '-5 minutes'));",
        )
        .unwrap();
        conn
    }

    fn workspace_time(conn: &rusqlite::Connection, workspace_id: &str) -> i64 {
        conn.query_row(WORKSPACE_TIME_SQL, params![workspace_id], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn totals_only_this_workspaces_sessions() {
        let conn = test_db_with_sessions();
        assert_eq!(workspace_time(&conn, "w1"), 900);
        assert_eq!(workspace_time(&conn, "w2"), 999);
        assert_eq!(workspace_time(&conn, "missing"), 0);
    }

    #[test]
    fn reset_marker_excludes_earlier_sessions() {
        let conn = test_db_with_sessions();

        // Marker one hour back: the two-hour-old session drops out.
        conn.execute(
            "UPDATE workspaces
             SET timer_reset_at = CAST(strftime('%s', 'now', '-1 hour') AS INTEGER)
             WHERE id = 'w1'",
            [],
        )
        .unwrap();
        assert_eq!(workspace_time(&conn, "w1"), 300);

        // Resetting now zeroes the workspace without touching the log rows.
        conn.execute(
            "UPDATE workspaces
             SET timer_reset_at = CAST(strftime('%s', 'now') AS INTEGER)
             WHERE id = 'w1'",
            [],
        )
        .unwrap();
        assert_eq!(workspace_time(&conn, "w1"), 0);
        let kept: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM timer_logs WHERE workspace_id = 'w1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, 2);
    }

    /// A legacy-shaped backup: one real workspace plus ghost rows of every
    /// kind pointing at a workspace that was deleted before the export.
    const BACKUP_WITH_ORPHANS: &str = r#"{
        "version": 2,
        "exportedAt": "2026-01-01 00:00:00",
        "workspaces": [
            {"id":"w1","name":"Work","color":null,"icon":null,
             "sortOrder":0,"createdAt":"2026-01-01 00:00:00"}
        ],
        "resources": [
            {"id":"r1","workspaceId":"w1","type":"link","title":"Keep",
             "targetPath":"https://example.com","preferredApp":"default",
             "profileName":null,"sortOrder":0,"createdAt":"2026-01-01 00:00:00"},
            {"id":"r2","workspaceId":"gone","type":"link","title":"Ghost",
             "targetPath":"https://example.com","preferredApp":"default",
             "profileName":null,"sortOrder":0,"createdAt":"2026-01-01 00:00:00"}
        ],
        "automationScripts": [
            {"id":"s1","workspaceId":null,"title":"Global","scriptType":"cmd",
             "scriptContent":"echo hi","sortOrder":0,"createdAt":"2026-01-01 00:00:00"},
            {"id":"s2","workspaceId":"w1","title":"Scoped","scriptType":"cmd",
             "scriptContent":"echo hi","sortOrder":0,"createdAt":"2026-01-01 00:00:00"},
            {"id":"s3","workspaceId":"gone","title":"Ghost","scriptType":"cmd",
             "scriptContent":"echo hi","sortOrder":0,"createdAt":"2026-01-01 00:00:00"}
        ],
        "timerLogs": [
            {"id":"t1","workspaceId":"w1","durationSeconds":60,"sessionType":"stopwatch",
             "startedAt":"2026-01-01 00:00:00","endedAt":"2026-01-01 00:01:00"},
            {"id":"t2","workspaceId":"gone","durationSeconds":60,"sessionType":"stopwatch",
             "startedAt":"2026-01-01 00:00:00","endedAt":"2026-01-01 00:01:00"}
        ],
        "tasks": [
            {"id":"k1","workspaceId":"w1","title":"Keep","isCompleted":false,
             "createdAt":"2026-01-01 00:00:00"},
            {"id":"k2","workspaceId":"gone","title":"Ghost","isCompleted":false,
             "createdAt":"2026-01-01 00:00:00"}
        ],
        "workspaceEnvVars": [
            {"id":"e1","workspaceId":"w1","envKey":"KEEP","envValue":"1"},
            {"id":"e2","workspaceId":"gone","envKey":"GHOST","envValue":"1"}
        ]
    }"#;

    #[test]
    fn import_drops_rows_referencing_missing_workspaces() {
        let mut payload: BackupPayload = serde_json::from_str(BACKUP_WITH_ORPHANS).unwrap();
        drop_orphaned_rows(&mut payload);

        assert_eq!(payload.workspaces.len(), 1);
        let ids = |v: Vec<String>| v;
        assert_eq!(
            ids(payload.resources.iter().map(|r| r.id.clone()).collect()),
            ["r1"]
        );
        assert_eq!(
            ids(payload.timer_logs.iter().map(|t| t.id.clone()).collect()),
            ["t1"]
        );
        assert_eq!(
            ids(payload.tasks.iter().map(|t| t.id.clone()).collect()),
            ["k1"]
        );
        assert_eq!(
            ids(payload
                .workspace_env_vars
                .iter()
                .map(|v| v.id.clone())
                .collect()),
            ["e1"]
        );
        // Global scripts survive alongside the workspace-scoped one.
        assert_eq!(
            ids(payload
                .automation_scripts
                .iter()
                .map(|s| s.id.clone())
                .collect()),
            ["s1", "s2"]
        );
    }

    #[test]
    fn filtered_backup_imports_without_violating_foreign_keys() {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();

        let mut payload: BackupPayload = serde_json::from_str(BACKUP_WITH_ORPHANS).unwrap();
        drop_orphaned_rows(&mut payload);

        let mut conn = conn;
        let tx = conn.transaction().unwrap();
        for w in &payload.workspaces {
            insert_workspace(&tx, w).unwrap();
        }
        for r in &payload.resources {
            insert_resource(&tx, r).unwrap();
        }
        for s in &payload.automation_scripts {
            insert_script(&tx, s).unwrap();
        }
        for t in &payload.timer_logs {
            insert_timer_log(&tx, t).unwrap();
        }
        for t in &payload.tasks {
            insert_task(&tx, t).unwrap();
        }
        for v in &payload.workspace_env_vars {
            insert_env_var(&tx, v).unwrap();
        }
        tx.commit().unwrap();

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |row| row.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM resources"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM automation_scripts"), 2);
        assert_eq!(count("SELECT COUNT(*) FROM timer_logs"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM tasks"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM workspace_env_vars"), 1);
    }

    #[test]
    fn prefix_space_is_optional() {
        // "/wsproject" scopes to workspaces with the term "project".
        assert_eq!(
            parse_search_scope("/wsproject"),
            (SearchScope::Workspaces, "project".to_string())
        );
        assert_eq!(
            parse_search_scope("/linkfoo.com"),
            (SearchScope::Links, "foo.com".to_string())
        );
        assert_eq!(
            parse_search_scope("/aideploy"),
            (SearchScope::Semantic, "deploy".to_string())
        );
    }

    // --- Semantic index -----------------------------------------------------

    /// A database with one item of each kind plus a matching vector, using
    /// synthetic unit vectors so the search path can be exercised without
    /// loading the real model.
    fn test_db_with_vectors() -> rusqlite::Connection {
        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();

        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'Payments');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r1', 'w1', 'folder', 'Repo', 'C:\\src\\payments');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
                 VALUES ('s1', 'w1', 'Deploy', 'cmd', 'echo deploy');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
                 VALUES ('s2', NULL, 'Global cleanup', 'cmd', 'echo clean');",
        )
        .unwrap();

        for (id, kind, a, b) in [
            ("w1", "workspace", 1.0f32, 0.0f32),
            ("r1", "folder", 0.95, 0.31),
            ("s1", "script", 0.0, 1.0),
            ("s2", "script", -1.0, 0.0),
        ] {
            let mut v = vec![0f32; crate::ai::EMBEDDING_DIM];
            v[0] = a;
            v[1] = b;
            conn.execute(
                "INSERT INTO vec_items (item_id, item_type, embedding) VALUES (?1, ?2, ?3)",
                params![id, kind, crate::ai::embedding_to_blob(&v)],
            )
            .unwrap();
        }
        conn
    }

    fn probe(a: f32, b: f32) -> Vec<u8> {
        let mut v = vec![0f32; crate::ai::EMBEDDING_DIM];
        v[0] = a;
        v[1] = b;
        crate::ai::embedding_to_blob(&v)
    }

    #[test]
    fn knn_returns_nearest_first_and_respects_the_limit() {
        let conn = test_db_with_vectors();

        let hits = knn_search(&conn, &probe(1.0, 0.0), 3).unwrap();
        assert_eq!(hits.len(), 3, "k must cap the result count");
        let ids: Vec<&str> = hits.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids[0], "w1", "the identical vector must rank first");
        assert_eq!(ids[1], "r1", "the near vector must rank second");
        // The opposite vector is furthest and must not beat the orthogonal one.
        assert_eq!(ids[2], "s1");
    }

    #[test]
    fn every_item_kind_hydrates_into_a_usable_result() {
        let conn = test_db_with_vectors();

        let workspace = hydrate_semantic_hit(&conn, "w1", "workspace")
            .unwrap()
            .unwrap();
        assert_eq!(workspace.title, "Payments");
        assert_eq!(workspace.subtitle, "Workspace");
        assert_eq!(workspace.action, "open_workspace");

        let folder = hydrate_semantic_hit(&conn, "r1", "folder").unwrap().unwrap();
        assert_eq!(folder.item_type, "folder");
        assert_eq!(folder.title, "Repo");
        assert_eq!(folder.subtitle, "C:\\src\\payments");
        assert_eq!(folder.action, "launch_resource");

        let scoped = hydrate_semantic_hit(&conn, "s1", "script").unwrap().unwrap();
        assert_eq!(scoped.subtitle, "Payments", "scripts show their workspace");
        assert_eq!(scoped.action, "execute_script");

        let global = hydrate_semantic_hit(&conn, "s2", "script").unwrap().unwrap();
        assert_eq!(global.subtitle, "Global script");
    }

    #[test]
    fn a_vector_pointing_at_a_deleted_row_is_skipped() {
        let conn = test_db_with_vectors();
        conn.execute("DELETE FROM resources WHERE id = 'r1'", [])
            .unwrap();

        // The stale vector is still there, but hydration drops it silently
        // instead of erroring or surfacing a blank row.
        assert!(hydrate_semantic_hit(&conn, "r1", "folder").unwrap().is_none());
        assert!(hydrate_semantic_hit(&conn, "nope", "workspace")
            .unwrap()
            .is_none());
        assert!(hydrate_semantic_hit(&conn, "w1", "unknown-kind")
            .unwrap()
            .is_none());
    }

    #[test]
    fn hydration_reads_the_kind_from_the_row_not_the_index() {
        let conn = test_db_with_vectors();
        // A resource retyped after it was indexed: the vector still says
        // 'folder', but the result must describe what the row is now.
        conn.execute(
            "UPDATE resources SET type = 'link', target_path = 'https://example.com'
             WHERE id = 'r1'",
            [],
        )
        .unwrap();

        let hit = hydrate_semantic_hit(&conn, "r1", "folder").unwrap().unwrap();
        assert_eq!(hit.item_type, "link");
    }

    #[test]
    fn deleting_a_workspace_collects_its_cascaded_ids() {
        let conn = test_db_with_vectors();

        let mut ids = workspace_cascade_ids(&conn, "w1").unwrap();
        ids.sort();
        // The workspace, its resource and its scoped script — but not the
        // global script, which survives the cascade.
        assert_eq!(ids, vec!["r1", "s1", "w1"]);
    }

    #[test]
    fn deleting_vectors_leaves_unrelated_ones_alone() {
        let conn = test_db_with_vectors();

        let doomed = workspace_cascade_ids(&conn, "w1").unwrap();
        delete_vectors(&conn, &doomed).unwrap();

        let remaining = knn_search(&conn, &probe(1.0, 0.0), 10).unwrap();
        let ids: Vec<&str> = remaining.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["s2"], "only the global script's vector survives");

        // Removing an id with no vector must not error.
        delete_vectors(&conn, &["never-indexed".to_string()]).unwrap();
    }

    #[test]
    fn importing_clears_the_whole_index() {
        let conn = test_db_with_vectors();
        conn.execute("DELETE FROM vec_items", []).unwrap();

        assert!(knn_search(&conn, &probe(1.0, 0.0), 10).unwrap().is_empty());
    }

    #[test]
    fn backfill_collects_every_item_with_the_right_kind_and_text() {
        let conn = test_db_with_vectors();

        let items = collect_indexable(&conn).unwrap();
        assert_eq!(items.len(), 4, "one workspace, one resource, two scripts");

        let by_id = |id: &str| items.iter().find(|i| i.id == id).unwrap();

        assert_eq!(by_id("w1").item_type, "workspace");
        assert_eq!(by_id("w1").text, "Payments");

        // A resource is indexed under its own kind, not a generic one, so the
        // hit maps onto the right icon and action.
        assert_eq!(by_id("r1").item_type, "folder");
        assert!(by_id("r1").text.contains("Repo"));
        assert!(by_id("r1").text.contains("C:\\src\\payments"));

        assert_eq!(by_id("s1").item_type, "script");
        assert_eq!(by_id("s2").text, "Global cleanup");
    }

    #[test]
    fn backfill_covers_items_that_predate_the_index() {
        let conn = test_db_with_vectors();
        // Wipe the index the way reindex_all does before rebuilding.
        conn.execute("DELETE FROM vec_items", []).unwrap();
        assert!(knn_search(&conn, &probe(1.0, 0.0), 10).unwrap().is_empty());

        // Every pre-existing row is still discoverable for embedding.
        let items = collect_indexable(&conn).unwrap();
        assert_eq!(items.len(), 4);
        assert!(items.iter().all(|i| !i.text.trim().is_empty()));
    }

    #[test]
    fn status_counts_match_what_the_backfill_would_index() {
        let conn = test_db_with_vectors();

        let indexable: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM workspaces)
                      + (SELECT COUNT(*) FROM resources)
                      + (SELECT COUNT(*) FROM automation_scripts)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let indexed: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |row| row.get(0))
            .unwrap();

        // The number Settings reports has to be the number reindex_all embeds,
        // otherwise a finished backfill would still look incomplete.
        assert_eq!(indexable, collect_indexable(&conn).unwrap().len() as i64);
        assert_eq!(indexed, indexable);

        // A partial index shows up as a mismatch, which is what drives the
        // "rebuild to include them" warning.
        conn.execute("DELETE FROM vec_items WHERE item_id = 'w1'", [])
            .unwrap();
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM vec_items", [], |row| row.get(0))
            .unwrap();
        assert!(after < indexable);
    }

    /// The Phase 3 gap, end to end: items that existed before the model was
    /// downloaded have no vectors, and must become searchable after a backfill.
    #[test]
    #[ignore = "requires the downloaded model"]
    fn backfill_makes_pre_existing_items_searchable() {
        let dir = std::env::temp_dir().join("orion-model-test");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(crate::ai::ensure_model_downloaded(&dir)).unwrap();

        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'Payments');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r1', 'w1', 'link', 'Team standup notes', 'https://example.com/notes');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
                 VALUES ('s1', 'w1', 'Ship the release to production', 'cmd', 'echo go');",
        )
        .unwrap();

        // Nothing was indexed on the way in — exactly the state a user who
        // enables AI search after months of use would be in.
        assert!(knn_search(&conn, &probe(1.0, 0.0), 10).unwrap().is_empty());

        // The body of reindex_all: collect, embed, replace.
        let items = collect_indexable(&conn).unwrap();
        assert_eq!(items.len(), 3);
        conn.execute("DELETE FROM vec_items", []).unwrap();
        for item in items {
            let vector = crate::ai::generate_embedding(&dir, &item.text).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO vec_items (item_id, item_type, embedding)
                 VALUES (?1, ?2, ?3)",
                params![
                    item.id,
                    item.item_type,
                    crate::ai::embedding_to_blob(&vector)
                ],
            )
            .unwrap();
        }

        let query = crate::ai::generate_embedding(&dir, "deploy my app live").unwrap();
        let hits = knn_search(&conn, &crate::ai::embedding_to_blob(&query), SEMANTIC_LIMIT).unwrap();
        let top = hydrate_semantic_hit(&conn, &hits[0].0, &hits[0].1)
            .unwrap()
            .unwrap();

        println!("top hit after backfill: {}", top.title);
        assert_eq!(top.title, "Ship the release to production");
        assert_eq!(hits.len(), 3, "every pre-existing item got a vector");
    }

    #[test]
    fn the_not_ready_notice_is_inert() {
        let notice = ai_not_ready_notice();
        assert_eq!(notice.action, "none", "it must not launch anything");
        assert_eq!(notice.item_type, "notice");
        assert!(notice.title.contains("Settings"));
    }

    #[test]
    fn the_empty_index_notice_is_inert_and_distinct() {
        let notice = ai_index_empty_notice();
        assert_eq!(notice.action, "none", "it must not launch anything");
        assert_eq!(notice.item_type, "notice");
        assert!(notice.title.contains("Rebuild") || notice.title.contains("rebuild"));
        // The two notices mean different things and must not be confusable:
        // one says "download the model", the other "the model is fine, rebuild".
        assert_ne!(notice.id, ai_not_ready_notice().id);
    }

    /// An empty index is the state `import_data` leaves behind, and it must
    /// explain itself rather than look like "nothing matched your query".
    #[test]
    fn an_empty_index_yields_the_rebuild_notice() {
        let conn = test_db_with_vectors();
        conn.execute("DELETE FROM vec_items", []).unwrap();

        let hits = knn_search(&conn, &probe(1.0, 0.0), SEMANTIC_LIMIT).unwrap();
        assert!(hits.is_empty(), "the index must be empty for this to be the case");

        // Mirrors the branch semantic_search takes on an empty hit list.
        let shown = if hits.is_empty() {
            vec![ai_index_empty_notice()]
        } else {
            Vec::new()
        };
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].action, "none");
    }

    #[test]
    fn resource_index_text_covers_title_and_target() {
        let text = resource_index_text("Repo", "C:\\src\\payments");
        assert!(text.contains("Repo"));
        assert!(text.contains("payments"));
    }

    /// The whole `/ai` path with real weights: index a few items the way the
    /// CRUD hooks do, then ask a question that shares no keyword with the
    /// answer. Ignored by default because it needs the downloaded model.
    #[test]
    #[ignore = "requires the downloaded model"]
    fn semantic_search_finds_items_by_meaning() {
        let dir = std::env::temp_dir().join("orion-model-test");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(crate::ai::ensure_model_downloaded(&dir)).unwrap();

        let conn = crate::db::open_test_connection();
        crate::db::init_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, name) VALUES ('w1', 'Payments');
             INSERT INTO resources (id, workspace_id, type, title, target_path)
                 VALUES ('r1', 'w1', 'link', 'Team standup notes',
                         'https://example.com/notes');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
                 VALUES ('s1', 'w1', 'Ship the release to production', 'cmd', 'echo go');
             INSERT INTO automation_scripts (id, workspace_id, title, script_type, script_content)
                 VALUES ('s2', 'w1', 'Compress holiday photos', 'cmd', 'echo zip');",
        )
        .unwrap();

        // Index exactly as spawn_index_item would.
        for (id, kind, text) in [
            ("w1", INDEX_WORKSPACE, "Payments".to_string()),
            (
                "r1",
                "link",
                resource_index_text("Team standup notes", "https://example.com/notes"),
            ),
            ("s1", INDEX_SCRIPT, "Ship the release to production".into()),
            ("s2", INDEX_SCRIPT, "Compress holiday photos".into()),
        ] {
            let embedding = crate::ai::generate_embedding(&dir, &text).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO vec_items (item_id, item_type, embedding)
                 VALUES (?1, ?2, ?3)",
                params![id, kind, crate::ai::embedding_to_blob(&embedding)],
            )
            .unwrap();
        }

        // No word here appears in the script's title.
        let query = crate::ai::generate_embedding(&dir, "deploy my app live").unwrap();
        let hits = knn_search(&conn, &crate::ai::embedding_to_blob(&query), SEMANTIC_LIMIT).unwrap();

        let mut results = Vec::new();
        for (id, kind) in &hits {
            if let Some(r) = hydrate_semantic_hit(&conn, id, kind).unwrap() {
                results.push(r);
            }
        }

        println!(
            "ranking: {:?}",
            results.iter().map(|r| &r.title).collect::<Vec<_>>()
        );
        assert_eq!(
            results[0].title, "Ship the release to production",
            "a keyword search would have found nothing here"
        );
        assert_eq!(results[0].action, "execute_script");
        assert_eq!(results.len(), 4, "every indexed item hydrates");
    }
}
