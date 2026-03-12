import { fireEvent, render, screen } from "@testing-library/preact";

import { App } from "./app";

function emit(type: string, payload: unknown) {
  window.dispatchEvent(
    new CustomEvent("baby-gervaise-event", {
      detail: { type, payload },
    }),
  );
}

describe("App", () => {
  beforeEach(() => {
    window.BabyGervaiseBridge = {
      postMessage: vi.fn(),
    };
  });

  it("renders bootstrap messages into the continuous timeline", async () => {
    render(<App />);

    emit("bootstrap_state", {
      previous_context: "medium",
      messages: [
        {
          id: 1,
          role: "assistant",
          content: "Hello again.",
          turn_id: "turn-0",
          input_source: "text",
          created_at: "2026-01-01T10:00:00Z",
        },
      ],
    });

    expect(await screen.findByText("Hello again.")).toBeInTheDocument();
  });

  it("shows overview stats and updates context level", async () => {
    render(<App />);

    fireEvent.click(screen.getByText("Overview"));
    emit("overview_state", {
      previous_context: "medium",
      model_stats: {
        model_name: "gpt-4o-mini",
        total_requests: 2,
        total_input_tokens: 10,
        total_output_tokens: 20,
        average_latency_ms: 120,
        latest_latency_ms: 140,
      },
      memory_stats: {
        message_count: 4,
        stored_memories: 2,
        vector_count: 2,
        retrieval_count: 1,
      },
      system_stats: {
        total_interactions: 2,
        tool_calls: 1,
        error_count: 0,
      },
      tool_states: {},
      recent_logs: [],
    });

    expect(await screen.findByText("Name: gpt-4o-mini")).toBeInTheDocument();
    fireEvent.change(screen.getByDisplayValue("Medium"), {
      target: { value: "high" },
    });
    expect(window.BabyGervaiseBridge?.postMessage).toHaveBeenCalled();
  });
});
