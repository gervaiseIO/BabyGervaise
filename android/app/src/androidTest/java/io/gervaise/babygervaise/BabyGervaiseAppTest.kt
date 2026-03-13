package io.gervaise.babygervaise

import androidx.activity.ComponentActivity
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.gervaise.babygervaise.bridge.BootstrapState
import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.InputSource
import io.gervaise.babygervaise.bridge.LogViewerEntry
import io.gervaise.babygervaise.bridge.ModelStats
import io.gervaise.babygervaise.bridge.MemoryStats
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import io.gervaise.babygervaise.bridge.SystemStats
import io.gervaise.babygervaise.theme.BabyGervaiseTheme
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class BabyGervaiseAppTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun chatScreenShowsEmptyStateAndTogglesToOverview() {
        render(BabyGervaiseUiState())

        composeRule.onNodeWithText("One conversation. No reset.").assertIsDisplayed()
        composeRule.onNodeWithText("Overview").performClick()
        composeRule.onNodeWithTag("overview-screen").assertIsDisplayed()
    }

    @Test
    fun optimisticPendingStateShowsTimelineAndSendingButton() {
        render(
            BabyGervaiseUiState(
                draft = "Hello",
                pendingTurnId = "turn-1",
                bootstrapState = BootstrapState(
                    previousContext = ContextLevel.MEDIUM,
                    messages = listOf(
                        message(role = "user", content = "Hello", turnId = "turn-1", id = 1),
                        message(role = "assistant", content = "", turnId = "turn-1", id = 2),
                    ),
                ),
            ),
        )

        composeRule.onNodeWithTag("timeline").assertIsDisplayed()
        composeRule.onNodeWithText("Hello").assertIsDisplayed()
        composeRule.onNodeWithText("…").assertIsDisplayed()
        composeRule.onNodeWithTag("send-button").assertIsDisplayed()
        composeRule.onNodeWithText("Sending...").assertIsDisplayed()
    }

    @Test
    fun composerEmitsDraftChangesAndSubmit() {
        var submitted = false
        var draft = ""
        composeRule.setContent {
            BabyGervaiseTheme {
                var uiState by remember { mutableStateOf(BabyGervaiseUiState()) }
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {
                        draft = it
                        uiState = uiState.copy(draft = it)
                    },
                    onSubmit = { submitted = true },
                    onToggleScreen = {},
                    onPreviousContextChanged = {},
                )
            }
        }

        composeRule.onNodeWithTag("composer-input").performTextInput("Tell me something")
        composeRule.onNodeWithTag("send-button").performClick()

        composeRule.runOnIdle {
            assertEquals("Tell me something", draft)
            assertTrue(submitted)
        }
    }

    @Test
    fun overviewShowsToolStateLogsAndContextMenu() {
        var selectedLevel: ContextLevel? = null
        composeRule.setContent {
            BabyGervaiseTheme {
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = BabyGervaiseUiState(
                        screen = Screen.OVERVIEW,
                        overviewSnapshot = OverviewSnapshot(
                            previousContext = ContextLevel.MEDIUM,
                            modelStats = ModelStats(
                                modelName = "gpt-4o-mini",
                                totalRequests = 2,
                                totalInputTokens = 10,
                                totalOutputTokens = 20,
                                averageLatencyMs = 120,
                                latestLatencyMs = 140,
                            ),
                            memoryStats = MemoryStats(
                                messageCount = 4,
                                storedMemories = 2,
                                vectorCount = 2,
                                retrievalCount = 1,
                            ),
                            systemStats = SystemStats(
                                totalInteractions = 2,
                                toolCalls = 1,
                                errorCount = 0,
                            ),
                            toolStates = mapOf("hue" to JsonPrimitive("online")),
                            recentLogs = listOf(
                                LogViewerEntry(
                                    timestamp = "2026-01-01T10:00:00Z",
                                    prompt = "{}",
                                    rawOutput = "{\"assistant_reply\":\"hi\"}",
                                    latencyMs = 140,
                                    status = 200,
                                ),
                            ),
                        ),
                    ),
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {},
                    onSubmit = {},
                    onToggleScreen = {},
                    onPreviousContextChanged = { selectedLevel = it },
                )
            }
        }

        composeRule.onNodeWithText("Tool State").assertIsDisplayed()
        composeRule.onNodeWithText("hue").assertIsDisplayed()
        composeRule.onNodeWithText("Raw Model Logs").assertIsDisplayed()
        composeRule.onNodeWithText("Status 200").assertIsDisplayed()
        composeRule.onNodeWithTag("previous-context-button").performClick()
        composeRule.onNodeWithText("High").performClick()
        composeRule.runOnIdle {
            assertEquals(ContextLevel.HIGH, selectedLevel)
        }
    }

    @Test
    fun toolAndErrorStatesAreVisible() {
        render(
            BabyGervaiseUiState(
                toolStatus = "Bridge failure",
                bootstrapState = BootstrapState(
                    previousContext = ContextLevel.HIGH,
                    messages = listOf(
                        message(role = "tool", content = "{\"status\":\"ok\"}", turnId = "turn-2", id = 3),
                    ),
                ),
            ),
        )

        composeRule.onNodeWithText("Bridge failure").assertIsDisplayed()
        composeRule.onNodeWithText("tool").assertIsDisplayed()
        composeRule.onNodeWithText("{\"status\":\"ok\"}").assertIsDisplayed()
    }

    private fun render(initialState: BabyGervaiseUiState) {
        composeRule.setContent {
            BabyGervaiseTheme {
                var uiState by remember { mutableStateOf(initialState) }
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = { uiState = uiState.copy(draft = it) },
                    onSubmit = {},
                    onToggleScreen = {
                        uiState = uiState.copy(
                            screen = if (uiState.screen == Screen.CHAT) {
                                Screen.OVERVIEW
                            } else {
                                Screen.CHAT
                            },
                        )
                    },
                    onPreviousContextChanged = {
                        uiState = uiState.copy(
                            bootstrapState = uiState.bootstrapState.copy(previousContext = it),
                            overviewSnapshot = uiState.overviewSnapshot.copy(previousContext = it),
                        )
                    },
                )
            }
        }
    }

    private fun message(
        role: String,
        content: String,
        turnId: String,
        id: Long,
    ) = ChatMessage(
        id = id,
        role = role,
        content = content,
        turnId = turnId,
        inputSource = InputSource.TEXT,
        createdAt = "2026-01-01T10:00:00Z",
    )
}
