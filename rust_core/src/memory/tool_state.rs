use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use super::sqlite::SqliteMemoryDb;
use super::ToolStateStore;

#[derive(Clone)]
pub struct SqliteToolStateStore {
    db: Arc<SqliteMemoryDb>,
}

impl SqliteToolStateStore {
    pub fn new(db: Arc<SqliteMemoryDb>) -> Self {
        Self { db }
    }
}

impl ToolStateStore for SqliteToolStateStore {
    fn get_tool_state(&self, tool_name: &str) -> Result<Option<Value>> {
        let conn = self.db.connection()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT state_json FROM tool_state WHERE tool_name = ?1",
                params![tool_name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw.map(|value| serde_json::from_str(&value)).transpose()?)
    }

    fn set_tool_state(&self, tool_name: &str, state_json: &Value, updated_at: &str) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO tool_state (tool_name, state_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(tool_name) DO UPDATE SET
                state_json = excluded.state_json,
                updated_at = excluded.updated_at
            "#,
            params![tool_name, serde_json::to_string(state_json)?, updated_at],
        )?;
        Ok(())
    }
}
