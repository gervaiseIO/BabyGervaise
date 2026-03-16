use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{Map, Value};

use crate::logging::{
    DecisionTraceEntry, DiagnosticIssue, DiagnosticsOverview, LogViewerEntry, MemoryStats,
    ModelLogEntry, ModelStats, ModelTraceEntry, OverviewSnapshot, RetrievalLogEntry,
    RuntimeOverview, SystemStats, ToolLogEntry, TraceEventRecord, TurnTraceSummary, UsageStats,
};
use crate::tools::ToolsOverview;
use crate::ContextLevel;

use super::sqlite::SqliteMemoryDb;
use super::MemoryObservabilityStore;

#[derive(Clone)]
pub struct SqliteMemoryObservabilityStore {
    db: Arc<SqliteMemoryDb>,
    max_model_logs: usize,
}

impl SqliteMemoryObservabilityStore {
    pub fn new(db: Arc<SqliteMemoryDb>, max_model_logs: usize) -> Self {
        Self { db, max_model_logs }
    }

    pub fn load_overview(
        &self,
        previous_context: ContextLevel,
        runtime: RuntimeOverview,
        model_name: &str,
        tools: ToolsOverview,
    ) -> Result<OverviewSnapshot> {
        let conn = self.db.connection()?;

        let message_count = scalar_i64(&conn, "SELECT COUNT(*) FROM messages")?;
        let all_durable_memories = scalar_i64(&conn, "SELECT COUNT(*) FROM memory_items")?;
        let active_durable_memories = scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM memory_items WHERE status = 'active'",
        )?;
        let superseded_durable_memories = scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM memory_items WHERE status = 'superseded'",
        )?;
        let vector_count = scalar_i64(
            &conn,
            "SELECT COUNT(*) FROM memory_items WHERE vector IS NOT NULL",
        )?;
        let retrieval_count = scalar_i64(&conn, "SELECT COUNT(*) FROM retrieval_logs")?;
        let working_snapshot_count =
            scalar_i64(&conn, "SELECT COUNT(*) FROM working_memory_snapshots")?;
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
        let cloud_stats = match runtime.selected_cloud_profile_label.as_deref() {
            Some(selected_label) => {
                load_model_usage_stats(&conn, "WHERE model_name = ?1", Some(selected_label), false)?
            }
            None => UsageStats {
                calls: 0,
                tokens_in: Some(0),
                tokens_out: Some(0),
                latency_avg_ms: None,
                latency_latest_ms: None,
                tokens_per_second: None,
            },
        };
        let nano_stats = load_model_usage_stats(
            &conn,
            "WHERE model_name IN ('Nano first beat', 'Nano follow-up', 'Nano ambient')",
            None,
            true,
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

        let mut stmt = conn.prepare(
            r#"
            SELECT created_at, tool_name, action, arguments_json, result_json, success, latency_ms
            FROM tool_logs
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![self.max_model_logs as i64], |row| {
            Ok(ToolLogEntry {
                created_at: row.get(0)?,
                tool_name: row.get(1)?,
                action: row.get(2)?,
                arguments_json: row.get(3)?,
                result_json: row.get(4)?,
                success: row.get::<_, i64>(5)? != 0,
                latency_ms: row.get(6)?,
            })
        })?;

        let mut recent_tool_logs = Vec::new();
        for row in rows {
            recent_tool_logs.push(row?);
        }

        let turn_summaries = self.load_recent_turn_summaries(&conn)?;
        let model_traces = self.load_recent_model_traces(&conn)?;
        let decision_events = self.load_recent_decision_events(&conn)?;
        let issues = self.load_recent_diagnostic_issues(&conn)?;

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
            cloud_stats,
            nano_stats,
            memory_stats: MemoryStats {
                message_count,
                stored_memories: all_durable_memories,
                vector_count,
                retrieval_count,
                active_durable_memories,
                superseded_durable_memories,
                all_durable_memories,
                working_snapshot_count,
            },
            system_stats: SystemStats {
                total_interactions,
                tool_calls,
                error_count,
            },
            runtime,
            tools,
            diagnostics: DiagnosticsOverview {
                turn_summaries: turn_summaries.clone(),
                model_traces: model_traces.clone(),
                decision_events: decision_events.clone(),
                issues,
                recent_logs: recent_logs.clone(),
                recent_tool_logs: recent_tool_logs.clone(),
            },
            tool_states,
            recent_logs,
            recent_tool_logs,
            turn_summaries,
            model_traces,
            decision_events,
        })
    }

    fn load_recent_turn_summaries(&self, conn: &Connection) -> Result<Vec<TurnTraceSummary>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT
                turn_id, created_at, user_input_summary, input_source, plan_kind, fallback_plan_kind,
                context_policy, model_stages_json, memory_used, tool_consulted, tool_used,
                nano_first_beat_used, cloud_escalated, cloud_used, selected_cloud_profile,
                delivery_mode, final_route, error_summary, total_latency_ms, final_visible_output,
                had_fallback
            FROM turns
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![self.max_model_logs as i64], |row| {
            let raw_model_stages: String = row.get(7)?;
            Ok(TurnTraceSummary {
                turn_id: row.get(0)?,
                created_at: row.get(1)?,
                user_input_summary: row.get(2)?,
                input_source: row.get(3)?,
                plan_kind: row.get(4)?,
                fallback_plan_kind: row.get(5)?,
                context_policy: row.get(6)?,
                model_stages: serde_json::from_str(&raw_model_stages).unwrap_or_default(),
                memory_used: row.get::<_, i64>(8)? != 0,
                tool_consulted: row.get::<_, i64>(9)? != 0,
                tool_used: row.get::<_, i64>(10)? != 0,
                nano_first_beat_used: row.get::<_, i64>(11)? != 0,
                cloud_escalated: row.get::<_, i64>(12)? != 0,
                cloud_used: row.get::<_, i64>(13)? != 0,
                selected_cloud_profile: row.get(14)?,
                delivery_mode: row.get(15)?,
                final_route: row.get(16)?,
                error_summary: row.get(17)?,
                total_latency_ms: row.get(18)?,
                final_visible_output: row.get(19)?,
                had_fallback: row.get::<_, i64>(20)? != 0,
            })
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(row?);
        }
        Ok(summaries)
    }

    fn load_recent_diagnostic_issues(&self, conn: &Connection) -> Result<Vec<DiagnosticIssue>> {
        let mut issues = Vec::new();

        let mut model_stmt = conn.prepare(
            r#"
            SELECT created_at, error_text, model_name
            FROM model_logs
            WHERE error_text IS NOT NULL AND error_text != ''
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let model_rows = model_stmt.query_map(params![10_i64], |row| {
            Ok(DiagnosticIssue {
                timestamp: row.get(0)?,
                subsystem: "model".to_owned(),
                level: "error".to_owned(),
                summary: row.get::<_, String>(1)?,
                detail: row.get::<_, String>(2).ok(),
            })
        })?;
        for row in model_rows {
            issues.push(row?);
        }

        let mut tool_stmt = conn.prepare(
            r#"
            SELECT created_at, tool_name, action, result_json
            FROM tool_logs
            WHERE success = 0
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let tool_rows = tool_stmt.query_map(params![10_i64], |row| {
            let result_json: String = row.get(3)?;
            let parsed = serde_json::from_str::<Value>(&result_json).unwrap_or(Value::Null);
            let summary = parsed
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Tool action failed.")
                .to_owned();
            Ok(DiagnosticIssue {
                timestamp: row.get(0)?,
                subsystem: format!("tool:{}", row.get::<_, String>(1)?),
                level: "warning".to_owned(),
                summary,
                detail: Some(format!("action={}", row.get::<_, String>(2)?)),
            })
        })?;
        for row in tool_rows {
            issues.push(row?);
        }

        issues.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
        issues.truncate(12);
        Ok(issues)
    }

    fn load_recent_model_traces(&self, conn: &Connection) -> Result<Vec<ModelTraceEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, timestamp, turn_id, name, prompt_mode, lane, provider, model, status, latency_ms, displayed_text
            FROM trace_events
            WHERE category = 'model'
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![self.max_model_logs as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        })?;

        let mut model_rows = Vec::new();
        let mut event_ids = Vec::new();
        for row in rows {
            let row = row?;
            event_ids.push(row.0);
            model_rows.push(row);
        }
        let payloads = self.load_trace_payloads(conn, &event_ids)?;

        let mut model_traces = Vec::new();
        for (
            event_id,
            timestamp,
            turn_id,
            stage_name,
            prompt_mode,
            lane,
            provider,
            model,
            status,
            latency_ms,
            displayed_text,
        ) in model_rows
        {
            let attachments = payloads.get(&event_id);
            model_traces.push(ModelTraceEntry {
                timestamp,
                turn_id,
                stage_name,
                prompt_mode,
                lane,
                provider,
                model,
                status: status.unwrap_or_else(|| "unknown".to_owned()),
                latency_ms: latency_ms.unwrap_or_default(),
                displayed_text,
                discarded_text: payload_slot_text(attachments, "discarded_output"),
                raw_input: payload_slot_text(attachments, "raw_input"),
                raw_output: payload_slot_text(attachments, "raw_output"),
                normalized_output: payload_slot_text(attachments, "normalized_output"),
            });
        }
        Ok(model_traces)
    }

    fn load_recent_decision_events(&self, conn: &Connection) -> Result<Vec<DecisionTraceEntry>> {
        let mut stmt = conn.prepare(
            r#"
            SELECT timestamp, turn_id, name, plan_kind, fallback_plan_kind, reason_codes_json, context_policy
            FROM trace_events
            WHERE category IN ('turn', 'decision', 'fallback', 'memory', 'render')
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![self.max_model_logs as i64], |row| {
            let raw_reason_codes: Option<String> = row.get(5)?;
            let context_policy: Option<String> = row.get(6)?;
            Ok(DecisionTraceEntry {
                timestamp: row.get(0)?,
                turn_id: row.get(1)?,
                name: row.get(2)?,
                plan_kind: row.get(3)?,
                fallback_plan_kind: row.get(4)?,
                reason_codes: raw_reason_codes
                    .as_deref()
                    .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
                    .unwrap_or_default(),
                detail: context_policy,
            })
        })?;

        let mut decisions = Vec::new();
        for row in rows {
            decisions.push(row?);
        }
        Ok(decisions)
    }

    fn load_trace_payloads(
        &self,
        conn: &Connection,
        event_ids: &[i64],
    ) -> Result<HashMap<i64, Vec<(String, String)>>> {
        let mut payloads = HashMap::<i64, Vec<(String, String)>>::new();
        let mut stmt = conn.prepare(
            r#"
            SELECT event_id, slot, content_text
            FROM trace_payloads
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let ids = event_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for row in rows {
            let (event_id, slot, content_text) = row?;
            if ids.contains(&event_id) {
                payloads
                    .entry(event_id)
                    .or_default()
                    .push((slot, content_text));
            }
        }
        Ok(payloads)
    }
}

