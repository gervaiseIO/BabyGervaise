package io.gervaise.babygervaise.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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
                    contentType = MessageContentType.PLAIN_TEXT,
                    displayJson = null,
                    visibleSummary = null,
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
    fun decodeEventParsesDiagnosticLog() {
        val event = CoreJson.decodeEvent(
            eventType = "diagnostic_log",
            payloadJson = """{"subsystem":"tools","level":"info","message":"tool action completed","turn_id":"turn-1","fields":{"tool":"spotify"}}""",
        )

        val entry = (event as CoreEvent.DebugLog).entry
        assertEquals("tools", entry.subsystem)
        assertEquals("info", entry.level)
        assertEquals("tool action completed", entry.message)
        assertEquals("turn-1", entry.turnId)
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
                      "content_type":"plain_text",
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
    fun decodeToolExecutionResultParsesPayload() {
        val result = CoreJson.decodeToolExecutionResult(
            """
                {
                  "tool":"spotify",
                  "action":"disconnect",
                  "summary":"Spotify has been disconnected. You can sign in again whenever you want.",
                  "state_json":{"connected":false},
                  "result_json":{"status":"success","connected":false}
                }
            """.trimIndent(),
        )

        assertEquals("spotify", result.tool)
        assertEquals("disconnect", result.action)
        assertEquals(
            "Spotify has been disconnected. You can sign in again whenever you want.",
            result.summary,
        )
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
                  "cloud_stats":{
                    "calls":2,
                    "tokens_in":10,
                    "tokens_out":20,
                    "latency_avg_ms":120,
                    "latency_latest_ms":140,
                    "tokens_per_second":167
                  },
                  "nano_stats":{
                    "calls":1,
                    "tokens_in":null,
                    "tokens_out":null,
                    "latency_avg_ms":32,
                    "latency_latest_ms":32,
                    "tokens_per_second":null
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
                  "runtime":{
                    "nano":{
                      "enabled":true,
                      "availability":"available",
                      "detail":"Gemini Nano is ready.",
                      "provider":"gemini",
                      "model":"gemini-nano",
                      "active":true
                    },
                    "selected_cloud_profile_id":"gemini_flash_lite",
                    "selected_cloud_profile_label":"Gemini Flash Lite",
                    "cloud_profiles":[
                      {
                        "id":"gemini_flash_lite",
                        "label":"Gemini Flash Lite",
                        "provider":"gemini",
                        "model":"gemini-2.5-flash-lite",
                        "enabled":true,
                        "available":true,
                        "selected":true
                      }
                    ]
                  },
                  "tools":{
                    "catalog":[
                      {
                        "tool_id":"spotify",
                        "display_name":"Spotify",
                        "category":"media",
                        "available":true,
                        "integrated":false,
                        "auth_state":"required_not_started",
                        "health_state":"healthy",
                        "next_step":"auth_required",
                        "summary":"Spotify is available but not connected."
                      }
                    ],
                    "available_tools":["spotify"],
                    "integrated_tools":[]
                  },
                  "diagnostics":{
                    "turn_summaries":[],
                    "model_traces":[],
                    "decision_events":[],
                    "issues":[
                      {
                        "timestamp":"2026-01-01T10:00:00Z",
                        "subsystem":"tools",
                        "level":"warning",
                        "summary":"Spotify auth required"
                      }
                    ],
                    "recent_logs":[],
                    "recent_tool_logs":[]
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
                  "recent_tool_logs":[
                    {
                      "created_at":"2026-01-01T10:01:00Z",
                      "tool_name":"spotify",
                      "action":"disconnect",
                      "arguments_json":"{}",
                      "result_json":"{\"status\":\"success\"}",
                      "success":true,
                      "latency_ms":20
                    }
                  ],
                  "turn_summaries":[
                    {
                      "turn_id":"turn-1",
                      "created_at":"2026-01-01T10:00:00Z",
                      "user_input_summary":"hello",
                      "input_source":"text",
                      "plan_kind":"direct_nano",
                      "context_policy":"transcript_only",
                      "model_stages":["first_beat","nano_reply"],
                      "memory_used":false,
                      "tool_consulted":false,
                      "tool_used":false,
                      "nano_first_beat_used":true,
                      "cloud_escalated":false,
                      "cloud_used":false,
                      "selected_cloud_profile":null,
                      "delivery_mode":"NANO_ONLY",
                      "final_route":"nano",
                      "total_latency_ms":32,
                      "final_visible_output":"Hello again.",
                      "had_fallback":false
                    }
                  ],
                  "model_traces":[
                    {
                      "timestamp":"2026-01-01T10:00:00Z",
                      "turn_id":"turn-1",
                      "stage_name":"PRIMARY_STAGE_COMPLETED",
                      "prompt_mode":"nano_reply",
                      "lane":"nano",
                      "status":"success",
                      "latency_ms":12,
                      "displayed_text":"Hello again.",
                      "raw_input":"hello",
                      "raw_output":"Hello again.",
                      "normalized_output":"Hello again."
                    }
                  ],
                  "decision_events":[
                    {
                      "timestamp":"2026-01-01T10:00:00Z",
                      "turn_id":"turn-1",
                      "name":"PLAN_SELECTED",
                      "plan_kind":"direct_nano",
                      "reason_codes":["default_direct"],
                      "detail":"transcript_only"
                    }
                  ],
                  "extra_field":"ignored"
                }
            """.trimIndent(),
        )

        assertEquals("gpt-4o-mini", snapshot.modelStats.modelName)
        assertEquals(2L, snapshot.cloudStats.calls)
        assertEquals(10L, snapshot.cloudStats.tokensIn ?: -1)
        assertEquals(167L, snapshot.cloudStats.tokensPerSecond ?: -1)
        assertEquals(1L, snapshot.nanoStats.calls)
        assertNull(snapshot.nanoStats.tokensIn)
        assertNull(snapshot.nanoStats.tokensPerSecond)
        assertEquals("available", snapshot.runtime.nano.availability)
        assertEquals(1, snapshot.toolStates.size)
        assertTrue(snapshot.toolStates.containsKey("hue"))
        assertEquals(1, snapshot.tools.catalog.size)
        assertEquals("spotify", snapshot.tools.availableTools.first())
        assertEquals(1, snapshot.diagnostics.issues.size)
        assertEquals(1, snapshot.recentLogs.size)
        assertEquals(200L, snapshot.recentLogs.first().status)
        assertEquals(1, snapshot.recentToolLogs.size)
        assertEquals("disconnect", snapshot.recentToolLogs.first().action)
        assertEquals(1, snapshot.turnSummaries.size)
        assertEquals("direct_nano", snapshot.turnSummaries.first().planKind)
        assertEquals("NANO_ONLY", snapshot.turnSummaries.first().deliveryMode)
        assertEquals(1, snapshot.modelTraces.size)
        assertEquals("nano_reply", snapshot.modelTraces.first().promptMode)
        assertEquals(1, snapshot.decisionEvents.size)
        assertEquals("PLAN_SELECTED", snapshot.decisionEvents.first().name)
    }

    @Test
    fun decodeOverviewSnapshotDefaultsSplitStatsWhenMissing() {
        val snapshot = CoreJson.decodeOverviewSnapshot(
            """
                {
                  "previous_context":"medium",
                  "model_stats":{
                    "model_name":"unconfigured",
                    "total_requests":0,
                    "total_input_tokens":0,
                    "total_output_tokens":0,
                    "average_latency_ms":0,
                    "latest_latency_ms":0
                  },
                  "memory_stats":{
                    "message_count":0,
                    "stored_memories":0,
                    "vector_count":0,
                    "retrieval_count":0
                  },
                  "system_stats":{
                    "total_interactions":0,
                    "tool_calls":0,
                    "error_count":0
                  },
                  "runtime":{
                    "nano":{
                      "enabled":false,
                      "availability":"unavailable",
                      "detail":"Nano is unavailable.",
                      "provider":"gemini",
                      "model":"gemini-nano",
                      "active":false
                    }
                  },
                  "tool_states":{},
                  "recent_logs":[],
                  "recent_tool_logs":[]
                }
            """.trimIndent(),
        )

        assertEquals(0L, snapshot.cloudStats.calls)
        assertNull(snapshot.cloudStats.tokensIn)
        assertEquals(0L, snapshot.nanoStats.calls)
        assertNull(snapshot.nanoStats.latencyAvgMs)
    }
}
