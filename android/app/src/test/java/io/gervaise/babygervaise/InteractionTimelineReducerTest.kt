package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.InputSource
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InteractionTimelineReducerTest {
    @Test
    fun bootstrapSeedsTimeline() {
        val bootstrapItems = listOf(
            UserBubble(
                id = "user-turn-1",
                timestampMs = 1L,
                turnId = "turn-1",
                text = "Hello",
            ),
            LiveStateCard(
                id = "live-nano-runtime",
                timestampMs = 2L,
                title = "Nano model ready",
                detail = "Gemini Nano",
            ),
        )

        val reduced = InteractionTimelineReducer.reduce(
            current = InteractionUiState(),
            event = HgieEvent.Bootstrap(items = bootstrapItems),
        )

        assertEquals(bootstrapItems, reduced.items)
    }

    @Test
    fun assistantReplyUpsertsSameBubbleId() {
        val initial = InteractionTimelineReducer.reduce(
            current = InteractionUiState(),
            event = HgieEvent.AssistantReply(
                item = AssistantBubble(
                    id = "assistant-turn-1",
                    timestampMs = 1L,
                    turnId = "turn-1",
                    text = "Checking Spotify.",
                    isStreaming = true,
                ),
            ),
        )

        val reduced = InteractionTimelineReducer.reduce(
            current = initial,
            event = HgieEvent.AssistantReply(
                item = AssistantBubble(
                    id = "assistant-turn-1",
                    timestampMs = 2L,
                    turnId = "turn-1",
                    text = "Checking Spotify and your devices.",
                    isStreaming = false,
                ),
            ),
        )

        val assistant = reduced.items.single() as AssistantBubble
        assertEquals(1, reduced.items.size)
        assertEquals("Checking Spotify and your devices.", assistant.text)
        assertFalse(assistant.isStreaming)
    }

    @Test
    fun actionProgressUpsertsWithoutDuplicatingRows() {
        val afterStart = InteractionTimelineReducer.reduce(
            current = InteractionUiState(),
            event = HgieEvent.ActionStarted(
                item = ActionCard(
                    id = "action-turn-1-spotify-play",
                    timestampMs = 1L,
                    turnId = "turn-1",
                    tool = "Spotify",
                    title = "Play",
                    status = "Running",
                    detail = "Starting playback.",
                ),
            ),
        )

        val afterRepeatedStart = InteractionTimelineReducer.reduce(
            current = afterStart,
            event = HgieEvent.ActionStarted(
                item = ActionCard(
                    id = "action-turn-1-spotify-play",
                    timestampMs = 2L,
                    turnId = "turn-1",
                    tool = "Spotify",
                    title = "Play",
                    status = "Running",
                    detail = "Starting playback.",
                ),
            ),
        )

        val afterProgress = InteractionTimelineReducer.reduce(
            current = afterRepeatedStart,
            event = HgieEvent.ActionProgress(
                item = ProgressCard(
                    id = "progress-turn-1",
                    timestampMs = 3L,
                    turnId = "turn-1",
                    tool = "Spotify",
                    title = "Device discovery",
                    detail = "Checking available devices...",
                ),
            ),
        )

        val afterRepeatedProgress = InteractionTimelineReducer.reduce(
            current = afterProgress,
            event = HgieEvent.ActionProgress(
                item = ProgressCard(
                    id = "progress-turn-1",
                    timestampMs = 4L,
                    turnId = "turn-1",
                    tool = "Spotify",
                    title = "Device discovery",
                    detail = "Checking available devices...",
                ),
            ),
        )

        assertEquals(2, afterRepeatedProgress.items.size)
    }

    @Test
    fun parseInteractionToolMessageMapsDisplayJsonToResultAndError() {
        val resultPayload = parseInteractionToolMessage(
            message = chatMessage(
                role = "tool",
                content = """{"tool":"spotify","status":"success","message":"Spotify is ready."}""",
                turnId = "turn-1",
                id = 1L,
                displayJson = """
                    {
                      "tool": "spotify",
                      "status": "connected",
                      "title": "Spotify connected",
                      "body": "Device: Denon",
                      "tone": "positive",
                      "supporting_lines": ["Account: Paul"]
                    }
                """.trimIndent(),
            ),
        )
        val errorPayload = parseInteractionToolMessage(
            message = chatMessage(
                role = "tool",
                content = """{"tool":"spotify","status":"error","message":"Premium may be required"}""",
                turnId = "turn-2",
                id = 2L,
            ),
        )

        assertEquals("Spotify connected", resultPayload?.title)
        assertEquals(InteractionTone.Positive, resultPayload?.tone)
        assertFalse(resultPayload?.isFailure ?: true)
        assertEquals("Spotify couldn't complete that", errorPayload?.title)
        assertTrue(errorPayload?.isFailure == true)
    }

    @Test
    fun liveStateUpdatesUpsertById() {
        val initial = InteractionTimelineReducer.reduce(
            current = InteractionUiState(),
            event = HgieEvent.LiveStateUpdate(
                item = LiveStateCard(
                    id = "live-spotify-playback",
                    timestampMs = 1L,
                    title = "Now playing",
                    detail = "The Beatles - Come Together",
                ),
            ),
        )

        val reduced = InteractionTimelineReducer.reduce(
            current = initial,
            event = HgieEvent.LiveStateUpdate(
                item = LiveStateCard(
                    id = "live-spotify-playback",
                    timestampMs = 2L,
                    title = "Now playing",
                    detail = "The Beatles - Here Comes The Sun",
                ),
            ),
        )

        assertEquals(1, reduced.items.size)
        assertEquals(
            "The Beatles - Here Comes The Sun",
            (reduced.items.single() as LiveStateCard).detail,
        )
    }

    @Test
    fun sanitizeAssistantTextHidesInternalPayloads() {
        assertEquals(
            "",
            sanitizeAssistantText("""{"assistant_reply":"hi","tool_request":null,"memory_candidates":[]}"""),
        )
        assertEquals(
            "Checking Spotify.",
            sanitizeAssistantText("Checking Spotify."),
        )
    }

    @Test
    fun consumeSuggestionMarksCardConsumed() {
        val reduced = InteractionTimelineReducer.consumeSuggestion(
            current = InteractionUiState(
                items = listOf(
                    SuggestionCard(
                        id = "suggestion-1",
                        timestampMs = 1L,
                        title = "Which device should I use?",
                        options = listOf(
                            SuggestionOption(id = "phone", label = "Phone"),
                            SuggestionOption(id = "denon", label = "Denon"),
                        ),
                    ),
                ),
            ),
            cardId = "suggestion-1",
            optionId = "denon",
        )

        val suggestion = reduced.items.single() as SuggestionCard
        assertTrue(suggestion.isConsumed)
        assertEquals("denon", suggestion.selectedOptionId)
    }

    private fun chatMessage(
        role: String,
        content: String,
        turnId: String,
        id: Long,
        displayJson: String? = null,
    ) = ChatMessage(
        id = id,
        role = role,
        content = content,
        turnId = turnId,
        inputSource = InputSource.TEXT,
        createdAt = "2026-01-01T10:00:00Z",
        displayJson = displayJson,
    )
}
