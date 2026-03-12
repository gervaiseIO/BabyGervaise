use std::cmp::Ordering;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::logging::{
    LogViewerEntry, MemoryStats, ModelLogEntry, ModelStats, OverviewSnapshot, RetrievalLogEntry,
    SystemStats, ToolLogEntry,
};
use crate::{now_rfc3339, AppConfig, BootstrapState, ChatMessage, ContextLevel, InputSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedMemory {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub salience: f32,
    pub similarity: f32,
}

#[derive(Clone)]
pub struct MemoryStore {
    db_path: PathBuf,
    vector_dimensions: usize,
    max_recent_messages_per_turn: usize,
    max_model_logs: usize,
}

impl MemoryStore {
    pub fn new(db_path: PathBuf, app_config: &AppConfig) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).context("failed to create database parent directory")?;
        }

        let store = Self {
            db_path,
            vector_dimensions: app_config.vector_dimensions,
            max_recent_messages_per_turn: app_config.max_recent_messages_per_turn,
            max_model_logs: app_config.max_model_logs,
        };
        store.ensure_schema()?;
        store.ensure_previous_context(app_config.default_previous_context)?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path).context("failed to open SQLite database")?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .context("failed to initialize SQLite pragmas")?;
        Ok(conn)
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
                created_at TEXT NOT NULL
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
                success INTEGER NOT NULL
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

            CREATE TABLE IF NOT EXISTS tool_state (
                tool_name TEXT PRIMARY KEY,
                state_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .context("failed to create SQLite schema")?;
        Ok(())
    }

    pub fn ensure_previous_context(&self, level: ContextLevel) -> Result<()> {
        let conn = self.connection()?;
        let exists: Option<String> = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'previous_context'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if exists.is_none() {
            conn.execute(
                "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)",
                params![
                    "previous_context",
                    serde_json::to_string(&level)?,
                    now_rfc3339()
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_previous_context(&self, default: ContextLevel) -> Result<ContextLevel> {
        let conn = self.connection()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'previous_context'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        Ok(match raw {
            Some(value) => serde_json::from_str(&value).unwrap_or(default),
            None => default,
        })
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO app_settings (key, value_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at
            "#,
            params![
                "previous_context",
                serde_json::to_string(&level)?,
                now_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn append_message(
        &self,
        role: &str,
        content: &str,
        turn_id: &str,
        input_source: InputSource,
        meta_json: Option<&Value>,
    ) -> Result<ChatMessage> {
        let conn = self.connection()?;
        let created_at = now_rfc3339();
        let meta_json = meta_json.map(serde_json::to_string).transpose()?;
        conn.execute(
            r#"
            INSERT INTO messages (role, content, turn_id, input_source, created_at, meta_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                role,
                content,
                turn_id,
                input_source.as_str(),
                created_at,
                meta_json
            ],
        )?;

        let id = conn.last_insert_rowid();
        Ok(ChatMessage {
            id,
            role: role.to_owned(),
            content: content.to_owned(),
            turn_id: turn_id.to_owned(),
            input_source,
            created_at,
        })
    }

    pub fn load_bootstrap_state(&self, default_level: ContextLevel) -> Result<BootstrapState> {
        Ok(BootstrapState {
            previous_context: self.get_previous_context(default_level)?,
            messages: self.load_all_messages()?,
        })
    }

    pub fn load_all_messages(&self) -> Result<Vec<ChatMessage>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, role, content, turn_id, input_source, created_at
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

    pub fn load_recent_messages(
        &self,
        level: ContextLevel,
        exclude_message_id: Option<i64>,
    ) -> Result<Vec<ChatMessage>> {
        let conn = self.connection()?;
        let limit = (level.recent_turn_limit() * 2).min(self.max_recent_messages_per_turn) as i64;
        let sql = if exclude_message_id.is_some() {
            r#"
            SELECT id, role, content, turn_id, input_source, created_at
            FROM messages
            WHERE id != ?1
            ORDER BY id DESC
            LIMIT ?2
            "#
        } else {
            r#"
            SELECT id, role, content, turn_id, input_source, created_at
            FROM messages
            ORDER BY id DESC
            LIMIT ?1
            "#
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(excluded_id) = exclude_message_id {
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

    pub fn store_memory_item(
        &self,
        kind: &str,
        text: &str,
        salience: f32,
        source_message_id: Option<i64>,
    ) -> Result<()> {
        let conn = self.connection()?;
        let vector = vectorize_text(text, self.vector_dimensions);
        conn.execute(
            r#"
            INSERT INTO memory_items (
                kind, text, salience, vector, vector_dim, vector_version, source_message_id, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                kind,
                text,
                salience,
                vector_to_blob(&vector),
                self.vector_dimensions as i64,
                1_i64,
                source_message_id,
                now_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn semantic_search(
        &self,
        query_text: &str,
        level: ContextLevel,
    ) -> Result<Vec<RetrievedMemory>> {
        let conn = self.connection()?;
        let query_vector = vectorize_text(query_text, self.vector_dimensions);
        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, text, salience, vector
            FROM memory_items
            WHERE vector_dim = ?1
            "#,
        )?;

        let rows = stmt.query_map(params![self.vector_dimensions as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f32>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })?;

        let mut matches = Vec::new();
        for row in rows {
            let (id, kind, text, salience, vector_blob) = row?;
            let stored_vector = blob_to_vector(&vector_blob)?;
            let similarity = cosine_similarity(&query_vector, &stored_vector);
            matches.push(RetrievedMemory {
                id,
                kind,
                text,
                salience,
                similarity,
            });
        }

        matches.sort_by(|left, right| {
            right
                .similarity
                .partial_cmp(&left.similarity)
                .unwrap_or(Ordering::Equal)
        });
        matches.retain(|item| item.similarity > 0.05);
        matches.truncate(level.semantic_limit());
        Ok(matches)
    }

    pub fn log_model_call(&self, entry: &ModelLogEntry) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO model_logs (
                created_at, model_name, prompt, raw_output, input_tokens, output_tokens, latency_ms, http_status, error_text
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                entry.created_at,
                entry.model_name,
                entry.prompt,
                entry.raw_output,
                entry.input_tokens,
                entry.output_tokens,
                entry.latency_ms,
                entry.http_status,
                entry.error_text
            ],
        )?;
        Ok(())
    }

    pub fn log_tool_call(&self, entry: &ToolLogEntry) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO tool_logs (created_at, tool_name, action, arguments_json, result_json, success)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                entry.created_at,
                entry.tool_name,
                entry.action,
                entry.arguments_json,
                entry.result_json,
                entry.success as i64
            ],
        )?;
        Ok(())
    }

    pub fn log_retrieval(&self, entry: &RetrievalLogEntry) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            r#"
            INSERT INTO retrieval_logs (created_at, level, recent_count, semantic_count, query_text)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                entry.created_at,
                entry.level.as_str(),
                entry.recent_count as i64,
                entry.semantic_count as i64,
                entry.query_text
            ],
        )?;
        Ok(())
    }

    pub fn set_tool_state(
        &self,
        tool_name: &str,
        state_json: &Value,
        updated_at: &str,
    ) -> Result<()> {
        let conn = self.connection()?;
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

    pub fn get_tool_state(&self, tool_name: &str) -> Result<Option<Value>> {
        let conn = self.connection()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT state_json FROM tool_state WHERE tool_name = ?1",
                params![tool_name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw.map(|value| serde_json::from_str(&value)).transpose()?)
    }

    pub fn load_overview(
        &self,
        previous_context: ContextLevel,
        model_name: &str,
    ) -> Result<OverviewSnapshot> {
        let conn = self.connection()?;

        let message_count = scalar_i64(&conn, "SELECT COUNT(*) FROM messages")?;
        let stored_memories = scalar_i64(&conn, "SELECT COUNT(*) FROM memory_items")?;
        let vector_count = scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM memory_items WHERE vector IS NOT NULL",
        )?;
        let retrieval_count = scalar_i64(&conn, "SELECT COUNT(*) FROM retrieval_logs")?;
        let total_interactions =
            scalar_i64(&conn, "SELECT COUNT(*) FROM messages WHERE role = 'user'")?;
        let tool_calls = scalar_i64(&conn, "SELECT COUNT(*) FROM tool_logs")?;
        let error_count = scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM model_logs WHERE error_text IS NOT NULL AND error_text != ''",
        )?;
        let total_requests = scalar_i64(&conn, "SELECT COUNT(*) FROM model_logs")?;
        let total_input_tokens = scalar_i64(
            &conn,
            "SELECT COALESCE(SUM(input_tokens), 0) FROM model_logs",
        )?;
        let total_output_tokens = scalar_i64(
            &conn,
            "SELECT COALESCE(SUM(output_tokens), 0) FROM model_logs",
        )?;
        let average_latency_ms = scalar_i64(
            &conn,
            "SELECT CAST(COALESCE(AVG(latency_ms), 0) AS INTEGER) FROM model_logs",
        )?;
        let latest_latency_ms = scalar_i64(
            &conn,
            "SELECT COALESCE((SELECT latency_ms FROM model_logs ORDER BY id DESC LIMIT 1), 0)",
        )?;

        let mut tool_states = Map::new();
        let mut stmt =
            conn.prepare("SELECT tool_name, state_json FROM tool_state ORDER BY tool_name ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (tool_name, raw_state) = row?;
            tool_states.insert(tool_name, serde_json::from_str(&raw_state)?);
        }

        let mut stmt = conn.prepare(
            r#"
            SELECT created_at, prompt, raw_output, latency_ms, http_status
            FROM model_logs
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![self.max_model_logs as i64], |row| {
            Ok(LogViewerEntry {
                timestamp: row.get(0)?,
                prompt: row.get(1)?,
                raw_output: row.get(2)?,
                latency_ms: row.get(3)?,
                status: row.get(4)?,
            })
        })?;

        let mut recent_logs = Vec::new();
        for row in rows {
            recent_logs.push(row?);
        }

        Ok(OverviewSnapshot {
            previous_context,
            model_stats: ModelStats {
                model_name: model_name.to_owned(),
                total_requests,
                total_input_tokens,
                total_output_tokens,
                average_latency_ms,
                latest_latency_ms,
            },
            memory_stats: MemoryStats {
                message_count,
                stored_memories,
                vector_count,
                retrieval_count,
            },
            system_stats: SystemStats {
                total_interactions,
                tool_calls,
                error_count,
            },
            tool_states,
            recent_logs,
        })
    }

    fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
        Ok(ChatMessage {
            id: row.get(0)?,
            role: row.get(1)?,
            content: row.get(2)?,
            turn_id: row.get(3)?,
            input_source: InputSource::from_str(&row.get::<_, String>(4)?),
            created_at: row.get(5)?,
        })
    }
}

fn scalar_i64(conn: &Connection, sql: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [], |row| row.get::<_, i64>(0))?)
}

