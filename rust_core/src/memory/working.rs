use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::sqlite::SqliteMemoryDb;
use super::types::WorkingMemorySnapshot;
use super::WorkingMemoryStore;

#[derive(Clone)]
pub struct SqliteWorkingMemoryStore {
    db: Arc<SqliteMemoryDb>,
}

impl SqliteWorkingMemoryStore {
    pub fn new(db: Arc<SqliteMemoryDb>) -> Self {
        Self { db }
    }
}

impl WorkingMemoryStore for SqliteWorkingMemoryStore {
    fn load_latest_snapshot(&self) -> Result<Option<WorkingMemorySnapshot>> {
        let conn = self.db.connection()?;
        let row = conn
            .query_row(
                r#"
                SELECT turn_id, created_at, version, focus_thread_key, state_json
                FROM working_memory_snapshots
                ORDER BY id DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(
            |(turn_id, created_at, version, focus_thread_key, state_json)| {
                let mut snapshot =
                    serde_json::from_str::<WorkingMemorySnapshot>(&state_json).unwrap_or_default();
                snapshot.turn_id = turn_id;
                snapshot.created_at = created_at;
                snapshot.version = version;
                snapshot.focus_thread_key = focus_thread_key;
                snapshot
            },
        ))
    }

    fn load_snapshot_for_turn(&self, turn_id: &str) -> Result<Option<WorkingMemorySnapshot>> {
        let conn = self.db.connection()?;
        let row = conn
            .query_row(
                r#"
                SELECT turn_id, created_at, version, focus_thread_key, state_json
                FROM working_memory_snapshots
                WHERE turn_id = ?1
                LIMIT 1
                "#,
                params![turn_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(
            |(turn_id, created_at, version, focus_thread_key, state_json)| {
                let mut snapshot =
                    serde_json::from_str::<WorkingMemorySnapshot>(&state_json).unwrap_or_default();
                snapshot.turn_id = turn_id;
                snapshot.created_at = created_at;
                snapshot.version = version;
                snapshot.focus_thread_key = focus_thread_key;
                snapshot
            },
        ))
    }

    fn save_snapshot(&self, snapshot: &WorkingMemorySnapshot) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO working_memory_snapshots (
                turn_id, created_at, version, focus_thread_key, thread_count, open_loop_count, state_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(turn_id) DO UPDATE SET
                created_at = excluded.created_at,
                version = excluded.version,
                focus_thread_key = excluded.focus_thread_key,
                thread_count = excluded.thread_count,
                open_loop_count = excluded.open_loop_count,
                state_json = excluded.state_json
            "#,
            params![
                snapshot.turn_id,
                snapshot.created_at,
                snapshot.version,
                snapshot.focus_thread_key,
                snapshot.threads.len() as i64,
                snapshot.open_loops.len() as i64,
                serde_json::to_string(snapshot)?,
            ],
        )?;
        Ok(())
    }

    fn count_snapshots(&self) -> Result<i64> {
        let conn = self.db.connection()?;
        Ok(
            conn.query_row("SELECT COUNT(*) FROM working_memory_snapshots", [], |row| {
                row.get(0)
            })?,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::memory::types::{WorkingMemorySourceRefs, WorkingThread, WorkingThreadStatus};
    use crate::memory::SqliteMemoryDb;

    #[test]
    fn working_memory_snapshot_round_trips_and_latest_snapshot_wins() {
        let temp = tempdir().expect("tempdir");
        let db = Arc::new(SqliteMemoryDb::new(temp.path().join("memory.sqlite3")).expect("db"));
        let store = SqliteWorkingMemoryStore::new(db);

        let first = WorkingMemorySnapshot {
            turn_id: "turn-1".to_owned(),
            created_at: "2026-03-15T10:00:00Z".to_owned(),
            version: 1,
            focus_thread_key: Some("thread:first".to_owned()),
            threads: vec![WorkingThread {
                key: "thread:first".to_owned(),
                status: WorkingThreadStatus::Focused,
                topic_label: "memory".to_owned(),
                synopsis: "We are talking about memory.".to_owned(),
                last_touched_turn_id: "turn-1".to_owned(),
                last_touched_at: "2026-03-15T10:00:00Z".to_owned(),
                message_refs: vec![1, 2],
                durable_memory_ids: vec![10],
                score: 1.0,
                stale_turns: 0,
            }],
            entity_anchors: vec![],
            open_loops: vec![],
            interaction_constraints: vec![],
            recent_tool_outcomes: vec![],
            source_refs: WorkingMemorySourceRefs {
                message_ids: vec![1, 2],
                durable_memory_ids: vec![10],
            },
        };
        let second = WorkingMemorySnapshot {
            turn_id: "turn-2".to_owned(),
            created_at: "2026-03-15T10:05:00Z".to_owned(),
            version: 1,
            focus_thread_key: Some("thread:second".to_owned()),
            threads: vec![WorkingThread {
                key: "thread:second".to_owned(),
                status: WorkingThreadStatus::Focused,
                topic_label: "mcp".to_owned(),
                synopsis: "We shifted to MCP.".to_owned(),
                last_touched_turn_id: "turn-2".to_owned(),
                last_touched_at: "2026-03-15T10:05:00Z".to_owned(),
                message_refs: vec![3],
                durable_memory_ids: vec![],
                score: 1.0,
                stale_turns: 0,
            }],
            entity_anchors: vec![],
            open_loops: vec![],
            interaction_constraints: vec![],
            recent_tool_outcomes: vec![],
            source_refs: WorkingMemorySourceRefs {
                message_ids: vec![3],
                durable_memory_ids: vec![],
            },
        };

        store.save_snapshot(&first).expect("save first");
        store.save_snapshot(&second).expect("save second");

        let loaded_first = store
            .load_snapshot_for_turn("turn-1")
            .expect("load first")
            .expect("first snapshot");
        assert_eq!(
            loaded_first.focus_thread_key.as_deref(),
            Some("thread:first")
        );
        assert_eq!(loaded_first.source_refs.message_ids, vec![1, 2]);

        let latest = store
            .load_latest_snapshot()
            .expect("load latest")
            .expect("latest snapshot");
        assert_eq!(latest.turn_id, "turn-2");
        assert_eq!(latest.focus_thread_key.as_deref(), Some("thread:second"));
        assert_eq!(store.count_snapshots().expect("count"), 2);
    }
}
