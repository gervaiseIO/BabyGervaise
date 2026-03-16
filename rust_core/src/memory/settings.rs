use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::{now_rfc3339, ContextLevel};

use super::sqlite::SqliteMemoryDb;

#[derive(Clone)]
pub struct SqliteSettingsStore {
    db: Arc<SqliteMemoryDb>,
}

impl SqliteSettingsStore {
    pub fn new(db: Arc<SqliteMemoryDb>) -> Self {
        Self { db }
    }

    pub fn ensure_previous_context(&self, level: ContextLevel) -> Result<()> {
        if self
            .get_setting::<ContextLevel>("previous_context")?
            .is_none()
        {
            self.set_setting("previous_context", &level)?;
        }
        Ok(())
    }

    pub fn get_previous_context(&self, default: ContextLevel) -> Result<ContextLevel> {
        Ok(self.get_setting("previous_context")?.unwrap_or(default))
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        self.set_setting("previous_context", &level)
    }

    pub fn get_selected_cloud_profile(&self) -> Result<Option<String>> {
        self.get_setting("selected_cloud_profile")
    }

    pub fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()> {
        self.set_setting("selected_cloud_profile", &profile_id)
    }

    pub fn ambient_cooldown_elapsed(&self, cooldown_seconds: u64) -> Result<bool> {
        let last_emitted: Option<String> = self.get_setting("ambient_last_emitted_at")?;
        let Some(last_emitted) = last_emitted else {
            return Ok(true);
        };
        let Some(last_emitted) = time::OffsetDateTime::parse(
            &last_emitted,
            &time::format_description::well_known::Rfc3339,
        )
        .ok() else {
            return Ok(true);
        };
        let elapsed_seconds = (time::OffsetDateTime::now_utc() - last_emitted).whole_seconds();
        Ok(elapsed_seconds >= cooldown_seconds as i64)
    }

    pub fn record_ambient_emit(&self, event_type: &str) -> Result<()> {
        self.set_setting("ambient_last_emitted_at", &now_rfc3339())?;
        self.set_setting("ambient_last_event_type", &event_type)
    }

    pub fn record_note_activity(
        &self,
        note_key: &str,
        relative_path: &str,
        title_snapshot: &str,
        event_type: &str,
        occurred_at: &str,
    ) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO note_activity_events (
                note_key,
                relative_path,
                title_snapshot,
                event_type,
                occurred_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![note_key, relative_path, title_snapshot, event_type, occurred_at],
        )?;
        Ok(())
    }

    fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let conn = self.db.connection()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw.map(|value| serde_json::from_str(&value)).transpose()?)
    }

    fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO app_settings (key, value_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at
            "#,
            params![key, serde_json::to_string(value)?, now_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::memory::sqlite::SqliteMemoryDb;

    #[test]
    fn record_note_activity_persists_lightweight_event() {
        let temp = tempdir().expect("tempdir");
        let db = Arc::new(SqliteMemoryDb::new(temp.path().join("memory.sqlite3")).expect("db"));
        let settings = SqliteSettingsStore::new(db.clone());

        settings
            .record_note_activity(
                "note-123",
                "HGIE.md",
                "HGIE Prompt Rephase",
                "note_opened",
                "2026-03-15T12:00:00Z",
            )
            .expect("record note activity");

        let conn = db.connection().expect("connection");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM note_activity_events", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }
}
