use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::Instant;

use anyhow::Result;
use serde_json::json;

use crate::now_rfc3339;

use super::durable::{RetrievedMemory, SqliteDurableMemoryStore};
use super::service::MemoryPolicy;
use super::transcript::SqliteTranscriptStore;
use super::types::{
    ContinuityMode, ContinuitySignal, EntityAnchor, HydrateWorkingMemoryRequest,
    InteractionConstraint, OpenLoop, RecallBundle, RecallExplanation, RecallRequest, RecallStats,
    RefreshWorkingMemoryRequest, ThreadBridge, WarmContext, WorkingMemorySnapshot,
    WorkingMemorySourceRefs, WorkingThread, WorkingThreadStatus, WorkingToolOutcome,
};
use super::vector::{cosine_similarity, normalize_memory_text, vectorize_text};
use super::working::SqliteWorkingMemoryStore;
use super::{DurableMemoryStore, MemoryRecallEngine, TranscriptStore, WorkingMemoryStore};

#[derive(Clone)]
pub struct DefaultMemoryRecallEngine {
    policy: MemoryPolicy,
    transcript: SqliteTranscriptStore,
    durable: SqliteDurableMemoryStore,
    working: SqliteWorkingMemoryStore,
}

#[derive(Debug, Clone)]
struct ThreadContinuityCandidate {
    key: String,
    status: WorkingThreadStatus,
    score: f32,
    entity_hits: usize,
    open_loop_key: Option<String>,
    open_loop_score: f32,
    tool_score: f32,
    stale_turns: u32,
    last_touched_at: String,
}

#[derive(Debug, Clone)]
struct ContinuityResolution {
    signal: ContinuitySignal,
}

impl DefaultMemoryRecallEngine {
    pub fn new(
        policy: MemoryPolicy,
        transcript: SqliteTranscriptStore,
        durable: SqliteDurableMemoryStore,
        working: SqliteWorkingMemoryStore,
    ) -> Self {
        Self {
            policy,
            transcript,
            durable,
            working,
        }
    }

    pub fn has_strong_hit(&self, memories: &[RetrievedMemory]) -> bool {
        memories.iter().any(|memory| {
            memory.relevance_score >= self.policy.strong_hit_score
                || (memory.similarity >= self.policy.strong_hit_similarity
                    && memory.salience >= self.policy.strong_hit_salience)
        })
    }

    fn derive_starter_snapshot(
        &self,
        request: &HydrateWorkingMemoryRequest,
        recent_messages: &[crate::ChatMessage],
    ) -> WorkingMemorySnapshot {
        let created_at = now_rfc3339();
        let thread_key = derive_thread_key(&request.user_text);
        let summary_text = request
            .latest_turn_summary
            .as_ref()
            .map(|summary| summary.user_input_summary.clone())
            .unwrap_or_else(|| compact_text(&request.user_text, 96));
        let message_refs = recent_messages
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let thread = WorkingThread {
            key: thread_key.clone(),
            status: WorkingThreadStatus::Focused,
            topic_label: derive_topic_label(&request.user_text, &summary_text),
            synopsis: compact_text(&summary_text, 140),
            last_touched_turn_id: request.turn_id.clone(),
            last_touched_at: created_at.clone(),
            message_refs: message_refs.clone(),
            durable_memory_ids: Vec::new(),
            score: 1.0,
            stale_turns: 0,
        };

        WorkingMemorySnapshot {
            turn_id: request.turn_id.clone(),
            created_at,
            version: 1,
            focus_thread_key: Some(thread_key.clone()),
            threads: vec![thread],
            entity_anchors: collect_entity_anchors(
                &request.turn_id,
                request.user_text.as_str(),
                thread_key.as_str(),
            ),
            open_loops: collect_open_loops(
                &request.turn_id,
                request.user_text.as_str(),
                thread_key.as_str(),
            ),
            interaction_constraints: collect_constraints(
                &request.turn_id,
                request.user_text.as_str(),
                self.policy.constraint_ttl_turns,
            ),
            recent_tool_outcomes: Vec::new(),
            source_refs: WorkingMemorySourceRefs {
                message_ids: message_refs,
                durable_memory_ids: Vec::new(),
            },
        }
    }

    fn age_snapshot(&self, snapshot: &mut WorkingMemorySnapshot) {
        for thread in &mut snapshot.threads {
            thread.stale_turns = thread.stale_turns.saturating_add(1);
            if thread.status == WorkingThreadStatus::Focused {
                thread.status = WorkingThreadStatus::Active;
            }
        }
        for anchor in &mut snapshot.entity_anchors {
            anchor.stale_turns = anchor.stale_turns.saturating_add(1);
        }
        for loop_item in &mut snapshot.open_loops {
            loop_item.stale_turns = loop_item.stale_turns.saturating_add(1);
        }
        for constraint in &mut snapshot.interaction_constraints {
            constraint.stale_turns = constraint.stale_turns.saturating_add(1);
        }
        for outcome in &mut snapshot.recent_tool_outcomes {
            outcome.stale_turns = outcome.stale_turns.saturating_add(1);
        }
    }

