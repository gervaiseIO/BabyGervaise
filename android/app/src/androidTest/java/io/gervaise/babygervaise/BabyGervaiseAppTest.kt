package io.gervaise.babygervaise

import androidx.activity.ComponentActivity
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.DiagnosticsOverview
import io.gervaise.babygervaise.bridge.ModelStats
import io.gervaise.babygervaise.bridge.MemoryStats
import io.gervaise.babygervaise.bridge.NanoRuntimeStatus
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import io.gervaise.babygervaise.bridge.RuntimeOverview
import io.gervaise.babygervaise.bridge.RuntimeProfileSummary
import io.gervaise.babygervaise.bridge.SystemStats
import io.gervaise.babygervaise.bridge.ToolActionAvailability
import io.gervaise.babygervaise.bridge.ToolOverviewEntry
import io.gervaise.babygervaise.bridge.ToolsOverview
import io.gervaise.babygervaise.bridge.TurnTraceSummary
import io.gervaise.babygervaise.bridge.UsageStats
import io.gervaise.babygervaise.notes.NoteEditorState
import io.gervaise.babygervaise.notes.NotesUiState
import io.gervaise.babygervaise.theme.BabyGervaiseTheme
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

        composeRule.onNodeWithText("One conversation. One timeline.").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Overview").performClick()
        composeRule.onNodeWithTag("overview-screen").assertIsDisplayed()
    }

    @Test
    fun pendingStateShowsTypingIndicatorAndButtonProgress() {
        render(
            BabyGervaiseUiState(
                interactionState = InteractionUiState(
                    items = listOf(
                        UserBubble(
                            id = "user-turn-1",
                            timestampMs = 1L,
                            turnId = "turn-1",
                            text = "Hello",
                        ),
                        AssistantBubble(
                            id = "assistant-turn-1",
                            timestampMs = 2L,
                            turnId = "turn-1",
                            text = "",
                            isStreaming = true,
                        ),
                    ),
                    draft = "Hello",
                    isSending = true,
                    canSubmit = true,
                ),
                pendingTurnId = "turn-1",
            ),
        )

        composeRule.onNodeWithTag("timeline").assertIsDisplayed()
        composeRule.onNodeWithText("Hello").assertIsDisplayed()
        composeRule.onNodeWithTag("assistant-processing").assertIsDisplayed()
        composeRule.onNodeWithTag("send-button").assertIsDisplayed()
        composeRule.onNodeWithTag("send-progress").assertIsDisplayed()
    }

    @Test
    fun composerEmitsDraftChangesAndSubmit() {
        var submitted = false
        var draft = ""
        composeRule.setContent {
            BabyGervaiseTheme {
                var uiState by remember {
                    mutableStateOf(
                        BabyGervaiseUiState(
                            interactionState = InteractionUiState(
                                canSubmit = true,
                            ),
                            isCoreReady = true,
                            canSubmitTurns = true,
                        ),
                    )
                }
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {
                        draft = it
                        uiState = uiState.copy(
                            draft = it,
                            interactionState = uiState.interactionState.copy(draft = it),
                        )
                    },
                    onSubmit = { submitted = true },
                    onStopCloudWorking = {},
                    onSuggestionSelected = { _, _ -> },
                    onOpenOverview = {},
                    onOpenChat = {},
                    onOpenNotes = {},
                    onOpenNotesSearch = {},
                    onCloseNotesSearch = {},
                    onNoteBodyChanged = {},
                    onNotesVaultSelected = {},
                    onOpenSearchedNote = {},
                    onNoteSearchQueryChanged = {},
                    onPreviousContextChanged = {},
                    onCloudProfileChanged = {},
                    onBeginToolAuth = {},
                    onRefreshToolState = {},
                    onDisconnectTool = {},
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
    fun chatRendersTimelineFamiliesAndSuggestionTap() {
        var selected: Pair<String, String>? = null
        render(
            BabyGervaiseUiState(
                interactionState = InteractionUiState(
                    items = InteractionTimelineDemoData.timeline,
                    canSubmit = true,
                ),
            ),
            onSuggestionSelected = { cardId, optionId ->
                selected = cardId to optionId
            },
        )

        composeRule.onNodeWithText("Play the Beatles on Denon").assertIsDisplayed()
        composeRule.onNodeWithText("Spotify connected").assertIsDisplayed()
        composeRule.onNodeWithText("Now playing").assertIsDisplayed()
        composeRule.onNodeWithText("Spotify couldn't start playback").assertIsDisplayed()
        composeRule.onNodeWithText("Denon").performClick()

        composeRule.runOnIdle {
            assertEquals("suggestion-demo-1" to "denon", selected)
        }
    }

    @Test
    fun composerTracksReadinessAndOverviewStillRenders() {
        lateinit var updateState: (BabyGervaiseUiState) -> Unit
        composeRule.setContent {
            BabyGervaiseTheme {
                val snackbarHostState = remember { SnackbarHostState() }
                var uiState by remember {
                    mutableStateOf(
                        BabyGervaiseUiState(
                            interactionState = InteractionUiState(
                                items = listOf(
                                    LiveStateCard(
                                        id = "live-nano-runtime",
                                        timestampMs = 1L,
                                        title = "Nano downloading",
                                        detail = "Preparing Gemini Nano.",
                                        tone = InteractionTone.Progress,
                                    ),
                                ),
                                canSubmit = false,
                            ),
                            isCoreReady = true,
                            canSubmitTurns = false,
                        ),
                    )
                }
                updateState = { uiState = it }
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {
                        uiState = uiState.copy(
                            draft = it,
                            interactionState = uiState.interactionState.copy(draft = it),
                        )
                    },
                    onSubmit = {},
                    onStopCloudWorking = {},
                    onSuggestionSelected = { _, _ -> },
                    onOpenOverview = {},
                    onOpenChat = {},
                    onOpenNotes = {},
                    onOpenNotesSearch = {},
                    onCloseNotesSearch = {},
                    onNoteBodyChanged = {},
                    onNotesVaultSelected = {},
                    onOpenSearchedNote = {},
                    onNoteSearchQueryChanged = {},
                    onPreviousContextChanged = {},
                    onCloudProfileChanged = {},
                    onBeginToolAuth = {},
                    onRefreshToolState = {},
                    onDisconnectTool = {},
                )
            }
        }

        composeRule.onNodeWithText("Nano downloading").assertIsDisplayed()
        composeRule.onNodeWithTag("composer-input").assertIsNotEnabled()

        composeRule.runOnIdle {
            updateState(
                BabyGervaiseUiState(
                    interactionState = InteractionUiState(
                        items = listOf(
                            LiveStateCard(
                                id = "live-nano-runtime",
                                timestampMs = 2L,
                                title = "Nano model ready",
                                detail = "Gemini Nano",
                                tone = InteractionTone.Positive,
                            ),
                        ),
                        canSubmit = true,
                    ),
                    isCoreReady = true,
                    canSubmitTurns = true,
                ),
            )
        }

        composeRule.onNodeWithText("Nano model ready").assertIsDisplayed()
        composeRule.onNodeWithTag("composer-input").assertIsEnabled()

        composeRule.runOnIdle {
            updateState(
                BabyGervaiseUiState(
                    screen = Screen.Overview,
                    interactionState = InteractionUiState(canSubmit = true),
                    isCoreReady = true,
                    canSubmitTurns = true,
                    overviewSnapshot = OverviewSnapshot.Empty.copy(
                        runtime = RuntimeOverview(
                            nano = NanoRuntimeStatus(
                                enabled = true,
                                availability = "available",
                                detail = "Gemini Nano is ready.",
                                provider = "gemini",
                                model = "gemini-nano",
                                active = true,
                            ),
                        ),
                    ),
                ),
            )
        }

        composeRule.onNodeWithTag("overview-screen").assertIsDisplayed()
        composeRule.onNodeWithText("Overview").assertIsDisplayed()
        composeRule.onNodeWithText("Nano").assertIsDisplayed()
    }

    @Test
    fun overviewShowsStructuredControlPlaneSections() {
        composeRule.setContent {
            BabyGervaiseTheme {
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = BabyGervaiseUiState(
                        screen = Screen.Overview,
                        interactionState = InteractionUiState(canSubmit = true),
                        isCoreReady = true,
                        overviewSnapshot = OverviewSnapshot.Empty.copy(
                            modelStats = ModelStats(
                                modelName = "gpt-4o-mini",
                                totalRequests = 2,
                                totalInputTokens = 10,
                                totalOutputTokens = 20,
                                averageLatencyMs = 120,
                                latestLatencyMs = 140,
                            ),
                            cloudStats = UsageStats(
                                calls = 2,
                                tokensIn = 10,
                                tokensOut = 20,
                                latencyAvgMs = 120,
                                latencyLatestMs = 140,
                                tokensPerSecond = 167,
                            ),
                            nanoStats = UsageStats(
                                calls = 1,
                                latencyAvgMs = 32,
                                latencyLatestMs = 32,
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
                            runtime = RuntimeOverview(
                                nano = NanoRuntimeStatus(
                                    enabled = true,
                                    availability = "available",
                                    detail = "Gemini Nano is ready.",
                                    provider = "gemini",
                                    model = "gemini-nano",
                                    active = true,
                                ),
                                selectedCloudProfileId = "gemini_flash_lite",
                                selectedCloudProfileLabel = "Gemini Flash Lite",
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
                            tools = ToolsOverview(
                                catalog = listOf(
                                    ToolOverviewEntry(
                                        toolId = "spotify",
                                        displayName = "Spotify",
                                        category = "media",
                                        available = true,
                                        integrated = false,
                                        authState = "required_not_started",
                                        healthState = "healthy",
                                        nextStep = "auth_required",
                                        summary = "Spotify is available but not connected.",
                                        actions = listOf(
                                            ToolActionAvailability(
                                                actionId = "begin_auth",
                                                label = "Connect",
                                                enabled = true,
                                            ),
                                            ToolActionAvailability(
                                                actionId = "refresh_state",
                                                label = "Refresh",
                                                enabled = true,
                                            ),
                                            ToolActionAvailability(
                                                actionId = "disconnect",
                                                label = "Disconnect",
                                                enabled = true,
                                            ),
                                        ),
                                    ),
                                ),
                                availableTools = listOf("spotify"),
                                integratedTools = emptyList(),
                            ),
                            diagnostics = DiagnosticsOverview(
                                turnSummaries = listOf(
                                    TurnTraceSummary(
                                        turnId = "turn-1",
                                        createdAt = "2026-01-01T10:00:00Z",
                                        userInputSummary = "Play some jazz",
                                        inputSource = "text",
                                        planKind = "cloud_tool",
                                        contextPolicy = "transcript_only",
                                        modelStages = listOf("first_beat", "cloud_reasoning"),
                                        memoryUsed = true,
                                        toolConsulted = true,
                                        toolUsed = false,
                                        nanoFirstBeatUsed = true,
                                        cloudEscalated = true,
                                        cloudUsed = true,
                                        selectedCloudProfile = "gemini_flash_lite",
                                        deliveryMode = "NANO_THEN_CLOUD",
                                        finalRoute = "nano + cloud",
                                        totalLatencyMs = 88,
                                        finalVisibleOutput = "Working on it.",
                                        hadFallback = false,
                                    ),
                                ),
                            ),
                        ),
                    ),
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {},
                    onSubmit = {},
                    onStopCloudWorking = {},
                    onSuggestionSelected = { _, _ -> },
                    onOpenOverview = {},
                    onOpenChat = {},
                    onOpenNotes = {},
                    onOpenNotesSearch = {},
                    onCloseNotesSearch = {},
                    onNoteBodyChanged = {},
                    onNotesVaultSelected = {},
                    onOpenSearchedNote = {},
                    onNoteSearchQueryChanged = {},
                    onPreviousContextChanged = {},
                    onCloudProfileChanged = {},
                    onBeginToolAuth = {},
                    onRefreshToolState = {},
                    onDisconnectTool = {},
                )
            }
        }

        composeRule.onNodeWithText("Overview").assertIsDisplayed()
        composeRule.onNodeWithTag("overview-screen").assertIsDisplayed()
        composeRule.onNodeWithText("Models").assertIsDisplayed()
        composeRule.onNodeWithText("Memory").assertIsDisplayed()
        composeRule.onNodeWithText("Tools").assertIsDisplayed()
        composeRule.onNodeWithText("Runtime / Diagnostics").assertIsDisplayed()
        composeRule.onNodeWithText("System").assertIsDisplayed()
        composeRule.onNodeWithText("Gemini Flash Lite").assertIsDisplayed()
        composeRule.onNodeWithText("Spotify").assertIsDisplayed()
        composeRule.onNodeWithText("Connect").assertIsDisplayed()
        composeRule.onNodeWithText("Play some jazz").assertIsDisplayed()
        composeRule.onNodeWithText("Requests").assertIsDisplayed()
        composeRule.onNodeWithText("Tokens/sec").assertIsDisplayed()
        composeRule.onNodeWithText("167").assertIsDisplayed()
        composeRule.onNodeWithTag("previous-context-button").assertIsDisplayed()
        composeRule.onNodeWithTag("cloud-profile-button").assertIsDisplayed()
        composeRule.onAllNodesWithText("Floating overlay").assertCountEquals(0)
    }

    @Test
    fun cloudPendingShowsWorkingOrnamentAndVisualStop() {
        var uiState by mutableStateOf(
            BabyGervaiseUiState(
                interactionState = InteractionUiState(
                    items = listOf(
                        UserBubble(
                            id = "user-turn-1",
                            timestampMs = 1L,
                            turnId = "turn-1",
                            text = "Can you think this through?",
                        ),
                        AssistantBubble(
                            id = "assistant-turn-1",
                            timestampMs = 2L,
                            turnId = "turn-1",
                            text = "Let me think.",
                        ),
                    ),
                    draft = "Can you think this through?",
                    isSending = true,
                    canSubmit = true,
                    deliveryState = DeliveryState.CLOUD_WORKING,
                ),
                pendingTurnId = "turn-1",
            ),
        )

        composeRule.setContent {
            BabyGervaiseTheme {
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = remember { SnackbarHostState() },
                    onDraftChanged = {},
                    onSubmit = {},
                    onStopCloudWorking = {
                        uiState = uiState.copy(
                            interactionState = uiState.interactionState.copy(
                                isCloudWorkingMuted = true,
                            ),
                        )
                    },
                    onSuggestionSelected = { _, _ -> },
                    onOpenOverview = {},
                    onOpenChat = {},
                    onOpenNotes = {},
                    onOpenNotesSearch = {},
                    onCloseNotesSearch = {},
                    onNoteBodyChanged = {},
                    onNotesVaultSelected = {},
                    onOpenSearchedNote = {},
                    onNoteSearchQueryChanged = {},
                    onPreviousContextChanged = {},
                    onCloudProfileChanged = {},
                    onBeginToolAuth = {},
                    onRefreshToolState = {},
                    onDisconnectTool = {},
                )
            }
        }

        composeRule.onNodeWithTag("cloud-working-indicator").assertIsDisplayed()
        composeRule.onNodeWithTag("send-stop-icon").assertIsDisplayed()
        composeRule.onNodeWithTag("send-button").performClick()
        composeRule.onAllNodesWithTag("cloud-working-indicator").assertCountEquals(0)
        composeRule.onNodeWithTag("send-stop-icon").assertIsDisplayed()
    }

    private fun render(
        initialState: BabyGervaiseUiState,
        onSuggestionSelected: (String, String) -> Unit = { _, _ -> },
    ) {
        composeRule.setContent {
            BabyGervaiseTheme {
                var uiState by remember { mutableStateOf(initialState) }
                val snackbarHostState = remember { SnackbarHostState() }
                BabyGervaiseApp(
                    uiState = uiState,
                    snackbarHostState = snackbarHostState,
                    onDraftChanged = {
                        uiState = uiState.copy(
                            draft = it,
                            interactionState = uiState.interactionState.copy(draft = it),
                        )
                    },
                    onSubmit = {},
                    onStopCloudWorking = {},
                    onSuggestionSelected = onSuggestionSelected,
                    onOpenOverview = {
                        uiState = uiState.copy(screen = Screen.Overview)
                    },
                    onOpenChat = {
                        uiState = uiState.copy(screen = Screen.Chat)
                    },
                    onOpenNotes = {
                        uiState = uiState.copy(screen = Screen.Notes(Screen.NotesRoute.Editor))
                    },
                    onOpenNotesSearch = {
                        uiState = uiState.copy(screen = Screen.Notes(Screen.NotesRoute.Search))
                    },
                    onCloseNotesSearch = {
                        uiState = uiState.copy(screen = Screen.Notes(Screen.NotesRoute.Editor))
                    },
                    onNoteBodyChanged = {
                        uiState = uiState.copy(
                            notesState = uiState.notesState.copy(
                                editor = uiState.notesState.editor.copy(body = it),
                            ),
                        )
                    },
                    onNotesVaultSelected = {},
                    onOpenSearchedNote = {},
                    onNoteSearchQueryChanged = {
                        uiState = uiState.copy(
                            notesState = uiState.notesState.copy(searchQuery = it),
                        )
                    },
                    onPreviousContextChanged = {},
                    onCloudProfileChanged = {},
                    onBeginToolAuth = {},
                    onRefreshToolState = {},
                    onDisconnectTool = {},
                )
            }
        }
    }

    @Test
    fun notesOnboardingShowsFolderPickerEntry() {
        render(
            BabyGervaiseUiState(
                screen = Screen.Notes(Screen.NotesRoute.Onboarding),
            ),
        )

        composeRule.onNodeWithText("Bring your vault. Keep your markdown.").assertIsDisplayed()
        composeRule.onNodeWithTag("notes-pick-folder").assertIsDisplayed()
    }

    @Test
    fun notesEditorShowsBodyFieldAndSearchFab() {
        render(
            BabyGervaiseUiState(
                screen = Screen.Notes(Screen.NotesRoute.Editor),
                notesState = NotesUiState(
                    editor = NoteEditorState(
                        title = "HGIE Prompt Rephase",
                        body = "Today I refined the architecture.",
                    ),
                ),
            ),
        )

        composeRule.onNodeWithTag("notes-editor-title").assertIsDisplayed()
        composeRule.onNodeWithTag("notes-editor-body").assertIsDisplayed()
        composeRule.onNodeWithTag("notes-search-fab").assertIsDisplayed()
    }
}