impl MemoryObservabilityStore for SqliteMemoryObservabilityStore {
    fn log_recall(&self, entry: &RetrievalLogEntry) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO retrieval_logs (
                created_at, level, recent_count, semantic_count, query_text, intent, latency_ms,
                selected_message_ids_json, selected_memory_ids_json, source_breakdown_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                entry.created_at,
                entry.level.as_str(),
                entry.recent_count as i64,
                entry.semantic_count as i64,
                entry.query_text,
                entry.intent,
                entry.latency_ms,
                entry.selected_message_ids_json,
                entry.selected_memory_ids_json,
                entry.source_breakdown_json,
            ],
        )?;
        Ok(())
    }

    fn log_model_call(&self, entry: &ModelLogEntry) -> Result<()> {
        let conn = self.db.connection()?;
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

    fn log_tool_call(&self, entry: &ToolLogEntry) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO tool_logs (
                created_at, tool_name, action, arguments_json, result_json, success, latency_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                entry.created_at,
                entry.tool_name,
                entry.action,
                entry.arguments_json,
                entry.result_json,
                entry.success as i64,
                entry.latency_ms
            ],
        )?;
        Ok(())
    }

    fn upsert_turn_summary(&self, summary: &TurnTraceSummary) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO turns (
                turn_id, created_at, user_input_summary, input_source, plan_kind, fallback_plan_kind,
                context_policy, model_stages_json, memory_used, tool_consulted, tool_used,
                nano_first_beat_used, cloud_escalated, cloud_used, selected_cloud_profile,
                delivery_mode, final_route, error_summary, total_latency_ms, final_visible_output,
                had_fallback
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
            ON CONFLICT(turn_id) DO UPDATE SET
                created_at = excluded.created_at,
                user_input_summary = excluded.user_input_summary,
                input_source = excluded.input_source,
                plan_kind = excluded.plan_kind,
                fallback_plan_kind = excluded.fallback_plan_kind,
                context_policy = excluded.context_policy,
                model_stages_json = excluded.model_stages_json,
                memory_used = excluded.memory_used,
                tool_consulted = excluded.tool_consulted,
                tool_used = excluded.tool_used,
                nano_first_beat_used = excluded.nano_first_beat_used,
                cloud_escalated = excluded.cloud_escalated,
                cloud_used = excluded.cloud_used,
                selected_cloud_profile = excluded.selected_cloud_profile,
                delivery_mode = excluded.delivery_mode,
                final_route = excluded.final_route,
                error_summary = excluded.error_summary,
                total_latency_ms = excluded.total_latency_ms,
                final_visible_output = excluded.final_visible_output,
                had_fallback = excluded.had_fallback
            "#,
            params![
                summary.turn_id,
                summary.created_at,
                summary.user_input_summary,
                summary.input_source,
                summary.plan_kind,
                summary.fallback_plan_kind,
                summary.context_policy,
                serde_json::to_string(&summary.model_stages)?,
                summary.memory_used as i64,
                summary.tool_consulted as i64,
                summary.tool_used as i64,
                summary.nano_first_beat_used as i64,
                summary.cloud_escalated as i64,
                summary.cloud_used as i64,
                summary.selected_cloud_profile,
                summary.delivery_mode,
                summary.final_route,
                summary.error_summary,
                summary.total_latency_ms,
                summary.final_visible_output,
                summary.had_fallback as i64
            ],
        )?;
        Ok(())
    }

    fn append_trace_event(&self, event: &TraceEventRecord) -> Result<()> {
        let conn = self.db.connection()?;
        conn.execute(
            r#"
            INSERT INTO trace_events (
                turn_id, stage_seq, timestamp, category, name, plan_kind, fallback_plan_kind,
                reason_codes_json, context_policy, prompt_mode, lane, provider, model, status,
                latency_ms, http_status, error_text, displayed_text, selected_refs_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
            "#,
            params![
                event.turn_id,
                event.stage_seq,
                event.timestamp,
                event.category,
                event.name,
                event.plan_kind,
                event.fallback_plan_kind,
                serde_json::to_string(&event.reason_codes)?,
                event.context_policy,
                event.prompt_mode,
                event.lane,
                event.provider,
                event.model,
                event.status,
                event.latency_ms,
                event.http_status,
                event.error_text,
                event.displayed_text,
                event.selected_refs_json
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?
            ],
        )?;
        let event_id = conn.last_insert_rowid();

        for payload in &event.payloads {
            conn.execute(
                r#"
                INSERT INTO trace_payloads (
                    event_id, slot, visibility_class, content_format, content_text, size_bytes
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    event_id,
                    payload.slot,
                    payload.visibility_class.as_str(),
                    payload.content_format,
                    payload.content_text,
                    payload.size_bytes as i64
                ],
            )?;
        }
        Ok(())
    }

    fn load_overview(
        &self,
        previous_context: ContextLevel,
        runtime: RuntimeOverview,
        model_name: &str,
        tools: ToolsOverview,
    ) -> Result<OverviewSnapshot> {
        SqliteMemoryObservabilityStore::load_overview(
            self,
            previous_context,
            runtime,
            model_name,
            tools,
        )
    }
}