    fn focused_thread<'a>(&self, snapshot: &'a WorkingMemorySnapshot) -> Option<&'a WorkingThread> {
        snapshot
            .focus_thread_key
            .as_ref()
            .and_then(|key| snapshot.threads.iter().find(|thread| thread.key == *key))
            .or_else(|| {
                snapshot
                    .threads
                    .iter()
                    .find(|thread| thread.status == WorkingThreadStatus::Focused)
            })
            .or_else(|| snapshot.threads.first())
    }

    fn starter_snapshot_for_turn(
        &self,
        snapshot: &WorkingMemorySnapshot,
        current_turn_id: &str,
    ) -> bool {
        snapshot.turn_id == current_turn_id
            && !snapshot.threads.is_empty()
            && snapshot
                .threads
                .iter()
                .all(|thread| thread.last_touched_turn_id == current_turn_id)
    }

    fn score_thread_candidate(
        &self,
        snapshot: &WorkingMemorySnapshot,
        thread: &WorkingThread,
        query_text: &str,
        query_tokens: &[String],
        focused_thread_key: Option<&str>,
    ) -> ThreadContinuityCandidate {
        let topic_overlap = overlap_score(
            query_tokens,
            &significant_tokens(&format!("{} {}", thread.topic_label, thread.synopsis)),
        );
        let entity_hits = snapshot
            .entity_anchors
            .iter()
            .filter(|anchor| anchor.thread_key == thread.key)
            .filter(|anchor| query_tokens.iter().any(|token| token == &anchor.label))
            .count();
        let entity_score = (entity_hits as f32 * 0.18).min(0.36);

        let thread_loops = snapshot
            .open_loops
            .iter()
            .filter(|loop_item| loop_item.thread_key == thread.key)
            .collect::<Vec<_>>();
        let short_vague = query_tokens.len() <= 3;
        let (open_loop_key, open_loop_score) = thread_loops
            .iter()
            .map(|loop_item| {
                let overlap =
                    overlap_score(query_tokens, &significant_tokens(loop_item.label.as_str()));
                let boosted = if short_vague
                    && focused_thread_key == Some(thread.key.as_str())
                    && !query_text.trim_end().ends_with('?')
                {
                    overlap.max(0.34)
                } else {
                    overlap
                };
                (loop_item.key.clone(), boosted)
            })
            .max_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| left.0.cmp(&right.0))
            })
            .unwrap_or_else(|| (String::new(), 0.0));
        let open_loop_key = (!open_loop_key.is_empty()).then_some(open_loop_key);

        let tool_score = snapshot
            .recent_tool_outcomes
            .iter()
            .filter(|outcome| outcome.thread_key == thread.key)
            .map(|outcome| {
                overlap_score(query_tokens, &significant_tokens(outcome.summary.as_str()))
            })
            .fold(0.0_f32, f32::max);

        let focus_bonus = if focused_thread_key == Some(thread.key.as_str()) {
            0.06
        } else {
            0.0
        };
        let score = (topic_overlap * 0.6
            + entity_score
            + (open_loop_score * 0.45)
            + (tool_score * 0.2)
            + focus_bonus)
            .min(1.0);

        ThreadContinuityCandidate {
            key: thread.key.clone(),
            status: thread.status.clone(),
            score,
            entity_hits,
            open_loop_key,
            open_loop_score,
            tool_score,
            stale_turns: thread.stale_turns,
            last_touched_at: thread.last_touched_at.clone(),
        }
    }

    fn build_continuity_resolution(
        &self,
        snapshot: &WorkingMemorySnapshot,
        query_text: &str,
        current_turn_id: &str,
    ) -> ContinuityResolution {
        let focused_thread = self.focused_thread(snapshot);
        let focused_thread_key = focused_thread.map(|thread| thread.key.clone());
        let focused_thread_label = focused_thread.map(|thread| thread.topic_label.clone());

        if snapshot.threads.is_empty() || self.starter_snapshot_for_turn(snapshot, current_turn_id)
        {
            return ContinuityResolution {
                signal: ContinuitySignal {
                    focused_thread_key,
                    focused_thread_label,
                    ..ContinuitySignal::default()
                },
            };
        }

        let query_tokens = significant_tokens(query_text);
        if query_tokens.is_empty() {
            return ContinuityResolution {
                signal: ContinuitySignal {
                    focused_thread_key: focused_thread_key.clone(),
                    focused_thread_label,
                    matched_thread_key: focused_thread_key,
                    matched_thread_status: focused_thread.map(|thread| thread.status.clone()),
                    continuity_confidence: 0.0,
                    ..ContinuitySignal::default()
                },
            };
        }

        let focused_thread_key_ref = focused_thread.map(|thread| thread.key.as_str());
        let mut candidates = snapshot
            .threads
            .iter()
            .map(|thread| {
                self.score_thread_candidate(
                    snapshot,
                    thread,
                    query_text,
                    &query_tokens,
                    focused_thread_key_ref,
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| status_rank(&left.status).cmp(&status_rank(&right.status)))
                .then_with(|| left.stale_turns.cmp(&right.stale_turns))
                .then_with(|| right.last_touched_at.cmp(&left.last_touched_at))
                .then_with(|| left.key.cmp(&right.key))
        });

        let best_candidate = candidates.first().cloned();
        let focused_candidate = focused_thread_key_ref.and_then(|focused_key| {
            candidates
                .iter()
                .find(|candidate| candidate.key == focused_key)
                .cloned()
        });
        let short_vague = query_tokens.len() <= 3;
        let best_open_loop = candidates
            .iter()
            .filter(|candidate| candidate.open_loop_key.is_some())
            .max_by(|left, right| {
                left.open_loop_score
                    .partial_cmp(&right.open_loop_score)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| {
                        right
                            .score
                            .partial_cmp(&left.score)
                            .unwrap_or(Ordering::Equal)
                    })
                    .then_with(|| left.key.cmp(&right.key))
            })
            .cloned();

        let mut signal = ContinuitySignal {
            focused_thread_key,
            focused_thread_label,
            ..ContinuitySignal::default()
        };

        if let Some(open_loop_candidate) = best_open_loop {
            let open_loop_threshold = if short_vague { 0.18 } else { 0.24 };
            if open_loop_candidate.open_loop_score >= open_loop_threshold
                || (short_vague && focused_thread_key_ref == Some(open_loop_candidate.key.as_str()))
            {
                signal.mode = ContinuityMode::OpenLoop;
                signal.matched_thread_key = Some(open_loop_candidate.key);
                signal.matched_thread_status = Some(open_loop_candidate.status);
                signal.matched_open_loop_key = open_loop_candidate.open_loop_key;
                signal.open_loop_match = true;
                signal.continuity_confidence = open_loop_candidate
                    .open_loop_score
                    .max(open_loop_candidate.score);
                return ContinuityResolution { signal };
            }
        }

        if let Some(best_candidate) = best_candidate {
            signal.matched_thread_key = Some(best_candidate.key.clone());
            signal.matched_thread_status = Some(best_candidate.status.clone());
            signal.continuity_confidence = best_candidate.score;

            let focused_score = focused_candidate
                .as_ref()
                .map(|candidate| candidate.score)
                .unwrap_or(0.0);
            if focused_thread_key_ref == Some(best_candidate.key.as_str())
                && (best_candidate.score >= 0.18
                    || best_candidate.entity_hits > 0
                    || best_candidate.tool_score >= 0.12)
            {
                signal.mode = ContinuityMode::OnThread;
            } else if focused_thread_key_ref != Some(best_candidate.key.as_str())
                && best_candidate.score >= 0.24
                && matches!(
                    best_candidate.status,
                    WorkingThreadStatus::Cooling | WorkingThreadStatus::Active
                )
            {
                signal.mode = ContinuityMode::Return;
            } else if query_tokens.len() > 3
                && focused_thread_key_ref.is_some()
                && focused_score < 0.12
                && best_candidate.score < 0.24
            {
                signal.mode = ContinuityMode::Pivot;
                signal.matched_thread_key = None;
                signal.matched_thread_status = None;
                signal.continuity_confidence = (1.0 - focused_score).clamp(0.0, 1.0);
            }
        }

        ContinuityResolution { signal }
    }

    fn touch_thread(
        &self,
        thread: &mut WorkingThread,
        request: &RefreshWorkingMemoryRequest,
        touched_message_ids: &[i64],
        touched_memory_ids: &[i64],
    ) {
        thread.status = WorkingThreadStatus::Focused;
        thread.topic_label = derive_topic_label(&request.user_text, &thread.topic_label);
        thread.synopsis = compact_text(
            &format!(
                "{} {}",
                request.user_text,
                compact_text(&request.assistant_visible_text, 96)
            ),
            140,
        );
        thread.last_touched_turn_id = request.turn_id.clone();
        thread.last_touched_at = now_rfc3339();
        thread.score = 1.0;
        thread.stale_turns = 0;
        merge_unique_i64(&mut thread.message_refs, touched_message_ids);
        merge_unique_i64(&mut thread.durable_memory_ids, touched_memory_ids);
    }

    fn load_thread_messages(
        &self,
        snapshot: &WorkingMemorySnapshot,
        continuity: &ContinuitySignal,
        recent_limit: usize,
    ) -> Result<Vec<crate::ChatMessage>> {
        let Some(thread_key) = continuity.matched_thread_key.as_ref() else {
            return Ok(Vec::new());
        };
        let Some(thread) = snapshot
            .threads
            .iter()
            .find(|thread| thread.key == *thread_key)
        else {
            return Ok(Vec::new());
        };
        let mut messages = self.transcript.load_messages_by_ids(&thread.message_refs)?;
        if messages.len() > recent_limit {
            let skip = messages.len().saturating_sub(recent_limit);
            messages = messages.into_iter().skip(skip).collect();
        }
        Ok(messages)
    }

    fn load_thread_memories(
        &self,
        snapshot: &WorkingMemorySnapshot,
        continuity: &ContinuitySignal,
        warm_limit: usize,
        query_text: &str,
    ) -> Result<Vec<RetrievedMemory>> {
        let Some(thread_key) = continuity.matched_thread_key.as_ref() else {
            return Ok(Vec::new());
        };
        let Some(thread) = snapshot
            .threads
            .iter()
            .find(|thread| thread.key == *thread_key)
        else {
            return Ok(Vec::new());
        };
        let mut memories = self
            .durable
            .load_active_by_ids(&thread.durable_memory_ids)?;
        if memories.len() > warm_limit {
            memories.truncate(warm_limit);
        }
        Ok(self.rescore_memories(query_text, memories))
    }

    fn rescore_memories(
        &self,
        query_text: &str,
        memories: Vec<RetrievedMemory>,
    ) -> Vec<RetrievedMemory> {
        let query_vector = vectorize_text(query_text, self.policy.vector_dimensions);
        memories
            .into_iter()
            .map(|mut memory| {
                let memory_vector = vectorize_text(&memory.text, self.policy.vector_dimensions);
                memory.similarity = cosine_similarity(&query_vector, &memory_vector);
                memory.relevance_score =
                    (memory.similarity * 0.7) + (memory.salience.clamp(0.0, 1.0) * 0.3);
                memory
            })
            .collect()
    }

    fn merge_recent_messages(
        &self,
        mut thread_messages: Vec<crate::ChatMessage>,
        fallback_recent_messages: Vec<crate::ChatMessage>,
        recent_limit: usize,
    ) -> Vec<crate::ChatMessage> {
        let thread_cap = recent_limit.min(self.policy.max_recent_messages_per_turn);
        if thread_messages.len() > thread_cap {
            let skip = thread_messages.len().saturating_sub(thread_cap);
            thread_messages = thread_messages.into_iter().skip(skip).collect();
        }

        let mut merged = Vec::new();
        let mut seen_ids = HashSet::new();
        for message in thread_messages
            .into_iter()
            .chain(fallback_recent_messages.into_iter())
        {
            if seen_ids.insert(message.id) {
                merged.push(message);
            }
            if merged.len() >= recent_limit {
                break;
            }
        }
        merged.sort_by_key(|message| message.id);
        merged
    }

    fn merge_durable_memories(
        &self,
        prioritized: Vec<RetrievedMemory>,
        search_hits: Vec<RetrievedMemory>,
        warm_limit: usize,
    ) -> Vec<RetrievedMemory> {
        let mut merged = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut seen_text = HashSet::new();
        let mut seen_keys = HashSet::new();

        for memory in prioritized.into_iter().chain(search_hits.into_iter()) {
            if !seen_ids.insert(memory.id) {
                continue;
            }
            let normalized_text = normalize_memory_text(&memory.text);
            let duplicate_text = !normalized_text.is_empty() && !seen_text.insert(normalized_text);
            let duplicate_key = memory
                .canonical_key
                .as_ref()
                .is_some_and(|key| !seen_keys.insert(key.clone()));
            if duplicate_text || duplicate_key {
                continue;
            }
            merged.push(memory);
            if merged.len() >= warm_limit {
                break;
            }
        }

        merged
    }

    fn prune_snapshot(&self, snapshot: &mut WorkingMemorySnapshot) {
        snapshot
            .interaction_constraints
            .retain(|constraint| constraint.stale_turns < constraint.expires_after_turns);
        snapshot
            .open_loops
            .retain(|loop_item| loop_item.stale_turns < loop_item.expires_after_turns);
        snapshot
            .recent_tool_outcomes
            .retain(|outcome| outcome.stale_turns < 2);
        snapshot
            .entity_anchors
            .retain(|anchor| anchor.stale_turns < 4);

        snapshot.threads.retain(|thread| {
            thread.status != WorkingThreadStatus::Cooling
                || thread.stale_turns < self.policy.cooling_turn_ttl
        });
        snapshot.threads.sort_by(|left, right| {
            status_rank(&left.status)
                .cmp(&status_rank(&right.status))
                .then_with(|| left.stale_turns.cmp(&right.stale_turns))
                .then_with(|| right.last_touched_at.cmp(&left.last_touched_at))
        });
        snapshot.threads.truncate(self.policy.working_max_threads);
        snapshot
            .entity_anchors
            .truncate(self.policy.working_max_entities);
        snapshot
            .open_loops
            .truncate(self.policy.working_max_open_loops);
        snapshot
            .recent_tool_outcomes
            .truncate(self.policy.working_max_tool_outcomes);
        snapshot.focus_thread_key = snapshot
            .threads
            .iter()
            .find(|thread| thread.status == WorkingThreadStatus::Focused)
            .map(|thread| thread.key.clone())
            .or_else(|| snapshot.threads.first().map(|thread| thread.key.clone()));
    }
}