pub fn vectorize_text(text: &str, dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0.0_f32; dimensions];
    if text.trim().is_empty() || dimensions == 0 {
        return vector;
    }

    let normalized = text.to_lowercase();
    let tokens: Vec<&str> = normalized.split_whitespace().collect();

    for token in &tokens {
        add_feature(&mut vector, token.as_bytes(), 1.0);
    }

    for pair in tokens.windows(2) {
        let phrase = format!("{}_{}", pair[0], pair[1]);
        add_feature(&mut vector, phrase.as_bytes(), 1.4);
    }

    let chars: Vec<char> = normalized.chars().collect();
    for window in chars.windows(3) {
        let trigram: String = window.iter().collect();
        add_feature(&mut vector, trigram.as_bytes(), 0.6);
    }

    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }

    vector
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| left_value * right_value)
        .sum()
}

fn add_feature(vector: &mut [f32], feature: &[u8], weight: f32) {
    let hash = stable_hash(feature);
    let index = (hash as usize) % vector.len();
    let sign = if hash & 1 == 0 { 1.0 } else { -1.0 };
    vector[index] += sign * weight;
}

fn stable_hash(input: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

fn blob_to_vector(blob: &[u8]) -> Result<Vec<f32>> {
    let mut vector = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        vector.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(vector)
}
