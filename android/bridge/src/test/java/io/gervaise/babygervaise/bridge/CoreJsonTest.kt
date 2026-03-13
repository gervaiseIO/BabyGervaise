package io.gervaise.babygervaise.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CoreJsonTest {
    @Test
    fun decodeEventParsesAssistantStarted() {
        val event = CoreJson.decodeEvent(
            eventType = "assistant_started",
            payloadJson = """{"turnId":"turn-1"}""",
        )

        assertEquals(CoreEvent.AssistantStarted("turn-1"), event)
    }

    @Test
    fun decodeEventParsesAssistantChunk() {
        val event = CoreJson.decodeEvent(
            eventType = "assistant_chunk",
            payloadJson = """{"turnId":"turn-1","chunk":"Hello"}""",
        )

        assertEquals(CoreEvent.AssistantChunk(turnId = "turn-1", chunk = "Hello"), event)
    }

    @Test
    fun decodeEventParsesAssistantCompleted() {
        val event = CoreJson.decodeEvent(
            eventType = "assistant_completed",
            payloadJson = """
                {
                  "turnId":"turn-1",
                  "message":{
                    "id":7,
                    "role":"assistant",
                    "content":"Done.",
                    "turn_id":"turn-1",
                    "input_source":"text",
                    "created_at":"2026-01-01T10:00:00Z"
                  }
                }
            """.trimIndent(),
        )

        assertEquals(
            CoreEvent.AssistantCompleted(
                turnId = "turn-1",
                message = ChatMessage(
                    id = 7,
                    role = "assistant",
                    content = "Done.",
                    turnId = "turn-1",
                    inputSource = InputSource.TEXT,
                    createdAt = "2026-01-01T10:00:00Z",
                ),
            ),
            event,
        )
    }

    @Test
    fun decodeEventParsesToolStatus() {
        val event = CoreJson.decodeEvent(
            eventType = "tool_status",
            payloadJson = """{"turnId":"turn-1","tool":"hue","action":"set_color","status":"executing"}""",
        )

        assertEquals(
            CoreEvent.ToolStatus(
                turnId = "turn-1",
                tool = "hue",
                action = "set_color",
                status = "executing",
            ),
            event,
        )
    }

    @Test
    fun decodeEventParsesAssistantError() {
        val event = CoreJson.decodeEvent(
            eventType = "assistant_error",
            payloadJson = """{"turnId":null,"error":"Bridge failure"}""",
        )

        assertEquals(
            CoreEvent.AssistantError(turnId = null, error = "Bridge failure"),
            event,
        )
    }

    @Test
    fun decodeEventParsesConfigUpdated() {
        val event = CoreJson.decodeEvent(
            eventType = "config_updated",
            payloadJson = """{"level":"high"}""",
        )

        assertEquals(CoreEvent.ConfigUpdated(ContextLevel.HIGH), event)
    }

    @Test
    fun decodeBootstrapStateIgnoresUnknownFields() {
        val snapshot = CoreJson.decodeBootstrapState(
            """
                {
                  "previous_context":"medium",
                  "messages":[
                    {
                      "id":1,
                      "role":"assistant",
                      "content":"Hello again.",
                      "turn_id":"turn-0",
                      "input_source":"text",
                      "created_at":"2026-01-01T10:00:00Z",
                      "ignored":"value"
                    }
                  ],
                  "extra_field":true
                }
            """.trimIndent(),
        )

        assertEquals(ContextLevel.MEDIUM, snapshot.previousContext)
        assertEquals(1, snapshot.messages.size)
        assertEquals("Hello again.", snapshot.messages.first().content)
    }

    @Test
    fun decodeOverviewSnapshotIgnoresUnknownFields() {
        val snapshot = CoreJson.decodeOverviewSnapshot(
            """
                {
                  "previous_context":"medium",
                  "model_stats":{
                    "model_name":"gpt-4o-mini",
                    "total_requests":2,
                    "total_input_tokens":10,
                    "total_output_tokens":20,
                    "average_latency_ms":120,
                    "latest_latency_ms":140
                  },
                  "memory_stats":{
                    "message_count":4,
                    "stored_memories":2,
                    "vector_count":2,
                    "retrieval_count":1
                  },
                  "system_stats":{
                    "total_interactions":2,
                    "tool_calls":1,
                    "error_count":0
                  },
                  "tool_states":{
                    "hue":{"power":true,"brightness":70}
                  },
                  "recent_logs":[
                    {
                      "timestamp":"2026-01-01T10:00:00Z",
                      "prompt":"{}",
                      "raw_output":"{}",
                      "latency_ms":140,
                      "status":200,
                      "ignored":"value"
                    }
                  ],
                  "extra_field":"ignored"
                }
            """.trimIndent(),
        )

        assertEquals("gpt-4o-mini", snapshot.modelStats.modelName)
        assertEquals(1, snapshot.toolStates.size)
        assertTrue(snapshot.toolStates.containsKey("hue"))
        assertEquals(1, snapshot.recentLogs.size)
        assertEquals(200, snapshot.recentLogs.first().status)
    }
}