impl MemoryRecallEngine for DefaultMemoryRecallEngine {
    fn hydrate_working_memory(
        &self,
        request: &HydrateWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        if let Some(snapshot) = self.working.load_latest_snapshot()? {
            return Ok(snapshot);
        }

        let recent_messages =
            self.transcript
                .load_recent_messages(super::types::TranscriptSliceQuery {
                    context_level: request.context_level,
                    exclude_message_id: request.exclude_message_id,
                    limit_override: None,
                })?;
        Ok(self.derive_starter_snapshot(request, &recent_messages))
    }

    fn recall(&self, request: &RecallRequest) -> Result<RecallBundle> {
        let started_at = Instant::now();
        let working_memory = match request.working_memory.clone() {
            Some(snapshot) => snapshot,
            None => self.hydrate_working_memory(&HydrateWorkingMemoryRequest {
                turn_id: request.turn_id.clone(),
                user_text: request.query_text.clone(),
                context_level: request.context_level,
                exclude_message_id: request.exclude_message_id,
                latest_turn_summary: None,
            })?,
        };
        let mut continuity = self
            .build_continuity_resolution(&working_memory, &request.query_text, &request.turn_id)
            .signal;

        let recent_limit = request
            .budget
            .max_recent_messages
            .unwrap_or_else(|| request.context_level.recent_turn_limit().saturating_mul(2));
        let recent_limit = recent_limit.min(self.policy.max_recent_messages_per_turn);
        let fallback_recent_messages =
            self.transcript
                .load_recent_messages(super::types::TranscriptSliceQuery {
                    context_level: request.context_level,
                    exclude_message_id: request.exclude_message_id,
                    limit_override: Some(recent_limit),
                })?;

        let warm_limit = request
            .budget
            .max_durable_memories
            .unwrap_or_else(|| request.context_level.semantic_limit());
        let durable_limit = if request.budget.include_cold_candidates {
            warm_limit.saturating_add(2)
        } else {
            warm_limit
        };
        let durable_hits = self
            .durable
            .search_active(&super::types::DurableRecallQuery {
                query_text: request.query_text.clone(),
                context_level: request.context_level,
                limit: Some(durable_limit),
            })?;
        let use_thread_context = matches!(
            continuity.mode,
            ContinuityMode::OnThread | ContinuityMode::Return | ContinuityMode::OpenLoop
        );
        let thread_messages = if use_thread_context {
            self.load_thread_messages(&working_memory, &continuity, recent_limit)?
        } else {
            Vec::new()
        };
        continuity.selected_thread_message_ids = thread_messages
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let recent_messages =
            self.merge_recent_messages(thread_messages, fallback_recent_messages, recent_limit);

        let thread_memories = if use_thread_context {
            self.load_thread_memories(
                &working_memory,
                &continuity,
                warm_limit,
                &request.query_text,
            )?
        } else {
            Vec::new()
        };
        continuity.selected_thread_memory_ids = thread_memories
            .iter()
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let warm_durable_memories =
            self.merge_durable_memories(thread_memories, durable_hits.clone(), warm_limit);
        let warm_memory_ids = warm_durable_memories
            .iter()
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let cold_candidates = durable_hits
            .into_iter()
            .filter(|memory| !warm_memory_ids.contains(&memory.id))
            .take(durable_limit.saturating_sub(warm_limit))
            .collect::<Vec<_>>();

        let selected_message_ids = recent_messages
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let selected_memory_ids = warm_durable_memories
            .iter()
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let source_breakdown = json!({
            "working_threads": working_memory.threads.len(),
            "continuity_mode": continuity.mode,
            "selected_thread_messages": continuity.selected_thread_message_ids.len(),
            "selected_thread_memories": continuity.selected_thread_memory_ids.len(),
            "recent_messages": recent_messages.len(),
            "durable_memories": warm_durable_memories.len(),
            "cold_candidates": cold_candidates.len(),
        });

        if !selected_memory_ids.is_empty() {
            self.durable
                .mark_recalled(&selected_memory_ids, &now_rfc3339())?;
        }

        let thread_bridges = working_memory
            .threads
            .iter()
            .filter(|thread| thread.status != WorkingThreadStatus::Focused)
            .map(|thread| ThreadBridge {
                thread_key: thread.key.clone(),
                synopsis: compact_text(&thread.synopsis, 120),
                status: thread.status.clone(),
            })
            .collect::<Vec<_>>();

        Ok(RecallBundle {
            refs_used: WorkingMemorySourceRefs {
                message_ids: selected_message_ids.clone(),
                durable_memory_ids: selected_memory_ids.clone(),
            },
            explanation: RecallExplanation {
                memory_used: !warm_durable_memories.is_empty()
                    || !thread_bridges.is_empty()
                    || !continuity.selected_thread_message_ids.is_empty()
                    || continuity.open_loop_match,
                strong_hit: self.has_strong_hit(&warm_durable_memories),
                continuity,
                source_breakdown,
                selected_message_ids,
                selected_memory_ids,
            },
            stats: RecallStats {
                latency_ms: started_at.elapsed().as_millis().min(i64::MAX as u128) as i64,
                recent_count: recent_messages.len(),
                semantic_count: warm_durable_memories.len(),
                working_thread_count: working_memory.threads.len(),
            },
            warm_context: WarmContext {
                recent_messages,
                thread_bridges,
                durable_memories: warm_durable_memories,
            },
            cold_candidates,
            working_memory,
        })
    }