fn scalar_i64(conn: &Connection, sql: &str) -> Result<i64> {
    Ok(conn.query_row(sql, [], |row| row.get::<_, i64>(0))?)
}

fn load_model_usage_stats(
    conn: &Connection,
    where_clause: &str,
    filter_param: Option<&str>,
    prefer_missing_tokens: bool,
) -> Result<UsageStats> {
    let count_sql = format!("SELECT COUNT(*) FROM model_logs {where_clause}");
    let input_sql = format!("SELECT SUM(input_tokens) FROM model_logs {where_clause}");
    let output_sql = format!("SELECT SUM(output_tokens) FROM model_logs {where_clause}");
    let total_latency_sql = format!("SELECT SUM(latency_ms) FROM model_logs {where_clause}");
    let average_sql =
        format!("SELECT CAST(AVG(latency_ms) AS INTEGER) FROM model_logs {where_clause}");
    let latest_sql =
        format!("SELECT latency_ms FROM model_logs {where_clause} ORDER BY id DESC LIMIT 1");

    let calls = match filter_param {
        Some(value) => conn.query_row(&count_sql, params![value], |row| row.get::<_, i64>(0))?,
        None => conn.query_row(&count_sql, [], |row| row.get::<_, i64>(0))?,
    };
    let total_input_tokens = optional_i64(conn, &input_sql, filter_param)?;
    let total_output_tokens = optional_i64(conn, &output_sql, filter_param)?;
    let total_latency_ms = optional_i64(conn, &total_latency_sql, filter_param)?;
    let average_latency_ms = if calls > 0 {
        optional_i64(conn, &average_sql, filter_param)?
    } else {
        None
    };
    let latest_latency_ms = if calls > 0 {
        optional_i64(conn, &latest_sql, filter_param)?
    } else {
        None
    };
    let tokens_per_second =
        compute_average_tokens_per_second(total_output_tokens, total_latency_ms);

    Ok(UsageStats {
        calls,
        tokens_in: if prefer_missing_tokens {
            total_input_tokens
        } else {
            Some(total_input_tokens.unwrap_or(0))
        },
        tokens_out: if prefer_missing_tokens {
            total_output_tokens
        } else {
            Some(total_output_tokens.unwrap_or(0))
        },
        latency_avg_ms: average_latency_ms,
        latency_latest_ms: latest_latency_ms,
        tokens_per_second,
    })
}

