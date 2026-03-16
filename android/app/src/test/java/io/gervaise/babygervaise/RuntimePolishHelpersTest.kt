package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.DebugLogEntry
import io.gervaise.babygervaise.bridge.InputSource
import io.gervaise.babygervaise.bridge.MessageContentType
import io.gervaise.babygervaise.bridge.NanoRuntimeStatus
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import io.gervaise.babygervaise.bridge.RuntimeOverview
import io.gervaise.babygervaise.bridge.RuntimeProfileSummary
import java.time.Instant
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RuntimePolishHelpersTest {
    @Test
    fun bootstrapWaitsForReadyState() {
        val initial = BabyGervaiseCoreState(
            isInitializing = true,
            isCoreReady = false,
            initializationError = null,
        )
        val ready = initial.copy(isInitializing = false, isCoreReady = true)

        assertFalse(shouldFinalizeInteractionBootstrap(initial))
        assertTrue(shouldFinalizeInteractionBootstrap(ready))
    }

    @Test
    fun projectedBootstrapItemsKeepFullTranscriptOrder() {
        val items = projectBootstrapTranscriptItems(
            messages = listOf(
                chatMessage(id = 1L, role = "user", turnId = "turn-1", content = "Hey"),
                chatMessage(id = 2L, role = "assistant", turnId = "turn-1", content = "Hi there"),
                chatMessage(
                    id = 3L,
                    role = "tool",
                    turnId = "turn-1",
                    content = """{"tool":"spotify","status":"success","message":"Playback resumed"}""",
                ),
                chatMessage(id = 4L, role = "system", turnId = "turn-1", content = "Nano ready"),
            ),
            pendingTurnId = null,
        )

        assertEquals(listOf(UserBubble::class, AssistantBubble::class, ActionResultCard::class), items.map { it::class })
        assertEquals("Hey", (items[0] as UserBubble).text)
        assertEquals("Hi there", (items[1] as AssistantBubble).text)
        assertEquals("Playback resumed", (items[2] as ActionResultCard).detail)
    }

    @Test
    fun conversationLaneIncludesCloudOnlyAvailability() {
        val overview = OverviewSnapshot.Empty.copy(
            runtime = RuntimeOverview(
                nano = NanoRuntimeStatus(
                    enabled = true,
                    availability = "unavailable",
                    detail = "Nano unavailable.",
                    provider = "gemini",
                    model = "gemini-nano",
                    active = false,
                ),
                cloudProfiles = listOf(
                    RuntimeProfileSummary(
                        id = "gemini_flash_lite",
                        label = "Gemini Flash Lite",
                        provider = "gemini",
                        model = "gemini-2.5-flash-lite",
                        enabled = true,
                        available = true,
                        selected = true,
                    ),
                ),
            ),
        )

        assertTrue(hasConversationLane(overview))
    }

    @Test
    fun persistedConversationHistoryRequiresRealUserOrAssistantMessages() {
        assertFalse(
            hasPersistedConversationHistory(
                listOf(chatMessage(id = 1L, role = "assistant", turnId = "turn-1", content = "")),
            ),
        )
        assertTrue(
            hasPersistedConversationHistory(
                listOf(chatMessage(id = 2L, role = "user", turnId = "turn-2", content = "Hello")),
            ),
        )
    }

    @Test
    fun welcomeBackGatingStaysConservative() {
        val now = Instant.parse("2026-03-15T10:30:00Z")
        val history = listOf(
            chatMessage(
                id = 2L,
                role = "assistant",
                turnId = "turn-2",
                content = "Welcome back.",
                createdAt = "2026-03-15T10:00:00Z",
            ),
        )

        assertFalse(
            shouldTriggerWelcomeBack(
                isCoreReady = true,
                pendingTurnId = "turn-pending",
                nanoActive = true,
                messages = history,
                now = now,
                resolvedIdleSeconds = 1800,
                lastWelcomeBackRequestAt = null,
                requiredIdleSeconds = 900,
                debounceSeconds = 60,
            ),
        )
        assertFalse(
            shouldTriggerWelcomeBack(
                isCoreReady = true,
                pendingTurnId = null,
                nanoActive = true,
                messages = history,
                now = now,
                resolvedIdleSeconds = 1800,
                lastWelcomeBackRequestAt = now.minusSeconds(20),
                requiredIdleSeconds = 900,
                debounceSeconds = 60,
            ),
        )
        assertFalse(
            shouldTriggerWelcomeBack(
                isCoreReady = true,
                pendingTurnId = null,
                nanoActive = true,
                messages = emptyList(),
                now = now,
                resolvedIdleSeconds = null,
                lastWelcomeBackRequestAt = null,
                requiredIdleSeconds = 900,
                debounceSeconds = 60,
            ),
        )
        assertTrue(
            shouldTriggerWelcomeBack(
                isCoreReady = true,
                pendingTurnId = null,
                nanoActive = true,
                messages = history,
                now = now,
                resolvedIdleSeconds = 1800,
                lastWelcomeBackRequestAt = null,
                requiredIdleSeconds = 900,
                debounceSeconds = 60,
            ),
        )
    }

    @Test
    fun resolvedWelcomeBackIdleSecondsUsesLatestConversationActivity() {
        val history = listOf(
            chatMessage(
                id = 1L,
                role = "user",
                turnId = "turn-1",
                content = "Hi",
                createdAt = "2026-03-15T10:00:00Z",
            ),
        )

        assertEquals(
            1800L,
            resolvedWelcomeBackIdleSeconds(
                messages = history,
                now = Instant.parse("2026-03-15T10:30:00Z"),
                explicitIdleSeconds = null,
            ),
        )
        assertNull(
            resolvedWelcomeBackIdleSeconds(
                messages = emptyList(),
                now = Instant.parse("2026-03-15T10:30:00Z"),
                explicitIdleSeconds = null,
            ),
        )
    }

    @Test
    fun cloudWorkingDiagnosticIsParsedInOneHelper() {
        val entry = DebugLogEntry(
            subsystem = "hgie",
            level = "info",
            message = "turn route selected",
            turnId = "turn-1",
            fields = buildJsonObject {
                put("cloud_escalated", true)
            },
        )

        assertTrue(isCloudWorkingDiagnostic(entry))
    }

    private fun chatMessage(
        id: Long,
        role: String,
        turnId: String,
        content: String,
        createdAt: String = "2026-03-15T10:00:00Z",
    ): ChatMessage = ChatMessage(
        id = id,
        role = role,
        content = content,
        turnId = turnId,
        inputSource = InputSource.TEXT,
        createdAt = createdAt,
        contentType = MessageContentType.PLAIN_TEXT,
        displayJson = null,
        visibleSummary = null,
    )
}
