export type ContextLevel = "low" | "medium" | "high";

export type ChatMessage = {
  id: number;
  role: string;
  content: string;
  turn_id: string;
  input_source: "text" | "voice";
  created_at: string;
};

export type BootstrapState = {
  previous_context: ContextLevel;
  messages: ChatMessage[];
};

export type OverviewSnapshot = {
  previous_context: ContextLevel;
  model_stats: {
    model_name: string;
    total_requests: number;
    total_input_tokens: number;
    total_output_tokens: number;
    average_latency_ms: number;
    latest_latency_ms: number;
  };
  memory_stats: {
    message_count: number;
    stored_memories: number;
    vector_count: number;
    retrieval_count: number;
  };
  system_stats: {
    total_interactions: number;
    tool_calls: number;
    error_count: number;
  };
  tool_states: Record<string, unknown>;
  recent_logs: Array<{
    timestamp: string;
    prompt: string;
    raw_output: string;
    latency_ms: number;
    status?: number;
  }>;
};

export type CoreEvent =
  | { type: "bootstrap_state"; payload: BootstrapState }
  | { type: "overview_state"; payload: OverviewSnapshot }
  | { type: "assistant_started"; payload: { turnId: string } }
  | { type: "assistant_chunk"; payload: { turnId: string; chunk: string } }
  | { type: "assistant_completed"; payload: { turnId: string; message: ChatMessage } }
  | { type: "tool_status"; payload: { turnId: string; tool: string; action: string; status: string } }
  | { type: "assistant_error"; payload: { turnId?: string | null; error: string } }
  | { type: "config_updated"; payload: { level: ContextLevel } };

