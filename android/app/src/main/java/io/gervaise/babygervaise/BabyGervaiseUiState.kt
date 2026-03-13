package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.BootstrapState
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.OverviewSnapshot

enum class Screen {
    CHAT,
    OVERVIEW,
}

data class BabyGervaiseUiState(
    val screen: Screen = Screen.CHAT,
    val bootstrapState: BootstrapState = BootstrapState.Empty,
    val overviewSnapshot: OverviewSnapshot = OverviewSnapshot.Empty,
    val draft: String = "",
    val pendingTurnId: String? = null,
    val toolStatus: String? = null,
    val snackbarMessage: String? = null,
    val isInitializing: Boolean = true,
    val initializationError: String? = null,
    val isCoreReady: Boolean = false,
) {
    val isPending: Boolean
        get() = pendingTurnId != null

    val statusText: String
        get() = initializationError ?: toolStatus ?: "HGIE ready."
}

fun ContextLevel.asStatusLabel(): String = wireName
