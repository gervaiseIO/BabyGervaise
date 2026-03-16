use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::Connection;

#[derive(Clone)]
pub struct SqliteMemoryDb {
    db_path: Arc<PathBuf>,
}

impl SqliteMemoryDb {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).context("failed to create database parent directory")?;
        }

        let db = Self {
            db_path: Arc::new(db_path),
        };
        db.ensure_schema()?;
        Ok(db)
    }

    pub fn connection(&self) -> Result<Connection> {
        let conn = Connection::open(&*self.db_path).context("failed to open SQLite database")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .context("failed to initialize SQLite pragmas")?;
        Ok(conn)
    }

    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }

    fn ensure_schema(&self) -> Result<()> {
        let conn = self.connection()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                input_source TEXT NOT NULL,
                created_at TEXT NOT NULL,
                content_type TEXT NOT NULL DEFAULT 'plain_text',
                display_json TEXT,
                visible_summary TEXT,
                meta_json TEXT
            );

            CREATE TABLE IF NOT EXISTS memory_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                text TEXT NOT NULL,
                salience REAL NOT NULL,
                vector BLOB NOT NULL,
                vector_dim INTEGER NOT NULL,
                vector_version INTEGER NOT NULL,
                source_message_id INTEGER,
                created_at TEXT NOT NULL,
                canonical_key TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                source_type TEXT,
                source_ref TEXT,
                supersedes_memory_id INTEGER,
                last_recalled_at TEXT
            );

            CREATE TABLE IF NOT EXISTS model_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                model_name TEXT NOT NULL,
                prompt TEXT NOT NULL,
                raw_output TEXT NOT NULL,
                input_tokens INTEGER,
                output_tokens INTEGER,
                latency_ms INTEGER NOT NULL,
                http_status INTEGER,
                error_text TEXT
            );

            CREATE TABLE IF NOT EXISTS tool_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                action TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                result_json TEXT NOT NULL,
                success INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS retrieval_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                level TEXT NOT NULL,
                recent_count INTEGER NOT NULL,
                semantic_count INTEGER NOT NULL,
                query_text TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS note_activity_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                note_key TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                title_snapshot TEXT NOT NULL,
                event_type TEXT NOT NULL,
                occurred_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tool_state (
                tool_name TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS turns (
                turn_id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                user_input_summary TEXT NOT NULL,
                input_source TEXT NOT NULL DEFAULT 'text',
                plan_kind TEXT NOT NULL,
                fallback_plan_kind TEXT,
                context_policy TEXT,
                model_stages_json TEXT NOT NULL DEFAULT '[]',
                memory_used INTEGER NOT NULL DEFAULT 0,
                tool_consulted INTEGER NOT NULL DEFAULT 0,
                tool_used INTEGER NOT NULL DEFAULT 0,
                nano_first_beat_used INTEGER NOT NULL DEFAULT 0,
                cloud_escalated INTEGER NOT NULL DEFAULT 0,
                cloud_used INTEGER NOT NULL DEFAULT 0,
                selected_cloud_profile TEXT,
                delivery_mode TEXT NOT NULL DEFAULT 'PENDING',
                final_route TEXT NOT NULL DEFAULT 'pending',
                error_summary TEXT,
                total_latency_ms INTEGER NOT NULL DEFAULT 0,
                final_visible_output TEXT NOT NULL DEFAULT '',
                had_fallback INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS trace_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_id TEXT NOT NULL,
                stage_seq INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                category TEXT NOT NULL,
                name TEXT NOT NULL,
                plan_kind TEXT,
                fallback_plan_kind TEXT,
                reason_codes_json TEXT,
                context_policy TEXT,
                prompt_mode TEXT,
                lane TEXT,
                provider TEXT,
                model TEXT,
                status TEXT,
                latency_ms INTEGER,
                http_status INTEGER,
                error_text TEXT,
                displayed_text TEXT,
                selected_refs_json TEXT
            );

            CREATE TABLE IF NOT EXISTS trace_payloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id INTEGER NOT NULL,
                slot TEXT NOT NULL,
                visibility_class TEXT NOT NULL,
                content_format TEXT NOT NULL,
                content_text TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                FOREIGN KEY(event_id) REFERENCES trace_events(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS working_memory_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_id TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                version INTEGER NOT NULL,
                focus_thread_key TEXT,
                thread_count INTEGER NOT NULL,
                open_loop_count INTEGER NOT NULL,
                state_json TEXT NOT NULL
            );
            "#,
        )
        .context("failed to create SQLite schema")?;

        self.ensure_column(
            &conn,
            "tool_logs",
            "latency_ms",
            "ALTER TABLE tool_logs ADD COLUMN latency_ms INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "messages",
            "content_type",
            "ALTER TABLE messages ADD COLUMN content_type TEXT NOT NULL DEFAULT 'plain_text'",
        )?;
        self.ensure_column(
            &conn,
            "messages",
            "display_json",
            "ALTER TABLE messages ADD COLUMN display_json TEXT",
        )?;
        self.ensure_column(
            &conn,
            "messages",
            "visible_summary",
            "ALTER TABLE messages ADD COLUMN visible_summary TEXT",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "canonical_key",
            "ALTER TABLE memory_items ADD COLUMN canonical_key TEXT",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "status",
            "ALTER TABLE memory_items ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "source_type",
            "ALTER TABLE memory_items ADD COLUMN source_type TEXT",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "source_ref",
            "ALTER TABLE memory_items ADD COLUMN source_ref TEXT",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "supersedes_memory_id",
            "ALTER TABLE memory_items ADD COLUMN supersedes_memory_id INTEGER",
        )?;
        self.ensure_column(
            &conn,
            "memory_items",
            "last_recalled_at",
            "ALTER TABLE memory_items ADD COLUMN last_recalled_at TEXT",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "input_source",
            "ALTER TABLE turns ADD COLUMN input_source TEXT NOT NULL DEFAULT 'text'",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "memory_used",
            "ALTER TABLE turns ADD COLUMN memory_used INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "tool_consulted",
            "ALTER TABLE turns ADD COLUMN tool_consulted INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "nano_first_beat_used",
            "ALTER TABLE turns ADD COLUMN nano_first_beat_used INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "cloud_escalated",
            "ALTER TABLE turns ADD COLUMN cloud_escalated INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "selected_cloud_profile",
            "ALTER TABLE turns ADD COLUMN selected_cloud_profile TEXT",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "delivery_mode",
            "ALTER TABLE turns ADD COLUMN delivery_mode TEXT NOT NULL DEFAULT 'PENDING'",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "final_route",
            "ALTER TABLE turns ADD COLUMN final_route TEXT NOT NULL DEFAULT 'pending'",
        )?;
        self.ensure_column(
            &conn,
            "turns",
            "error_summary",
            "ALTER TABLE turns ADD COLUMN error_summary TEXT",
        )?;
        self.ensure_column(
            &conn,
            "retrieval_logs",
            "intent",
            "ALTER TABLE retrieval_logs ADD COLUMN intent TEXT NOT NULL DEFAULT 'unspecified'",
        )?;
        self.ensure_column(
            &conn,
            "retrieval_logs",
            "latency_ms",
            "ALTER TABLE retrieval_logs ADD COLUMN latency_ms INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            &conn,
            "retrieval_logs",
            "selected_message_ids_json",
            "ALTER TABLE retrieval_logs ADD COLUMN selected_message_ids_json TEXT NOT NULL DEFAULT '[]'",
        )?;
        self.ensure_column(
            &conn,
            "retrieval_logs",
            "selected_memory_ids_json",
            "ALTER TABLE retrieval_logs ADD COLUMN selected_memory_ids_json TEXT NOT NULL DEFAULT '[]'",
        )?;
        self.ensure_column(
            &conn,
            "retrieval_logs",
            "source_breakdown_json",
            "ALTER TABLE retrieval_logs ADD COLUMN source_breakdown_json TEXT NOT NULL DEFAULT '{}'",
        )?;
        self.ensure_index(
            &conn,
            "CREATE INDEX IF NOT EXISTS idx_memory_items_canonical_key_status ON memory_items(canonical_key, status)",
        )?;
        self.ensure_index(
            &conn,
            "CREATE INDEX IF NOT EXISTS idx_memory_items_status_vector_dim ON memory_items(status, vector_dim)",
        )?;
        self.ensure_index(
            &conn,
            "CREATE INDEX IF NOT EXISTS idx_working_memory_snapshots_created_at ON working_memory_snapshots(created_at DESC)",
        )?;
        self.ensure_index(
            &conn,
            "CREATE INDEX IF NOT EXISTS idx_note_activity_events_occurred_at ON note_activity_events(occurred_at DESC)",
        )?;
        Ok(())
    }

    pub(crate) fn ensure_column(
        &self,
        conn: &Connection,
        table: &str,
        column: &str,
        sql: &str,
    ) -> Result<()> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut exists = false;
        for row in rows {
            if row? == column {
                exists = true;
                break;
            }
        }
        if exists {
            return Ok(());
        }
        conn.execute(sql, [])
            .with_context(|| format!("failed to add {table}.{column}"))?;
        Ok(())
    }

    fn ensure_index(&self, conn: &Connection, sql: &str) -> Result<()> {
        conn.execute(sql, [])
            .with_context(|| format!("failed to ensure index with sql: {sql}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn schema_bootstrap_is_idempotent_and_includes_working_memory_and_recall_columns() {
        let temp = tempdir().expect("tempdir");
        let db_path = temp.path().join("memory.sqlite3");

        let db = SqliteMemoryDb::new(db_path.clone()).expect("create db");
        let _ = SqliteMemoryDb::new(db_path).expect("re-open db");

        let conn = db.connection().expect("connection");
        let working_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'working_memory_snapshots'",
                [],
                |row| row.get(0),
            )
            .expect("working table count");
        assert_eq!(working_exists, 1);
        let note_activity_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'note_activity_events'",
                [],
                |row| row.get(0),
            )
            .expect("note activity table count");
        assert_eq!(note_activity_exists, 1);

        let mut stmt = conn
            .prepare("PRAGMA table_info(retrieval_logs)")
            .expect("table info");
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("column values");
        assert!(columns.contains(&"intent".to_owned()));
        assert!(columns.contains(&"latency_ms".to_owned()));
        assert!(columns.contains(&"selected_message_ids_json".to_owned()));
        assert!(columns.contains(&"selected_memory_ids_json".to_owned()));
        assert!(columns.contains(&"source_breakdown_json".to_owned()));
    }
}