    fn refresh_working_memory(
        &self,
        request: &RefreshWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        let mut snapshot = request
            .previous_snapshot
            .clone()
            .unwrap_or_else(|| request.recall_bundle.working_memory.clone());
        self.age_snapshot(&mut snapshot);

        let mut touched_message_ids = request
            .recall_bundle
            .explanation
            .selected_message_ids
            .clone();
        merge_unique_i64(&mut touched_message_ids, &request.current_turn_message_ids);
        let touched_memory_ids = &request.recall_bundle.explanation.selected_memory_ids;
        let continuity =
            self.build_continuity_resolution(&snapshot, &request.user_text, &request.turn_id);
        let mut resolved_thread_key = None;

        if continuity.signal.mode != ContinuityMode::Pivot {
            let matched_thread_key = continuity.signal.matched_thread_key.as_deref();
            let thread_index = matched_thread_key.and_then(|matched_key| {
                snapshot
                    .threads
                    .iter()
                    .position(|thread| thread.key == matched_key)
            });
            if let Some(index) = thread_index {
                resolved_thread_key = Some(snapshot.threads[index].key.clone());
                for (current_index, thread) in snapshot.threads.iter_mut().enumerate() {
                    if current_index == index {
                        self.touch_thread(
                            thread,
                            request,
                            &touched_message_ids,
                            touched_memory_ids,
                        );
                    } else if matches!(
                        thread.status,
                        WorkingThreadStatus::Focused | WorkingThreadStatus::Active
                    ) {
                        thread.status = WorkingThreadStatus::Cooling;
                    }
                }
            }
        }

        if resolved_thread_key.is_none() {
            for (current_index, thread) in snapshot.threads.iter_mut().enumerate() {
                let _ = current_index;
                if matches!(
                    thread.status,
                    WorkingThreadStatus::Focused | WorkingThreadStatus::Active
                ) {
                    thread.status = WorkingThreadStatus::Cooling;
                }
            }
            let thread_key = derive_thread_key(&request.user_text);
            resolved_thread_key = Some(thread_key.clone());
            snapshot.threads.push(WorkingThread {
                key: thread_key.clone(),
                status: WorkingThreadStatus::Focused,
                topic_label: derive_topic_label(
                    &request.user_text,
                    &request.assistant_visible_text,
                ),
                synopsis: compact_text(
                    &format!(
                        "{} {}",
                        request.user_text,
                        compact_text(&request.assistant_visible_text, 96)
                    ),
                    140,
                ),
                last_touched_turn_id: request.turn_id.clone(),
                last_touched_at: now_rfc3339(),
                message_refs: touched_message_ids.clone(),
                durable_memory_ids: touched_memory_ids.clone(),
                score: 1.0,
                stale_turns: 0,
            });
        }

        snapshot.turn_id = request.turn_id.clone();
        snapshot.created_at = now_rfc3339();
        snapshot.version = 1;
        snapshot.source_refs = request.recall_bundle.refs_used.clone();
        let resolved_thread_key = resolved_thread_key
            .or_else(|| snapshot.focus_thread_key.clone())
            .unwrap_or_else(|| derive_thread_key(&request.user_text));

        let new_entities = collect_entity_anchors(
            &request.turn_id,
            request.user_text.as_str(),
            resolved_thread_key.as_str(),
        );
        for entity in new_entities {
            upsert_entity_anchor(&mut snapshot.entity_anchors, entity);
        }

        let new_loops = collect_open_loops(
            &request.turn_id,
            request.user_text.as_str(),
            resolved_thread_key.as_str(),
        );
        for loop_item in new_loops {
            upsert_open_loop(&mut snapshot.open_loops, loop_item);
        }

        let new_constraints = collect_constraints(
            &request.turn_id,
            request.user_text.as_str(),
            self.policy.constraint_ttl_turns,
        );
        for constraint in new_constraints {
            upsert_constraint(&mut snapshot.interaction_constraints, constraint);
        }

        if let Some(tool_summary) = request
            .tool_summary
            .as_ref()
            .filter(|summary| !summary.trim().is_empty())
        {
            snapshot.recent_tool_outcomes.insert(
                0,
                WorkingToolOutcome {
                    tool_name: "tool".to_owned(),
                    action: "summary".to_owned(),
                    summary: compact_text(tool_summary, 140),
                    turn_id: request.turn_id.clone(),
                    created_at: snapshot.created_at.clone(),
                    thread_key: resolved_thread_key.clone(),
                    stale_turns: 0,
                },
            );
        }

        self.prune_snapshot(&mut snapshot);
        Ok(snapshot)
    }
}

