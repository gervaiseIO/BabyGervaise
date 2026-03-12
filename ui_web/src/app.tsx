import { useEffect, useMemo, useState } from "preact/hooks";

import { bootstrap, listenToCoreEvents, submitMessage, updateContextLevel } from "./bridge";
import { BootstrapState, ChatMessage, ContextLevel, CoreEvent, OverviewSnapshot } from "./types";

type Screen = "chat" | "overview";

const emptyBootstrap: BootstrapState = {
  previous_context: "medium",
  messages: [],
};

const emptyOverview: OverviewSnapshot = {
  previous_context: "medium",
  model_stats: {
    model_name: "unconfigured",
    total_requests: 0,
    total_input_tokens: 0,
    total_output_tokens: 0,
    average_latency_ms: 0,
    latest_latency_ms: 0,
  },
  memory_stats: {
    message_count: 0,
    stored_memories: 0,
    vector_count: 0,
    retrieval_count: 0,
  },
  system_stats: {
    total_interactions: 0,
    tool_calls: 0,
    error_count: 0,
  },
  tool_states: {},
  recent_logs: [],
};

function createLocalMessage(role: string, content: string, turnId: string): ChatMessage {
  return {
    id: Date.now(),
    role,
    content,
    turn_id: turnId,
    input_source: "text",
    created_at: new Date().toISOString(),
  };
}

function nextTurnId() {
  return globalThis.crypto?.randomUUID?.() ?? `turn-${Date.now()}`;
}

