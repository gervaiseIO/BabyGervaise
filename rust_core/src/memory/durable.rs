use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::now_rfc3339;

use super::sqlite::SqliteMemoryDb;
use super::types::{DurableMemoryCounts, DurableMemoryRecord, DurableRecallQuery};
use super::vector::{
    blob_to_vector, cosine_similarity, normalize_memory_text, vector_to_blob, vectorize_text,
};
use super::DurableMemoryStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedMemory {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub salience: f32,
    pub similarity: f32,
    #[serde(default)]
    pub source_message_id: Option<i64>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub relevance_score: f32,
    #[serde(default)]
    pub canonical_key: Option<String>,
}

impl RetrievedMemory {
    pub fn prompt_fact_text(&self, max_chars: usize) -> String {
        let normalized = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.chars().count() <= max_chars {
            normalized
        } else {
            let truncated = normalized
                .chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>();
            format!("{truncated}…")
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroundedMemoryWrite<'a> {
    pub kind: &'a str,
    pub canonical_key: &'a str,
    pub text: &'a str,
    pub salience: f32,
    pub source_message_id: Option<i64>,
    pub source_type: &'a str,
    pub source_ref: &'a str,
}

#[derive(Clone)]
pub struct SqliteDurableMemoryStore {
    db: Arc<SqliteMemoryDb>,
    vector_dimensions: usize,
}

impl SqliteDurableMemoryStore {
    pub fn new(db: Arc<SqliteMemoryDb>, vector_dimensions: usize) -> Self {
        Self {
            db,
            vector_dimensions,
        }
    }
}

impl DurableMemoryStore for SqliteDurableMemoryStore {
    type Hit = RetrievedMemory;

    fn promote_grounded_memory(
        &self,
        request: &GroundedMemoryWrite<'_>,
    ) -> Result<DurableMemoryRecord> {
        let conn = self.db.connection()?;
        let vector = vectorize_text(request.text, self.vector_dimensions);
        let existing_active: Option<i64> = conn
            .query_row(
                r#"
                SELECT id
                FROM memory_items
                WHERE canonical_key = ?1 AND status = 'active'
                ORDER BY id DESC
                LIMIT 1
                "#,
                params![request.canonical_key],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(existing_id) = existing_active {
            conn.execute(
                r#"
                UPDATE memory_items
                SET status = 'superseded'
                WHERE id = ?1
                "#,
                params![existing_id],
            )?;
        }

        let created_at = now_rfc3339();
        conn.execute(
            r#"
            INSERT INTO memory_items (
                kind, text, salience, vector, vector_dim, vector_version, source_message_id, created_at,
                canonical_key, status, source_type, source_ref, supersedes_memory_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?11, ?12)
            "#,
            params![
                request.kind,
                request.text,
                request.salience,
                vector_to_blob(&vector),
                self.vector_dimensions as i64,
                1_i64,
                request.source_message_id,
                created_at,
                request.canonical_key,
                request.source_type,
                request.source_ref,
                existing_active
            ],
        )?;
        let id = conn.last_insert_rowid();

        Ok(DurableMemoryRecord {
            id,
            kind: request.kind.to_owned(),
            text: request.text.to_owned(),
            salience: request.salience,
            source_message_id: request.source_message_id,
            created_at,
            canonical_key: Some(request.canonical_key.to_owned()),
            status: "active".to_owned(),
            source_type: Some(request.source_type.to_owned()),
            source_ref: Some(request.source_ref.to_owned()),
            supersedes_memory_id: existing_active,
            last_recalled_at: None,
        })
    }

    fn search_active(&self, query: &DurableRecallQuery) -> Result<Vec<Self::Hit>> {
        let conn = self.db.connection()?;
        let query_vector = vectorize_text(&query.query_text, self.vector_dimensions);
        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, text, salience, vector, source_message_id, created_at, canonical_key
            FROM memory_items
            WHERE vector_dim = ?1
              AND status = 'active'
            "#,
        )?;

        let rows = stmt.query_map(params![self.vector_dimensions as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f32>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;

        let mut matches = Vec::new();
        for row in rows {
            let (
                id,
                kind,
                text,
                salience,
                vector_blob,
                source_message_id,
                created_at,
                canonical_key,
            ) = row?;
            let stored_vector = blob_to_vector(&vector_blob)?;
            let similarity = cosine_similarity(&query_vector, &stored_vector);
            let relevance_score = (similarity * 0.7) + (salience.clamp(0.0, 1.0) * 0.3);
            matches.push(RetrievedMemory {
                id,
                kind,
                text,
                salience,
                similarity,
                source_message_id,
                created_at,
                relevance_score,
                canonical_key,
            });
        }

        matches.sort_by(|left, right| {
            right
                .relevance_score
                .partial_cmp(&left.relevance_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| {
                    right
                        .similarity
                        .partial_cmp(&left.similarity)
                        .unwrap_or(Ordering::Equal)
                })
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        matches.retain(|item| item.similarity > 0.05);

        let mut deduped = Vec::new();
        let mut seen_text = HashSet::new();
        let mut seen_keys = HashSet::new();
        let limit = query.limit.unwrap_or(query.context_level.semantic_limit());
        for item in matches {
            let normalized_text = normalize_memory_text(&item.text);
            let duplicate_text = !normalized_text.is_empty() && !seen_text.insert(normalized_text);
            let duplicate_key = item
                .canonical_key
                .as_ref()
                .is_some_and(|key| !seen_keys.insert(key.clone()));
            if duplicate_text || duplicate_key {
                continue;
            }
            deduped.push(item);
            if deduped.len() >= limit {
                break;
            }
        }

        Ok(deduped)
    }

    fn load_active_by_ids(&self, memory_ids: &[i64]) -> Result<Vec<Self::Hit>> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.db.connection()?;
        let mut seen_ids = HashSet::new();
        let ordered_ids = memory_ids
            .iter()
            .copied()
            .filter(|memory_id| seen_ids.insert(*memory_id))
            .collect::<Vec<_>>();

        let mut stmt = conn.prepare(
            r#"
            SELECT id, kind, text, salience, source_message_id, created_at, canonical_key
            FROM memory_items
            WHERE id = ?1
              AND status = 'active'
            LIMIT 1
            "#,
        )?;

        let mut matches = Vec::new();
        for memory_id in ordered_ids {
            if let Some(memory) = stmt
                .query_row(params![memory_id], |row| {
                    Ok(RetrievedMemory {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        text: row.get(2)?,
                        salience: row.get(3)?,
                        similarity: 0.0,
                        source_message_id: row.get(4)?,
                        created_at: row.get(5)?,
                        relevance_score: 0.0,
                        canonical_key: row.get(6)?,
                    })
                })
                .optional()?
            {
                matches.push(memory);
            }
        }

        Ok(matches)
    }

    fn mark_recalled(&self, memory_ids: &[i64], recalled_at: &str) -> Result<()> {
        if memory_ids.is_empty() {
            return Ok(());
        }

        let conn = self.db.connection()?;
        for memory_id in memory_ids {
            conn.execute(
                r#"
                UPDATE memory_items
                SET last_recalled_at = ?2
                WHERE id = ?1 AND status = 'active'
                "#,
                params![memory_id, recalled_at],
            )?;
        }
        Ok(())
    }

    fn load_counts(&self) -> Result<DurableMemoryCounts> {
        let conn = self.db.connection()?;
        let active_count = conn.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE status = 'active'",
            [],
            |row| row.get(0),
        )?;
        let superseded_count = conn.query_row(
            "SELECT COUNT(*) FROM memory_items WHERE status = 'superseded'",
            [],
            |row| row.get(0),
        )?;
        let all_count =
            conn.query_row("SELECT COUNT(*) FROM memory_items", [], |row| row.get(0))?;
        Ok(DurableMemoryCounts {
            active_count,
            superseded_count,
            all_count,
        })
    }
}