fn derive_thread_key(text: &str) -> String {
    let tokens = significant_tokens(text);
    if tokens.is_empty() {
        "thread:general".to_owned()
    } else {
        format!(
            "thread:{}",
            tokens.into_iter().take(4).collect::<Vec<_>>().join("_")
        )
    }
}

fn derive_topic_label(primary: &str, fallback: &str) -> String {
    let label = significant_tokens(primary)
        .into_iter()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        compact_text(fallback, 48)
    } else {
        compact_text(&label, 48)
    }
}

fn significant_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character.is_whitespace() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|token| token.len() >= 3 && !is_stop_word(token))
        .map(ToOwned::to_owned)
        .collect()
}

fn overlap_score(left: &[String], right: &[String]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let left_set = left.iter().cloned().collect::<HashSet<_>>();
    let right_set = right.iter().cloned().collect::<HashSet<_>>();
    let intersection = left_set.intersection(&right_set).count() as f32;
    let union = left_set.union(&right_set).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn compact_text(text: &str, limit: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        normalized
    } else {
        let truncated = normalized
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>();
        format!("{truncated}…")
    }
}

fn merge_unique_i64(target: &mut Vec<i64>, values: &[i64]) {
    let mut seen = target.iter().copied().collect::<HashSet<_>>();
    for value in values {
        if seen.insert(*value) {
            target.push(*value);
        }
    }
}

