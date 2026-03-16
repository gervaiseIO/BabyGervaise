package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.BootstrapState
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import io.gervaise.babygervaise.notes.NotesUiState

sealed interface Screen {
    data object Chat : Screen

    data object Overview : Screen

    data class Notes(
        val route: NotesRoute = NotesRoute.Editor,
    ) : Screen

    sealed interface NotesRoute {
        data object Onboarding : NotesRoute

        data object Editor : NotesRoute

        data object Search : NotesRoute
    }
}

data class ActiveToolStatus(
    val turnId: String,
    val tool: String,
    val action: String,
    val status: String,
)

data class BabyGervaiseUiState(
    val screen: Screen = Screen.Chat,
    val bootstrapState: BootstrapState = BootstrapState.Empty,
    val overviewSnapshot: OverviewSnapshot = OverviewSnapshot.Empty,
    val draft: String = "",
    val interactionState: InteractionUiState = InteractionUiState(),
    val notesState: NotesUiState = NotesUiState(),
    val pendingTurnId: String? = null,
    val activeToolStatus: ActiveToolStatus? = null,
    val toolStatus: String? = null,
    val snackbarMessage: String? = null,
    val isInitializing: Boolean = true,
    val initializationError: String? = null,
    val isCoreReady: Boolean = false,
    val canSubmitTurns: Boolean = false,
) {
    val isPending: Boolean
        get() = pendingTurnId != null

    val statusText: String
        get() = initializationError ?: toolStatus ?: "HGIE ready."
}

fun ContextLevel.asStatusLabel(): String = wireName
