use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::{now_rfc3339, ChatMessage, InputSource, MessageContentType};

use super::sqlite::SqliteMemoryDb;
use super::types::{AppendMessageRequest, TranscriptSliceQuery};
use super::TranscriptStore;

#[derive(Clone)]
pub struct SqliteTranscriptStore {
    db: Arc<SqliteMemoryDb>,
    max_recent_messages_per_turn: usize,
}

impl SqliteTranscriptStore {
    pub fn new(db: Arc<SqliteMemoryDb>, max_recent_messages_per_turn: usize) -> Self {
        Self {
            db,
            max_recent_messages_per_turn,
        }
    }

    fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
        Ok(ChatMessage {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            turn_id: row.get(3)?,
            input_source: InputSource::from_str(&row.get::<_, String>(4)?),
            created_at: row.get(5)?,
            content_type: MessageContentType::from_str(&row.get::<_, String>(6)?),
            display_json: row.get(7)?,
            visible_summary: row.get(8)?,
        })
    }
}

impl TranscriptStore for SqliteTranscriptStore {
    fn append_message(&self, request: AppendMessageRequest) -> Result<ChatMessage> {
        let conn = self.db.connection()?;
        let created_at = now_rfc3339();
        let meta_json = request
            .meta_json
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        conn.execute(
            r#"
            INSERT INTO messages (
                role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary, meta_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                request.role,
                request.content,
                request.turn_id,
                request.input_source.as_str(),
                created_at,
                request.content_type.as_str(),
                request.display_json,
                request.visible_summary,
                meta_json
            ],
        )?;

        let id = conn.last_insert_rowid();
        Ok(ChatMessage {
            id,
            role: request.role,
            content: request.content,
            turn_id: request.turn_id,
            input_source: request.input_source,
            created_at,
            content_type: request.content_type,
            display_json: request.display_json,
            visible_summary: request.visible_summary,
        })
    }

    fn load_all_messages(&self) -> Result<Vec<ChatMessage>> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary
            FROM messages
            ORDER BY id ASC
            "#,
        )?;

        let rows = stmt.query_map([], Self::map_message_row)?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    fn load_recent_messages(&self, query: TranscriptSliceQuery) -> Result<Vec<ChatMessage>> {
        let conn = self.db.connection()?;
        let base_limit =
            (query.context_level.recent_turn_limit() * 2).min(self.max_recent_messages_per_turn);
        let limit = query.limit_override.unwrap_or(base_limit) as i64;
        let sql = if query.exclude_message_id.is_some() {
            r#"
            SELECT id, role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary
            FROM messages
            WHERE id != ?1
            ORDER BY id DESC
            LIMIT ?2
            "#
        } else {
            r#"
            SELECT id, role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary
            FROM messages
            ORDER BY id DESC
            LIMIT ?1
            "#
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(excluded_id) = query.exclude_message_id {
            stmt.query_map(params![excluded_id, limit], Self::map_message_row)?
        } else {
            stmt.query_map(params![limit], Self::map_message_row)?
        };

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        messages.reverse();
        Ok(messages)
    }

    fn load_turn_messages(&self, turn_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.db.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary
            FROM messages
            WHERE turn_id = ?1
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![turn_id], Self::map_message_row)?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    fn load_messages_by_ids(&self, message_ids: &[i64]) -> Result<Vec<ChatMessage>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.db.connection()?;
        let mut messages = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut sorted_ids = message_ids
            .iter()
            .copied()
            .filter(|message_id| seen_ids.insert(*message_id))
            .collect::<Vec<_>>();
        sorted_ids.sort_unstable();

        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, turn_id, input_source, created_at, content_type, display_json, visible_summary
            FROM messages
            WHERE id = ?1
            LIMIT 1
            "#,
        )?;
        for message_id in sorted_ids {
            if let Some(message) = stmt
                .query_row(params![message_id], Self::map_message_row)
                .optional()?
            {
                messages.push(message);
            }
        }

        Ok(messages)
    }
}