fn status_rank(status: &WorkingThreadStatus) -> usize {
    match status {
        WorkingThreadStatus::Focused => 0,
        WorkingThreadStatus::Active => 1,
        WorkingThreadStatus::Cooling => 2,
    }
}

fn collect_entity_anchors(turn_id: &str, user_text: &str, thread_key: &str) -> Vec<EntityAnchor> {
    significant_tokens(user_text)
        .into_iter()
        .take(3)
        .map(|token| EntityAnchor {
            key: format!("entity:{token}"),
            label: token,
            kind: "topic_token".to_owned(),
            thread_key: thread_key.to_owned(),
            last_seen_turn_id: turn_id.to_owned(),
            stale_turns: 0,
        })
        .collect()
}

fn collect_open_loops(turn_id: &str, user_text: &str, thread_key: &str) -> Vec<OpenLoop> {
    if !user_text.trim_end().ends_with('?') {
        return Vec::new();
    }
    vec![OpenLoop {
        key: format!("loop:{thread_key}"),
        label: compact_text(user_text, 96),
        thread_key: thread_key.to_owned(),
        opened_turn_id: turn_id.to_owned(),
        last_touched_turn_id: turn_id.to_owned(),
        expires_after_turns: 4,
        stale_turns: 0,
    }]
}