export function App() {
  const [screen, setScreen] = useState<Screen>("chat");
  const [bootstrapState, setBootstrapState] = useState<BootstrapState>(emptyBootstrap);
  const [overview, setOverview] = useState<OverviewSnapshot>(emptyOverview);
  const [draft, setDraft] = useState("");
  const [toolStatus, setToolStatus] = useState<string | null>(null);
  const [pendingTurnId, setPendingTurnId] = useState<string | null>(null);

  useEffect(() => {
    const unsubscribe = listenToCoreEvents((event) => {
      handleCoreEvent(event);
    });
    bootstrap();
    return unsubscribe;
  }, []);

  const handleCoreEvent = (event: CoreEvent) => {
    if (event.type === "bootstrap_state") {
      setBootstrapState(event.payload);
      return;
    }

    if (event.type === "overview_state") {
      setOverview(event.payload);
      return;
    }

    if (event.type === "config_updated") {
      setBootstrapState((current) => ({
        ...current,
        previous_context: event.payload.level,
      }));
      setOverview((current) => ({
        ...current,
        previous_context: event.payload.level,
      }));
      return;
    }

    if (event.type === "assistant_started") {
      setPendingTurnId(event.payload.turnId);
      setToolStatus(null);
      setBootstrapState((current) => ({
        ...current,
        messages: [...current.messages, createLocalMessage("assistant", "", event.payload.turnId)],
      }));
      return;
    }

    if (event.type === "assistant_chunk") {
      setBootstrapState((current) => ({
        ...current,
        messages: current.messages.map((message) =>
          message.turn_id === event.payload.turnId && message.role === "assistant"
            ? { ...message, content: `${message.content} ${event.payload.chunk}`.trim() }
            : message,
        ),
      }));
      return;
    }

    if (event.type === "assistant_completed") {
      setPendingTurnId(null);
      setBootstrapState((current) => ({
        ...current,
        messages: current.messages.map((message) =>
          message.turn_id === event.payload.turnId && message.role === "assistant"
            ? event.payload.message
            : message,
        ),
      }));
      return;
    }

    if (event.type === "tool_status") {
      setToolStatus(`${event.payload.tool}.${event.payload.action} is ${event.payload.status}`);
      return;
    }

    if (event.type === "assistant_error") {
      setPendingTurnId(null);
      setToolStatus(event.payload.error);
    }
  };

  const groupedToolState = useMemo(
    () => Object.entries(overview.tool_states),
    [overview.tool_states],
  );

  const onSubmit = (event: Event) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text) {
      return;
    }

    const turnId = nextTurnId();
    setBootstrapState((current) => ({
      ...current,
      messages: [...current.messages, createLocalMessage("user", text, turnId)],
    }));
    setDraft("");
    setPendingTurnId(turnId);
    submitMessage(turnId, text);
  };

  const changeContextLevel = (level: ContextLevel) => {
    setBootstrapState((current) => ({ ...current, previous_context: level }));
    setOverview((current) => ({ ...current, previous_context: level }));
    updateContextLevel(level);
  };

  return (
    <div className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">Continuous Intelligence Prototype</p>
          <h1>Baby Gervaise</h1>
        </div>
        <button className="toggle" onClick={() => setScreen(screen === "chat" ? "overview" : "chat")}>
          {screen === "chat" ? "Overview" : "Back to Chat"}
        </button>
      </header>

      {screen === "chat" ? (
        <section className="chat-screen">
          <div className="timeline" data-testid="timeline">
            {bootstrapState.messages.length === 0 ? (
              <article className="empty-state">
                <h2>One conversation. No reset.</h2>
                <p>Start speaking with Gervaise. This timeline is the whole relationship.</p>
              </article>
            ) : null}

            {bootstrapState.messages.map((message) => (
              <article key={`${message.turn_id}-${message.id}-${message.role}`} className={`bubble ${message.role}`}>
                <span className="role">{message.role}</span>
                <p>{message.content || (pendingTurnId === message.turn_id ? "…" : "")}</p>
              </article>
            ))}
          </div>

          <aside className="status-bar">
            <span>Previous Context: {bootstrapState.previous_context}</span>
            <span>{toolStatus ?? "HGIE ready."}</span>
          </aside>

          <form className="composer" onSubmit={onSubmit}>
            <textarea
              placeholder="Tell Gervaise what you need."
              rows={3}
              value={draft}
              onInput={(event) => setDraft((event.target as HTMLTextAreaElement).value)}
            />
            <button type="submit">Send</button>
          </form>
        </section>
      ) : (
        <section className="overview-screen" data-testid="overview">
          <div className="stats-grid">
            <StatsCard
              title="Model"
              lines={[
                `Name: ${overview.model_stats.model_name}`,
                `Requests: ${overview.model_stats.total_requests}`,
                `Tokens in/out: ${overview.model_stats.total_input_tokens} / ${overview.model_stats.total_output_tokens}`,
                `Latency avg/latest: ${overview.model_stats.average_latency_ms}ms / ${overview.model_stats.latest_latency_ms}ms`,
              ]}
            />
            <StatsCard
              title="Memory"
              lines={[
                `Messages: ${overview.memory_stats.message_count}`,
                `Stored memories: ${overview.memory_stats.stored_memories}`,
                `Vectors: ${overview.memory_stats.vector_count}`,
                `Retrievals: ${overview.memory_stats.retrieval_count}`,
              ]}
            />
            <StatsCard
              title="System"
              lines={[
                `Interactions: ${overview.system_stats.total_interactions}`,
                `Tool calls: ${overview.system_stats.tool_calls}`,
                `Errors: ${overview.system_stats.error_count}`,
              ]}
            />
          </div>

          <div className="overview-panel">
            <label className="context-control">
              <span>Previous Context</span>
              <select
                value={overview.previous_context}
                onChange={(event) => changeContextLevel((event.target as HTMLSelectElement).value as ContextLevel)}
              >
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
              </select>
            </label>

            <div className="tool-state">
              <h3>Tool State</h3>
              {groupedToolState.length === 0 ? <p>No tool state recorded yet.</p> : null}
              {groupedToolState.map(([key, value]) => (
                <pre key={key}>{JSON.stringify({ [key]: value }, null, 2)}</pre>
              ))}
            </div>
          </div>

          <div className="log-viewer">
            <h3>Raw Model Logs</h3>
            {overview.recent_logs.length === 0 ? <p>No model logs yet.</p> : null}
            {overview.recent_logs.map((entry) => (
              <details key={`${entry.timestamp}-${entry.latency_ms}`}>
                <summary>
                  <span>{entry.timestamp}</span>
                  <span>{entry.latency_ms}ms</span>
                  <span>Status {entry.status ?? "n/a"}</span>
                </summary>
                <pre>{entry.prompt}</pre>
                <pre>{entry.raw_output}</pre>
              </details>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

type StatsCardProps = {
  title: string;
  lines: string[];
};

function StatsCard({ title, lines }: StatsCardProps) {
  return (
    <article className="stats-card">
      <h2>{title}</h2>
      {lines.map((line) => (
        <p key={line}>{line}</p>
      ))}
    </article>
  );
}

