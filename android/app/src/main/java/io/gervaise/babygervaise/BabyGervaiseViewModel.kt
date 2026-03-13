package io.gervaise.babygervaise

import android.app.Application
import android.content.Intent
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.gervaise.babygervaise.bridge.BootstrapState
import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.CoreEvent
import io.gervaise.babygervaise.bridge.InputSource
import io.gervaise.babygervaise.bridge.NativeCoreBridge
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.Instant
import java.util.UUID

class BabyGervaiseViewModel(
    application: Application,
) : AndroidViewModel(application) {
    private val bridge = NativeCoreBridge()
    private val _uiState = MutableStateFlow(BabyGervaiseUiState())
    private var pendingSpotifyCallbackUrl: String? = null
    private var lastHandledSpotifyCallbackUrl: String? = null
    val uiState: StateFlow<BabyGervaiseUiState> = _uiState.asStateFlow()

    init {
        observeCoreEvents()
        initializeCore()
    }

    fun updateDraft(value: String) {
        _uiState.update { current ->
            current.copy(draft = value)
        }
    }

    fun toggleScreen() {
        val nextScreen = if (_uiState.value.screen == Screen.CHAT) {
            Screen.OVERVIEW
        } else {
            Screen.CHAT
        }
        _uiState.update { current ->
            current.copy(screen = nextScreen)
        }
        if (nextScreen == Screen.OVERVIEW) {
            refreshOverviewSnapshot()
        }
    }

    fun submitDraft() {
        val snapshot = _uiState.value
        if (!snapshot.isCoreReady) {
            showSnackbar(snapshot.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        val text = snapshot.draft.trim()
        if (text.isEmpty() || snapshot.isPending) {
            return
        }

        val turnId = UUID.randomUUID().toString()
        _uiState.update { current ->
            current.copy(
                draft = "",
                pendingTurnId = turnId,
                toolStatus = "Sending to HGIE...",
                bootstrapState = current.bootstrapState.copy(
                    messages = current.bootstrapState.messages +
                        createLocalMessage("user", text, turnId) +
                        createLocalMessage("assistant", "", turnId),
                ),
            )
        }

        viewModelScope.launch {
            runCatching {
                bridge.submitUserTurn(
                    turnId = turnId,
                    text = text,
                    inputSource = InputSource.TEXT,
                )
            }.onFailure { error ->
                handleCoreEvent(
                    CoreEvent.AssistantError(
                        turnId = turnId,
                        error = error.message ?: "Unknown bridge failure",
                    ),
                )
            }
        }
    }

    fun updatePreviousContext(level: ContextLevel) {
        if (!_uiState.value.isCoreReady) {
            showSnackbar(_uiState.value.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        _uiState.update { current ->
            current.copy(
                bootstrapState = current.bootstrapState.copy(previousContext = level),
                overviewSnapshot = current.overviewSnapshot.copy(previousContext = level),
            )
        }
        viewModelScope.launch {
            runCatching {
                bridge.setPreviousContext(level)
                val overview = bridge.loadOverviewSnapshot()
                _uiState.update { current ->
                    current.copy(overviewSnapshot = overview)
                }
            }.onFailure { error ->
                showSnackbar(error.message ?: "Failed to update Previous Context.")
            }
        }
    }

    fun consumeSnackbar() {
        _uiState.update { current ->
            current.copy(snackbarMessage = null)
        }
    }

    fun handleSpotifyAuthRedirect(callbackUrl: String) {
        if (callbackUrl == lastHandledSpotifyCallbackUrl) {
            return
        }
        if (!_uiState.value.isCoreReady) {
            pendingSpotifyCallbackUrl = callbackUrl
            return
        }
        completeSpotifyAuth(callbackUrl)
    }

    private fun observeCoreEvents() {
        viewModelScope.launch {
            bridge.events.collect(::handleCoreEvent)
        }
    }

    private fun initializeCore() {
        viewModelScope.launch {
            val application = getApplication<Application>()
            val configDir = AssetConfigInstaller(application).install()
            runCatching {
                bridge.initialize(
                    appFilesDir = application.filesDir.absolutePath,
                    assetConfigDir = configDir.absolutePath,
                )
                val bootstrap = bridge.loadBootstrapState()
                val overview = bridge.loadOverviewSnapshot()
                _uiState.update { current ->
                    current.copy(
                        bootstrapState = bootstrap,
                        overviewSnapshot = overview,
                        isInitializing = false,
                        initializationError = null,
                        isCoreReady = true,
                        toolStatus = "HGIE ready.",
                    )
                }
                processPendingSpotifyCallback()
            }.onFailure { error ->
                _uiState.update { current ->
                    current.copy(
                        isInitializing = false,
                        initializationError = error.message ?: "Failed to initialize Baby Gervaise core.",
                        isCoreReady = false,
                        toolStatus = "Initialization failed.",
                    )
                }
                showSnackbar(error.message ?: "Failed to initialize Baby Gervaise core.")
            }
        }
    }

    private suspend fun refreshBootstrapState() {
        val bootstrap = bridge.loadBootstrapState()
        _uiState.update { current ->
            current.copy(bootstrapState = bootstrap)
        }
    }

    private fun refreshOverviewSnapshot() {
        viewModelScope.launch {
            runCatching {
                val overview = bridge.loadOverviewSnapshot()
                _uiState.update { current ->
                    current.copy(overviewSnapshot = overview)
                }
            }.onFailure { error ->
                showSnackbar(error.message ?: "Failed to load overview.")
            }
        }
    }

    private fun handleCoreEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.AssistantStarted -> {
                _uiState.update { current ->
                    current.copy(
                        pendingTurnId = event.turnId,
                        toolStatus = "Gervaise is thinking.",
                        bootstrapState = current.bootstrapState.copy(
                            messages = ensureAssistantPlaceholder(
                                current.bootstrapState.messages,
                                event.turnId,
                            ),
                        ),
                    )
                }
            }

            is CoreEvent.AssistantChunk -> {
                _uiState.update { current ->
                    current.copy(
                        bootstrapState = current.bootstrapState.copy(
                            messages = current.bootstrapState.messages.map { message ->
                                if (message.turnId == event.turnId && message.role == "assistant") {
                                    message.copy(
                                        content = listOf(message.content, event.chunk)
                                            .filter { it.isNotBlank() }
                                            .joinToString(" "),
                                    )
                                } else {
                                    message
                                }
                            },
                        ),
                    )
                }
            }

            is CoreEvent.AssistantCompleted -> {
                _uiState.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        toolStatus = "HGIE ready.",
                        bootstrapState = current.bootstrapState.copy(
                            messages = replaceAssistantMessage(
                                current.bootstrapState.messages,
                                event.turnId,
                                event.message,
                            ),
                        ),
                    )
                }
                viewModelScope.launch {
                    runCatching {
                        refreshBootstrapState()
                        val overview = bridge.loadOverviewSnapshot()
                        _uiState.update { current ->
                            current.copy(overviewSnapshot = overview)
                        }
                    }.onFailure { error ->
                        showSnackbar(error.message ?: "Failed to refresh state after response.")
                    }
                }
            }

            is CoreEvent.ToolStatus -> {
                _uiState.update { current ->
                    current.copy(toolStatus = "${event.tool}.${event.action} is ${event.status}")
                }
            }

            is CoreEvent.OpenExternalUrl -> {
                launchExternalUrl(event.url)
            }

            is CoreEvent.AssistantError -> {
                _uiState.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        toolStatus = event.error,
                    )
                }
                viewModelScope.launch {
                    runCatching {
                        refreshBootstrapState()
                        val overview = bridge.loadOverviewSnapshot()
                        _uiState.update { current ->
                            current.copy(overviewSnapshot = overview)
                        }
                    }.onFailure { error ->
                        showSnackbar(error.message ?: "Failed to refresh state after error.")
                    }
                }
            }

            is CoreEvent.ConfigUpdated -> {
                _uiState.update { current ->
                    current.copy(
                        bootstrapState = current.bootstrapState.copy(previousContext = event.level),
                        overviewSnapshot = current.overviewSnapshot.copy(previousContext = event.level),
                    )
                }
            }
        }
    }

    private fun showSnackbar(message: String) {
        _uiState.update { current ->
            current.copy(snackbarMessage = message)
        }
    }

    private fun processPendingSpotifyCallback() {
        val callbackUrl = pendingSpotifyCallbackUrl ?: return
        pendingSpotifyCallbackUrl = null
        handleSpotifyAuthRedirect(callbackUrl)
    }

    private fun completeSpotifyAuth(callbackUrl: String) {
        if (callbackUrl == lastHandledSpotifyCallbackUrl) {
            return
        }
        lastHandledSpotifyCallbackUrl = callbackUrl
        val turnId = "spotify-auth-${UUID.randomUUID()}"
        _uiState.update { current ->
            current.copy(
                pendingTurnId = turnId,
                toolStatus = "Completing Spotify sign-in...",
            )
        }
        viewModelScope.launch {
            runCatching {
                bridge.handleSpotifyAuthCallback(turnId, callbackUrl)
            }.onFailure { error ->
                _uiState.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        toolStatus = error.message ?: "Spotify authentication failed.",
                    )
                }
                showSnackbar(error.message ?: "Spotify authentication failed.")
            }
        }
    }

    private fun launchExternalUrl(url: String) {
        val application = getApplication<Application>()
        runCatching {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            application.startActivity(intent)
        }.onFailure { error ->
            showSnackbar(error.message ?: "Failed to open browser.")
        }
    }

    private fun createLocalMessage(role: String, content: String, turnId: String): ChatMessage =
        ChatMessage(
            id = Instant.now().toEpochMilli(),
            role = role,
            content = content,
            turnId = turnId,
            inputSource = InputSource.TEXT,
            createdAt = Instant.now().toString(),
        )

    private fun ensureAssistantPlaceholder(
        messages: List<ChatMessage>,
        turnId: String,
    ): List<ChatMessage> =
        if (messages.any { it.turnId == turnId && it.role == "assistant" }) {
            messages
        } else {
            messages + createLocalMessage("assistant", "", turnId)
        }

    private fun replaceAssistantMessage(
        messages: List<ChatMessage>,
        turnId: String,
        nextMessage: ChatMessage,
    ): List<ChatMessage> {
        var replaced = false
        val updated = messages.map { message ->
            if (message.turnId == turnId && message.role == "assistant") {
                replaced = true
                nextMessage
            } else {
                message
            }
        }
        return if (replaced) updated else updated + nextMessage
    }

    override fun onCleared() {
        bridge.close()
        super.onCleared()
    }
}