fn collect_constraints(turn_id: &str, user_text: &str, ttl: u32) -> Vec<InteractionConstraint> {
    let lower = user_text.to_lowercase();
    let mut constraints = Vec::new();
    if lower.contains("brief") || lower.contains("concise") {
        constraints.push(InteractionConstraint {
            key: "constraint:brevity".to_owned(),
            text: "Keep responses brief.".to_owned(),
            source: "user_text".to_owned(),
            last_confirmed_turn_id: turn_id.to_owned(),
            expires_after_turns: ttl,
            stale_turns: 0,
        });
    }
    constraints
}

fn upsert_entity_anchor(target: &mut Vec<EntityAnchor>, entity: EntityAnchor) {
    if let Some(existing) = target.iter_mut().find(|anchor| anchor.key == entity.key) {
        existing.label = entity.label;
        existing.kind = entity.kind;
        existing.thread_key = entity.thread_key;
        existing.last_seen_turn_id = entity.last_seen_turn_id;
        existing.stale_turns = 0;
    } else {
        target.push(entity);
    }
}

fn upsert_open_loop(target: &mut Vec<OpenLoop>, loop_item: OpenLoop) {
    if let Some(existing) = target
        .iter_mut()
        .find(|current| current.key == loop_item.key)
    {
        existing.label = loop_item.label;
        existing.thread_key = loop_item.thread_key;
        existing.last_touched_turn_id = loop_item.last_touched_turn_id;
        existing.stale_turns = 0;
    } else {
        target.push(loop_item);
    }
}

fn upsert_constraint(target: &mut Vec<InteractionConstraint>, constraint: InteractionConstraint) {
    if let Some(existing) = target
        .iter_mut()
        .find(|current| current.key == constraint.key)
    {
        existing.text = constraint.text;
        existing.source = constraint.source;
        existing.last_confirmed_turn_id = constraint.last_confirmed_turn_id;
        existing.expires_after_turns = constraint.expires_after_turns;
        existing.stale_turns = 0;
    } else {
        target.push(constraint);
    }
}

