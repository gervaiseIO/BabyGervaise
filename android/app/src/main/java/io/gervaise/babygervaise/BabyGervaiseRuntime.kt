package io.gervaise.babygervaise

import android.app.Application
import android.content.Intent
import android.net.Uri
import android.util.Log
import io.gervaise.babygervaise.bridge.BootstrapState
import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.CoreEvent
import io.gervaise.babygervaise.bridge.DebugLogEntry
import io.gervaise.babygervaise.bridge.InputSource
import io.gervaise.babygervaise.bridge.MessageContentType
import io.gervaise.babygervaise.bridge.NativeCoreBridge
import io.gervaise.babygervaise.bridge.NoteActivityEvent
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import java.time.Duration
import java.time.Instant
import java.util.UUID
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonPrimitive

data class InteractionErrorSnapshot(
    val id: String,
    val message: String,
    val timestampMs: Long,
)

data class BabyGervaiseCoreState(
    val bootstrapState: BootstrapState = BootstrapState.Empty,
    val overviewSnapshot: OverviewSnapshot = OverviewSnapshot.Empty,
    val pendingTurnId: String? = null,
    val activeToolStatus: ActiveToolStatus? = null,
    val toolStatus: String? = null,
    val isInitializing: Boolean = true,
    val initializationError: String? = null,
    val isCoreReady: Boolean = false,
    val latestAssistantTurnId: String? = null,
    val latestAssistantMessage: ChatMessage? = null,
    val interactionError: InteractionErrorSnapshot? = null,
    val deliveryState: DeliveryState = DeliveryState.IDLE,
) {
    val statusText: String
        get() = initializationError ?: toolStatus ?: "HGIE ready."

    val canSubmitTurns: Boolean
        get() = isCoreReady && hasConversationLane(overviewSnapshot)
}