fn compute_average_tokens_per_second(
    total_output_tokens: Option<i64>,
    total_latency_ms: Option<i64>,
) -> Option<i64> {
    let tokens = total_output_tokens?;
    let latency_ms = total_latency_ms?;
    if tokens <= 0 || latency_ms <= 0 {
        return None;
    }

    Some(((tokens as f64 * 1000.0) / latency_ms as f64).round() as i64)
}

fn optional_i64(conn: &Connection, sql: &str, filter_param: Option<&str>) -> Result<Option<i64>> {
    Ok(match filter_param {
        Some(value) => conn.query_row(sql, params![value], |row| row.get::<_, Option<i64>>(0))?,
        None => conn.query_row(sql, [], |row| row.get::<_, Option<i64>>(0))?,
    })
}

fn payload_slot_text(attachments: Option<&Vec<(String, String)>>, slot: &str) -> Option<String> {
    attachments.and_then(|entries| {
        entries
            .iter()
            .find(|(entry_slot, _)| entry_slot == slot)
            .map(|(_, text)| text.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::compute_average_tokens_per_second;

    #[test]
    fn computes_tokens_per_second_when_latency_and_tokens_exist() {
        assert_eq!(
            compute_average_tokens_per_second(Some(240), Some(120)),
            Some(2000)
        );
    }

    #[test]
    fn tokens_per_second_is_none_without_complete_stats() {
        assert_eq!(compute_average_tokens_per_second(None, Some(120)), None);
        assert_eq!(compute_average_tokens_per_second(Some(240), None), None);
        assert_eq!(compute_average_tokens_per_second(Some(0), Some(120)), None);
        assert_eq!(compute_average_tokens_per_second(Some(240), Some(0)), None);
    }
}