fn is_stop_word(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "that"
            | "this"
            | "from"
            | "have"
            | "what"
            | "when"
            | "where"
            | "which"
            | "about"
            | "your"
            | "my"
            | "our"
    )
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::memory::{AppendMessageRequest, GroundedMemoryWrite, RecallBudget, RecallRequest};
    use crate::AppConfig;
    use crate::{ContextLevel, InputSource, MessageContentType};

    use super::*;
    use crate::memory::{SqliteMemoryDb, SqliteWorkingMemoryStore};

    fn policy() -> MemoryPolicy {
        MemoryPolicy::from_app_config(&AppConfig {
            default_previous_context: ContextLevel::Medium,
            vector_dimensions: 32,
            memory_salience_threshold: 0.6,
            stream_chunk_size: 32,
            max_recent_messages_per_turn: 32,
            max_model_logs: 20,
            idle_resume_threshold_seconds: 900,
            ambient_cooldown_seconds: 600,
        })
    }

    #[test]
    fn refresh_working_memory_creates_new_thread_on_clear_pivot_and_reactivates_prior_topic() {
        let temp = tempdir().expect("tempdir");
        let db = std::sync::Arc::new(
            SqliteMemoryDb::new(temp.path().join("memory.sqlite3")).expect("db"),
        );
        let transcript = SqliteTranscriptStore::new(db.clone(), 32);
        let durable = SqliteDurableMemoryStore::new(db.clone(), 32);
        let working = SqliteWorkingMemoryStore::new(db);
        let engine = DefaultMemoryRecallEngine::new(policy(), transcript, durable, working);

        let starter = WorkingMemorySnapshot {
            turn_id: "turn-1".to_owned(),
            created_at: now_rfc3339(),
            version: 1,
            focus_thread_key: Some("thread:memory".to_owned()),
            threads: vec![WorkingThread {
                key: "thread:memory".to_owned(),
                status: WorkingThreadStatus::Focused,
                topic_label: "memory design hot state".to_owned(),
                synopsis: "We are discussing memory design and hot state.".to_owned(),
                last_touched_turn_id: "turn-1".to_owned(),
                last_touched_at: now_rfc3339(),
                message_refs: vec![],
                durable_memory_ids: vec![],
                score: 1.0,
                stale_turns: 0,
            }],
            entity_anchors: vec![EntityAnchor {
                key: "entity:memory".to_owned(),
                label: "memory".to_owned(),
                kind: "topic_token".to_owned(),
                thread_key: "thread:memory".to_owned(),
                last_seen_turn_id: "turn-1".to_owned(),
                stale_turns: 0,
            }],
            open_loops: vec![],
            interaction_constraints: vec![],
            recent_tool_outcomes: vec![],
            source_refs: WorkingMemorySourceRefs::default(),
        };

        let pivot_bundle = RecallBundle {
            working_memory: starter.clone(),
            warm_context: WarmContext::default(),
            cold_candidates: vec![],
            refs_used: WorkingMemorySourceRefs::default(),
            explanation: RecallExplanation::default(),
            stats: RecallStats::default(),
        };
        let pivoted = engine
            .refresh_working_memory(&RefreshWorkingMemoryRequest {
                turn_id: "turn-2".to_owned(),
                user_text: "Let's switch to gardening soil moisture now.".to_owned(),
                assistant_visible_text: "We can map the watering schedule next.".to_owned(),
                tool_summary: None,
                current_turn_message_ids: vec![21, 22],
                recall_bundle: pivot_bundle,
                previous_snapshot: Some(starter),
            })
            .expect("refresh");
        assert_eq!(pivoted.threads[0].status, WorkingThreadStatus::Focused);
        assert!(pivoted
            .threads
            .iter()
            .any(|thread| thread.status == WorkingThreadStatus::Cooling));

        let return_bundle = RecallBundle {
            working_memory: pivoted.clone(),
            warm_context: WarmContext::default(),
            cold_candidates: vec![],
            refs_used: WorkingMemorySourceRefs::default(),
            explanation: RecallExplanation::default(),
            stats: RecallStats::default(),
        };
        let returned = engine
            .refresh_working_memory(&RefreshWorkingMemoryRequest {
                turn_id: "turn-3".to_owned(),
                user_text: "Back to memory design and hot state.".to_owned(),
                assistant_visible_text: "We can return to the memory thread.".to_owned(),
                tool_summary: None,
                current_turn_message_ids: vec![31, 32],
                recall_bundle: return_bundle,
                previous_snapshot: Some(pivoted),
            })
            .expect("refresh");
        assert_eq!(returned.focus_thread_key.as_deref(), Some("thread:memory"));
        assert!(returned.threads.len() <= policy().working_max_threads);
    }

    #[test]
    fn recall_is_deterministic_for_same_snapshot_query_and_refs() {
        let temp = tempdir().expect("tempdir");
        let db = std::sync::Arc::new(
            SqliteMemoryDb::new(temp.path().join("memory.sqlite3")).expect("db"),
        );
        let transcript = SqliteTranscriptStore::new(db.clone(), 32);
        let durable = SqliteDurableMemoryStore::new(db.clone(), 32);
        let working = SqliteWorkingMemoryStore::new(db);
        let engine =
            DefaultMemoryRecallEngine::new(policy(), transcript.clone(), durable.clone(), working);

        let user_message = transcript
            .append_message(AppendMessageRequest {
                role: "user".to_owned(),
                content: "My cat is Mocha".to_owned(),
                turn_id: "turn-1".to_owned(),
                input_source: InputSource::Text,
                content_type: MessageContentType::PlainText,
                display_json: None,
                visible_summary: None,
                meta_json: None,
            })
            .expect("user message");
        let assistant_message = transcript
            .append_message(AppendMessageRequest {
                role: "assistant".to_owned(),
                content: "Mocha is your cat.".to_owned(),
                turn_id: "turn-1".to_owned(),
                input_source: InputSource::Text,
                content_type: MessageContentType::PlainText,
                display_json: None,
                visible_summary: Some("Mocha is your cat.".to_owned()),
                meta_json: None,
            })
            .expect("assistant message");
        let durable_memory = durable
            .promote_grounded_memory(&GroundedMemoryWrite {
                kind: "fact",
                canonical_key: "profile:cat",
                text: "Your cat is Mocha.",
                salience: 0.95,
                source_message_id: Some(user_message.id),
                source_type: "user_turn",
                source_ref: "turn-1",
            })
            .expect("durable memory");
        let thread_key = derive_thread_key("My cat is Mocha");
        let snapshot = WorkingMemorySnapshot {
            turn_id: "turn-1".to_owned(),
            created_at: now_rfc3339(),
            version: 1,
            focus_thread_key: Some(thread_key.clone()),
            threads: vec![WorkingThread {
                key: thread_key.clone(),
                status: WorkingThreadStatus::Focused,
                topic_label: "cat mocha".to_owned(),
                synopsis: "We are discussing your cat Mocha.".to_owned(),
                last_touched_turn_id: "turn-1".to_owned(),
                last_touched_at: now_rfc3339(),
                message_refs: vec![user_message.id, assistant_message.id],
                durable_memory_ids: vec![durable_memory.id],
                score: 1.0,
                stale_turns: 0,
            }],
            entity_anchors: vec![EntityAnchor {
                key: "entity:mocha".to_owned(),
                label: "mocha".to_owned(),
                kind: "topic_token".to_owned(),
                thread_key: thread_key.clone(),
                last_seen_turn_id: "turn-1".to_owned(),
                stale_turns: 0,
            }],
            open_loops: vec![],
            interaction_constraints: vec![],
            recent_tool_outcomes: vec![],
            source_refs: WorkingMemorySourceRefs::default(),
        };
        let request = RecallRequest {
            turn_id: "inspect-turn".to_owned(),
            query_text: "Tell me about Mocha".to_owned(),
            intent: "test".to_owned(),
            context_level: ContextLevel::Medium,
            budget: RecallBudget {
                max_recent_messages: Some(6),
                max_durable_memories: Some(4),
                include_cold_candidates: false,
            },
            working_memory: Some(snapshot),
            exclude_message_id: None,
        };

        let first = engine.recall(&request).expect("first recall");
        let second = engine.recall(&request).expect("second recall");

        assert_eq!(first.explanation.continuity, second.explanation.continuity);
        assert_eq!(
            first.explanation.selected_message_ids,
            second.explanation.selected_message_ids
        );
        assert_eq!(
            first.explanation.selected_memory_ids,
            second.explanation.selected_memory_ids
        );
        assert_eq!(first.explanation.continuity.mode, ContinuityMode::OnThread);
        assert_eq!(
            first.explanation.continuity.selected_thread_message_ids,
            vec![user_message.id, assistant_message.id]
        );
        assert_eq!(
            first.explanation.continuity.selected_thread_memory_ids,
            vec![durable_memory.id]
        );
    }
}