class BabyGervaiseRuntime(
    private val application: Application,
) {
    companion object {
        private const val TAG = "BGRuntime"
        private const val REQUIRED_IDLE_SECONDS = 15 * 60L
        private const val WELCOME_BACK_REQUEST_DEBOUNCE_SECONDS = 60L
    }

    private val bridge = NativeCoreBridge(nanoHost = MlKitNanoHost())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val _state = MutableStateFlow(BabyGervaiseCoreState())
    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 16)
    private var pendingSpotifyCallbackUrl: String? = null
    private var lastHandledSpotifyCallbackUrl: String? = null
    private var lastForegroundAt: Instant? = null
    private var lastWelcomeBackRequestAt: Instant? = null
    private var nanoReadinessMonitorJob: Job? = null

    val state = _state.asStateFlow()
    val messages = _messages.asSharedFlow()

    init {
        observeCoreEvents()
        initializeCore()
    }

    fun submitUserTurn(
        text: String,
        inputSource: InputSource,
    ): Boolean {
        val snapshot = _state.value
        if (!snapshot.canSubmitTurns) {
            Log.w(TAG, "submitUserTurn rejected ready=${snapshot.isCoreReady} pending=${snapshot.pendingTurnId != null}")
            emitMessage(snapshot.initializationError ?: mainChatStatusForOverview(snapshot.overviewSnapshot))
            return false
        }

        val trimmed = text.trim()
        if (trimmed.isEmpty() || snapshot.pendingTurnId != null) {
            return false
        }
        Log.i(TAG, "submitUserTurn source=${inputSource.name.lowercase()} chars=${trimmed.length}")

        val turnId = UUID.randomUUID().toString()
        _state.update { current ->
            current.copy(
                pendingTurnId = turnId,
                activeToolStatus = null,
                toolStatus = "Waiting for Gervaise.",
                interactionError = null,
                deliveryState = DeliveryState.LOCAL_TYPING,
                bootstrapState = current.bootstrapState.copy(
                    messages = current.bootstrapState.messages +
                        createLocalMessage("user", trimmed, turnId, inputSource) +
                        createLocalMessage("assistant", "", turnId, inputSource),
                ),
            )
        }

        scope.launch {
            runCatching {
                bridge.submitUserTurn(
                    turnId = turnId,
                    text = trimmed,
                    inputSource = inputSource,
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
        return true
    }

    fun setPreviousContext(level: ContextLevel) {
        if (!_state.value.isCoreReady) {
            emitMessage(_state.value.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }

        _state.update { current ->
            current.copy(
                bootstrapState = current.bootstrapState.copy(previousContext = level),
                overviewSnapshot = current.overviewSnapshot.copy(previousContext = level),
            )
        }

        scope.launch {
            runCatching {
                bridge.setPreviousContext(level)
                refreshOverviewSnapshot(force = false)
            }.onFailure { error ->
                emitMessage(error.message ?: "Failed to update Previous Context.")
            }
        }
    }

    fun setCloudProfile(profileId: String) {
        val snapshot = _state.value
        if (!snapshot.isCoreReady) {
            emitMessage(snapshot.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        if (snapshot.pendingTurnId != null) {
            emitMessage("Wait for the current Gervaise response to finish before changing models.")
            return
        }

        scope.launch {
            runCatching {
                bridge.setCloudProfile(profileId)
                Log.i(TAG, "cloudProfile selected=$profileId")
                refreshOverviewSnapshot(force = false)
            }.onFailure { error ->
                Log.e(TAG, "cloudProfile failed profile=$profileId", error)
                emitMessage(error.message ?: "Failed to update cloud model.")
            }
        }
    }

    fun onAppResumed() {
        val snapshot = _state.value
        if (snapshot.isCoreReady) {
            refreshOverviewSnapshot(force = false)
        }
        if (!snapshot.isCoreReady || snapshot.pendingTurnId != null) {
            lastForegroundAt = Instant.now()
            return
        }

        val previousForeground = lastForegroundAt
        lastForegroundAt = Instant.now()
        val idleSeconds = previousForeground
            ?.let { Duration.between(it, Instant.now()).seconds.coerceAtLeast(0) }
            ?: return
        maybeTriggerWelcomeBack(idleSeconds = idleSeconds)
    }

    fun refreshOverviewSnapshot(
        force: Boolean = true,
        allowAmbientTriggers: Boolean = true,
    ) {
        if (!_state.value.isCoreReady) {
            if (force) {
                emitMessage(_state.value.initializationError ?: "Baby Gervaise is not ready yet.")
            }
            return
        }

        scope.launch {
            runCatching {
                val previousOverview = _state.value.overviewSnapshot
                val overview = bridge.loadOverviewSnapshot()
                applyOverviewSnapshot(overview)
                if (allowAmbientTriggers) {
                    maybeTriggerCapabilityAmbient(previousOverview, overview)
                }
            }.onFailure { error ->
                emitMessage(error.message ?: "Failed to load overview.")
            }
        }
    }

    fun recordNoteActivity(event: NoteActivityEvent) {
        if (!_state.value.isCoreReady) {
            return
        }
        scope.launch {
            runCatching {
                bridge.recordNoteActivity(event)
            }.onFailure { error ->
                Log.w(TAG, "recordNoteActivity failed type=${event.eventType}", error)
            }
        }
    }

    fun beginToolAuth(tool: String) {
        val snapshot = _state.value
        if (!snapshot.isCoreReady) {
            emitMessage(snapshot.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        if (snapshot.pendingTurnId != null) {
            emitMessage("Wait for the current Gervaise response to finish before changing ${tool.replaceFirstChar { it.uppercase() }}.")
            return
        }
        launchToolLifecycle(
            tool = tool,
            action = "connect",
            pendingStatus = "Connecting ${tool.replaceFirstChar { it.uppercase() }}...",
        ) {
            bridge.beginToolAuth(tool)
        }
    }

    fun disconnectTool(tool: String) {
        val snapshot = _state.value
        if (!snapshot.isCoreReady) {
            emitMessage(snapshot.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        if (snapshot.pendingTurnId != null) {
            emitMessage("Wait for the current Gervaise response to finish before changing ${tool.replaceFirstChar { it.uppercase() }}.")
            return
        }
        if (
            snapshot.activeToolStatus?.tool == tool &&
            snapshot.activeToolStatus.action == "disconnect" &&
            snapshot.activeToolStatus.status == "executing"
        ) {
            return
        }
        launchToolLifecycle(
            tool = tool,
            action = "disconnect",
            pendingStatus = "Disconnecting ${tool.replaceFirstChar { it.uppercase() }}...",
        ) {
            bridge.disconnectTool(tool)
        }
    }

    fun refreshToolState(tool: String) {
        val snapshot = _state.value
        if (!snapshot.isCoreReady) {
            emitMessage(snapshot.initializationError ?: "Baby Gervaise is not ready yet.")
            return
        }
        if (snapshot.pendingTurnId != null) {
            emitMessage("Wait for the current Gervaise response to finish before refreshing ${tool.replaceFirstChar { it.uppercase() }}.")
            return
        }
        launchToolLifecycle(
            tool = tool,
            action = "refresh_state",
            pendingStatus = "Refreshing ${tool.replaceFirstChar { it.uppercase() }} state...",
        ) {
            bridge.refreshToolState(tool)
        }
    }

    fun handleToolAuthRedirect(
        tool: String,
        callbackUrl: String,
    ) {
        if (tool == "spotify" && callbackUrl == lastHandledSpotifyCallbackUrl) {
            return
        }
        if (!_state.value.isCoreReady) {
            pendingSpotifyCallbackUrl = callbackUrl
            return
        }
        completeToolAuth(tool, callbackUrl)
    }

    private fun observeCoreEvents() {
        scope.launch {
            bridge.events.collectLatest(::handleCoreEvent)
        }
    }

    private fun initializeCore() {
        scope.launch {
            val configDir = AssetConfigInstaller(application).install()
            runCatching {
                bridge.initialize(
                    appFilesDir = application.filesDir.absolutePath,
                    assetConfigDir = configDir.absolutePath,
                )
                val bootstrap = bridge.loadBootstrapState()
                val overview = bridge.loadOverviewSnapshot()
                _state.update { current ->
                    current.copy(
                        bootstrapState = bootstrap,
                        overviewSnapshot = overview,
                        isInitializing = false,
                        initializationError = null,
                        isCoreReady = true,
                        toolStatus = mainChatStatusForOverview(overview),
                        interactionError = null,
                    )
                }
                Log.i(TAG, "initializeCore success")
                syncNanoReadinessMonitor()
                processPendingSpotifyCallback()
                maybeTriggerWelcomeBack()
            }.onFailure { error ->
                _state.update { current ->
                    current.copy(
                        isInitializing = false,
                        initializationError = error.message ?: "Failed to initialize Baby Gervaise core.",
                        isCoreReady = false,
                        toolStatus = "Initialization failed.",
                        interactionError = InteractionErrorSnapshot(
                            id = "initialize-${System.currentTimeMillis()}",
                            message = error.message ?: "Failed to initialize Baby Gervaise core.",
                            timestampMs = System.currentTimeMillis(),
                        ),
                        deliveryState = DeliveryState.IDLE,
                    )
                }
                Log.e(TAG, "initializeCore failed", error)
                emitMessage(error.message ?: "Failed to initialize Baby Gervaise core.")
            }
        }
    }

    private suspend fun refreshBootstrapState() {
        val bootstrap = bridge.loadBootstrapState()
        _state.update { current ->
            current.copy(bootstrapState = bootstrap)
        }
    }

    private fun handleCoreEvent(event: CoreEvent) {
        when (event) {
            is CoreEvent.AssistantStarted -> {
                _state.update { current ->
                    current.copy(
                        pendingTurnId = event.turnId,
                        activeToolStatus = null,
                        toolStatus = "Gervaise is thinking.",
                        interactionError = null,
                        deliveryState = if (current.deliveryState == DeliveryState.CLOUD_WORKING) {
                            DeliveryState.CLOUD_WORKING
                        } else {
                            DeliveryState.LOCAL_TYPING
                        },
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
                _state.update { current ->
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
                _state.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        activeToolStatus = null,
                        toolStatus = mainChatStatusForOverview(current.overviewSnapshot),
                        latestAssistantTurnId = event.turnId,
                        latestAssistantMessage = event.message,
                        interactionError = null,
                        deliveryState = DeliveryState.IDLE,
                        bootstrapState = current.bootstrapState.copy(
                            messages = replaceAssistantMessage(
                                current.bootstrapState.messages,
                                event.turnId,
                                event.message,
                            ),
                        ),
                    )
                }
                scope.launch {
                    runCatching {
                        refreshBootstrapState()
                        refreshOverviewSnapshot(force = false)
                    }.onFailure { error ->
                        emitMessage(error.message ?: "Failed to refresh state after response.")
                    }
                }
            }

            is CoreEvent.ToolStatus -> {
                _state.update { current ->
                    current.copy(
                        activeToolStatus = ActiveToolStatus(
                            turnId = event.turnId,
                            tool = event.tool,
                            action = event.action,
                            status = event.status,
                        ),
                        toolStatus = formatToolStatus(event.tool, event.action, event.status),
                    )
                }
            }

            is CoreEvent.OpenExternalUrl -> {
                _state.update { current ->
                    current.copy(
                        activeToolStatus = when (event.purpose) {
                            "spotify_auth" -> ActiveToolStatus(
                                turnId = event.turnId,
                                tool = "spotify",
                                action = "connect",
                                status = "auth_started",
                            )

                            else -> current.activeToolStatus
                        },
                        toolStatus = when (event.purpose) {
                            "spotify_auth" -> "Spotify sign-in opened in your browser."
                            else -> current.toolStatus
                        },
                    )
                }
                launchExternalUrl(event.url)
            }

            is CoreEvent.AssistantError -> {
                val presentedError = presentAssistantError(
                    error = event.error,
                    overview = _state.value.overviewSnapshot,
                )
                _state.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        activeToolStatus = null,
                        toolStatus = presentAssistantError(
                            error = event.error,
                            overview = current.overviewSnapshot,
                        ),
                        deliveryState = DeliveryState.IDLE,
                        interactionError = InteractionErrorSnapshot(
                            id = "assistant-error-${System.currentTimeMillis()}",
                            message = presentedError,
                            timestampMs = System.currentTimeMillis(),
                        ),
                    )
                }
                emitMessage(presentedError)
                scope.launch {
                    runCatching {
                        refreshBootstrapState()
                        val overview = bridge.loadOverviewSnapshot()
                        applyOverviewSnapshot(
                            overview = overview,
                            preserveToolStatus = true,
                        )
                    }.onFailure { error ->
                        emitMessage(error.message ?: "Failed to refresh state after error.")
                    }
                }
            }

            is CoreEvent.ConfigUpdated -> {
                _state.update { current ->
                    current.copy(
                        bootstrapState = current.bootstrapState.copy(previousContext = event.level),
                        overviewSnapshot = current.overviewSnapshot.copy(previousContext = event.level),
                    )
                }
            }

            is CoreEvent.DebugLog -> {
                updateDeliveryStateFromDiagnostic(event.entry)
                val fields = event.entry.fields?.entries
                    ?.joinToString(separator = " ") { (key, value) -> "$key=$value" }
                    .orEmpty()
                val message = "${event.entry.message} $fields".trim()
                when (event.entry.level.lowercase()) {
                    "error" -> Log.e(coreTag(event.entry.subsystem), message)
                    "warning", "warn" -> Log.w(coreTag(event.entry.subsystem), message)
                    else -> Log.i(coreTag(event.entry.subsystem), message)
                }
            }
        }
    }

    private fun emitMessage(message: String) {
        _messages.tryEmit(message)
    }

    private fun applyOverviewSnapshot(
        overview: OverviewSnapshot,
        preserveToolStatus: Boolean = false,
    ) {
        _state.update { current ->
            current.copy(
                overviewSnapshot = overview,
                toolStatus = if (current.pendingTurnId == null && !preserveToolStatus) {
                    mainChatStatusForOverview(overview)
                } else {
                    current.toolStatus
                },
            )
        }
        syncNanoReadinessMonitor()
    }

    private fun syncNanoReadinessMonitor() {
        val snapshot = _state.value
        val shouldMonitor =
            snapshot.isCoreReady &&
                snapshot.pendingTurnId == null &&
                !snapshot.overviewSnapshot.runtime.nano.active

        if (!shouldMonitor) {
            nanoReadinessMonitorJob?.cancel()
            nanoReadinessMonitorJob = null
            return
        }

        if (nanoReadinessMonitorJob?.isActive == true) {
            return
        }

        nanoReadinessMonitorJob = scope.launch {
            while (true) {
                delay(1500)
                val current = _state.value
                if (
                    !current.isCoreReady ||
                    current.pendingTurnId != null ||
                    current.overviewSnapshot.runtime.nano.active
                ) {
                    break
                }
                runCatching { bridge.loadOverviewSnapshot() }
                    .onSuccess { overview ->
                        val previousOverview = _state.value.overviewSnapshot
                        applyOverviewSnapshot(overview)
                        maybeTriggerCapabilityAmbient(previousOverview, overview)
                    }
            }
            nanoReadinessMonitorJob = null
        }
    }

    private fun mainChatStatusForOverview(overview: OverviewSnapshot): String =
        when {
            overview.runtime.nano.availability == "available" -> "HGIE ready."
            hasConversationLane(overview) -> "HGIE ready through cloud."
            overview.runtime.nano.availability == "downloading" ->
                "Gervaise is preparing on-device AI."
            overview.runtime.nano.availability in setOf("unavailable", "disabled") ->
                "Conversation AI is not ready yet."
            overview.runtime.nano.availability == "error" ->
                "Gervaise hit a local AI issue. Please try again."
            else -> overview.runtime.nano.detail
        }

    private fun presentAssistantError(
        error: String,
        overview: OverviewSnapshot,
    ): String {
        val lower = error.lowercase()
        return when {
            lower.contains("downloading") -> "Gervaise is preparing on-device AI."
            lower.contains("unavailable") || lower.contains("not ready") -> "On-device AI is not ready yet."
            lower.contains("nano") ||
                lower.contains("local ai") ||
                lower.contains("maxoutputtokens") ||
                lower.contains("on-device") -> "Gervaise hit a local AI issue. Please try again."

            else -> error.ifBlank { mainChatStatusForOverview(overview) }
        }
    }

    private fun maybeTriggerCapabilityAmbient(
        previousOverview: OverviewSnapshot,
        nextOverview: OverviewSnapshot,
    ) {
        if (!nextOverview.runtime.nano.active || _state.value.pendingTurnId != null) {
            return
        }
        val wasConnected = spotifyConnected(previousOverview)
        val isConnected = spotifyConnected(nextOverview)
        if (!wasConnected && isConnected) {
            submitAmbientEvent(
                eventType = "capability_available",
                payloadJson = """{"capability":"spotify"}""",
            )
        }
    }

    private fun spotifyConnected(overview: OverviewSnapshot): Boolean {
        return overview.tools.catalog
            .firstOrNull { it.toolId == "spotify" }
            ?.integrated == true
    }

    private fun submitAmbientEvent(
        eventType: String,
        payloadJson: String,
    ) {
        if (_state.value.pendingTurnId != null) {
            return
        }
        val turnId = "ambient-${UUID.randomUUID()}"
        scope.launch {
            runCatching {
                bridge.submitAmbientEvent(
                    turnId = turnId,
                    eventType = eventType,
                    payloadJson = payloadJson,
                )
            }.onFailure { error ->
                emitMessage(error.message ?: "Ambient update failed.")
            }
        }
    }

    private fun maybeTriggerWelcomeBack(idleSeconds: Long? = null) {
        val snapshot = _state.value
        val now = Instant.now()
        val effectiveIdleSeconds = resolvedWelcomeBackIdleSeconds(
            messages = snapshot.bootstrapState.messages,
            now = now,
            explicitIdleSeconds = idleSeconds,
        )
        if (
            !shouldTriggerWelcomeBack(
                isCoreReady = snapshot.isCoreReady,
                pendingTurnId = snapshot.pendingTurnId,
                nanoActive = snapshot.overviewSnapshot.runtime.nano.active,
                messages = snapshot.bootstrapState.messages,
                now = now,
                resolvedIdleSeconds = effectiveIdleSeconds,
                lastWelcomeBackRequestAt = lastWelcomeBackRequestAt,
                requiredIdleSeconds = REQUIRED_IDLE_SECONDS,
                debounceSeconds = WELCOME_BACK_REQUEST_DEBOUNCE_SECONDS,
            )
        ) {
            return
        }

        val idleSecondsToSend = effectiveIdleSeconds
            ?: return
        lastWelcomeBackRequestAt = now
        submitAmbientEvent(
            eventType = "resume_after_idle",
            payloadJson = """{"idle_seconds":$idleSecondsToSend}""",
        )
    }

    private fun updateDeliveryStateFromDiagnostic(entry: DebugLogEntry) {
        if (!isCloudWorkingDiagnostic(entry) || _state.value.pendingTurnId == null) {
            return
        }
        _state.update { current ->
            if (current.deliveryState == DeliveryState.CLOUD_WORKING) {
                current
            } else {
                current.copy(deliveryState = DeliveryState.CLOUD_WORKING)
            }
        }
    }

    private fun processPendingSpotifyCallback() {
        val callbackUrl = pendingSpotifyCallbackUrl ?: return
        pendingSpotifyCallbackUrl = null
        handleToolAuthRedirect("spotify", callbackUrl)
    }

    private fun completeToolAuth(
        tool: String,
        callbackUrl: String,
    ) {
        if (tool == "spotify" && callbackUrl == lastHandledSpotifyCallbackUrl) {
            return
        }
        if (tool == "spotify") {
            lastHandledSpotifyCallbackUrl = callbackUrl
        }
        val turnId = "$tool-auth-${UUID.randomUUID()}"
        _state.update { current ->
            current.copy(
                pendingTurnId = turnId,
                activeToolStatus = ActiveToolStatus(
                    turnId = turnId,
                    tool = tool,
                    action = "connect",
                    status = "executing",
                ),
                toolStatus = "Completing ${tool.replaceFirstChar { it.uppercase() }} sign-in...",
                interactionError = null,
                deliveryState = DeliveryState.LOCAL_TYPING,
            )
        }
        scope.launch {
            runCatching {
                bridge.handleToolAuthCallback(tool, turnId, callbackUrl)
            }.onFailure { error ->
                _state.update { current ->
                    current.copy(
                        pendingTurnId = null,
                        activeToolStatus = null,
                        toolStatus = error.message ?: "${tool.replaceFirstChar { it.uppercase() }} authentication failed.",
                        deliveryState = DeliveryState.IDLE,
                        interactionError = InteractionErrorSnapshot(
                            id = "auth-$tool-${System.currentTimeMillis()}",
                            message = error.message ?: "${tool.replaceFirstChar { it.uppercase() }} authentication failed.",
                            timestampMs = System.currentTimeMillis(),
                        ),
                    )
                }
                Log.e(TAG, "toolAuth failed tool=$tool", error)
                emitMessage(error.message ?: "${tool.replaceFirstChar { it.uppercase() }} authentication failed.")
            }
        }
    }

    private fun launchToolLifecycle(
        tool: String,
        action: String,
        pendingStatus: String,
        block: suspend () -> io.gervaise.babygervaise.bridge.ToolExecutionResult,
    ) {
        _state.update { current ->
            current.copy(
                activeToolStatus = ActiveToolStatus(
                    turnId = "$tool-overview-$action",
                    tool = tool,
                    action = action,
                    status = "executing",
                ),
                toolStatus = pendingStatus,
                interactionError = null,
            )
        }

        scope.launch {
            runCatching { block() }
                .onSuccess { result ->
                    Log.i(TAG, "toolLifecycle tool=$tool action=$action summary=${result.summary}")
                    _state.update { current ->
                        current.copy(
                            activeToolStatus = null,
                            toolStatus = "HGIE ready.",
                            interactionError = null,
                        )
                    }
                    emitMessage(result.summary)
                    refreshOverviewSnapshot(force = false)
                }
                .onFailure { error ->
                    Log.e(TAG, "toolLifecycle failed tool=$tool action=$action", error)
                    _state.update { current ->
                        current.copy(
                            activeToolStatus = null,
                            toolStatus = error.message ?: "${tool.replaceFirstChar { it.uppercase() }} update failed.",
                            interactionError = InteractionErrorSnapshot(
                                id = "tool-$tool-$action-${System.currentTimeMillis()}",
                                message = error.message ?: "${tool.replaceFirstChar { it.uppercase() }} update failed.",
                                timestampMs = System.currentTimeMillis(),
                            ),
                        )
                    }
                    emitMessage(error.message ?: "${tool.replaceFirstChar { it.uppercase() }} update failed.")
                    refreshOverviewSnapshot(force = false)
                }
        }
    }

    private fun launchExternalUrl(url: String) {
        runCatching {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            application.startActivity(intent)
        }.onFailure { error ->
            emitMessage(error.message ?: "Failed to open browser.")
        }
    }

    private fun formatToolStatus(
        tool: String,
        action: String,
        status: String,
    ): String {
        if (status != "executing") {
            return "${tool.replaceFirstChar { it.uppercase() }} updated."
        }

        return when (tool.lowercase()) {
            "spotify" -> when (action.lowercase()) {
                "play" -> "Trying Spotify playback."
                "pause" -> "Pausing Spotify playback."
                "next" -> "Skipping ahead on Spotify."
                "previous" -> "Going back on Spotify."
                "set_volume" -> "Adjusting Spotify volume."
                "select_device" -> "Checking Spotify devices."
                "connect", "get_connection_state" -> "Checking Spotify connection."
                else -> "Working with Spotify."
            }

            "hue" -> when (action.lowercase()) {
                "set_power" -> "Updating your Hue lights."
                "set_brightness" -> "Adjusting Hue brightness."
                "set_color" -> "Changing Hue color."
                "activate_scene" -> "Activating a Hue scene."
                else -> "Working with Hue."
            }

            else -> "Working with ${tool.replaceFirstChar { it.uppercase() }}."
        }
    }

    private fun coreTag(subsystem: String): String =
        when (subsystem.lowercase()) {
            "hgie" -> "BGHgie"
            "memory" -> "BGMemory"
            "model" -> "BGModel"
            "tools" -> "BGTools"
            else -> "BGCore"
        }

    private fun createLocalMessage(
        role: String,
        content: String,
        turnId: String,
        inputSource: InputSource,
    ): ChatMessage = ChatMessage(
        id = Instant.now().toEpochMilli(),
        role = role,
        content = content,
        turnId = turnId,
        inputSource = inputSource,
        createdAt = Instant.now().toString(),
        contentType = MessageContentType.PLAIN_TEXT,
        displayJson = null,
        visibleSummary = null,
    )

    private fun ensureAssistantPlaceholder(
        messages: List<ChatMessage>,
        turnId: String,
    ): List<ChatMessage> =
        if (messages.any { it.turnId == turnId && it.role == "assistant" }) {
            messages
        } else {
            messages + createLocalMessage("assistant", "", turnId, InputSource.TEXT)
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
}

internal fun hasConversationLane(overview: OverviewSnapshot): Boolean =
    overview.runtime.nano.active || overview.runtime.cloudProfiles.any { profile -> profile.available }

internal fun hasPersistedConversationHistory(messages: List<ChatMessage>): Boolean =
    messages.any { message ->
        message.role in setOf("user", "assistant") && message.content.isNotBlank()
    }

internal fun latestConversationActivity(messages: List<ChatMessage>): Instant? =
    messages
        .asSequence()
        .filter { message -> message.role in setOf("user", "assistant") }
        .filter { message -> message.content.isNotBlank() }
        .mapNotNull { message ->
            runCatching { Instant.parse(message.createdAt) }.getOrNull()
        }
        .maxOrNull()

internal fun resolvedWelcomeBackIdleSeconds(
    messages: List<ChatMessage>,
    now: Instant,
    explicitIdleSeconds: Long?,
): Long? =
    explicitIdleSeconds ?: latestConversationActivity(messages)
        ?.let { lastActivity -> Duration.between(lastActivity, now).seconds.coerceAtLeast(0) }

internal fun shouldTriggerWelcomeBack(
    isCoreReady: Boolean,
    pendingTurnId: String?,
    nanoActive: Boolean,
    messages: List<ChatMessage>,
    now: Instant,
    resolvedIdleSeconds: Long?,
    lastWelcomeBackRequestAt: Instant?,
    requiredIdleSeconds: Long,
    debounceSeconds: Long,
): Boolean {
    if (!isCoreReady || pendingTurnId != null || !nanoActive) {
        return false
    }
    if (!hasPersistedConversationHistory(messages)) {
        return false
    }
    if (resolvedIdleSeconds == null || resolvedIdleSeconds < requiredIdleSeconds) {
        return false
    }
    if (
        lastWelcomeBackRequestAt?.let { previous ->
            Duration.between(previous, now).seconds < debounceSeconds
        } == true
    ) {
        return false
    }
    return true
}

internal fun isCloudWorkingDiagnostic(entry: DebugLogEntry): Boolean {
    if (entry.subsystem != "hgie" || entry.message != "turn route selected") {
        return false
    }
    return entry.fields
        ?.get("cloud_escalated")
        ?.jsonPrimitive
        ?.booleanOrNull == true
}
