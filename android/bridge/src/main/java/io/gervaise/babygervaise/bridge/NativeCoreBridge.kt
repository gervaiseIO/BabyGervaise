package io.gervaise.babygervaise.bridge

import android.util.Log
import java.io.Closeable
import java.util.concurrent.Executors
import kotlinx.coroutines.ExecutorCoroutineDispatcher
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

class NativeCoreBridge(
    private val nativeCore: NativeBabyGervaise = NativeBabyGervaise(),
    private val nanoHost: NanoHost,
) : Closeable {
    companion object {
        private const val TAG = "BGBridge"
    }

    private val dispatcher: ExecutorCoroutineDispatcher =
        Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val initializeMutex = Mutex()
    private val eventsFlow = MutableSharedFlow<CoreEvent>(
        extraBufferCapacity = 32,
    )
    private var isInitialized = false

    val events = eventsFlow.asSharedFlow()

    private val callbacks = CoreCallbackChannel { eventType, payloadJson ->
        val decoded = runCatching {
            CoreJson.decodeEvent(eventType, payloadJson)
        }.getOrElse { error ->
            CoreEvent.AssistantError(
                turnId = null,
                error = error.message ?: "Failed to decode core event.",
            )
        }
        eventsFlow.tryEmit(decoded)
    }

    suspend fun initialize(
        appFilesDir: String,
        assetConfigDir: String,
    ) {
        initializeMutex.withLock {
            if (isInitialized) {
                return@withLock
            }

            withContext(dispatcher) {
                Log.i(TAG, "initialize core")
                nativeCore.init(
                    appFilesDir = appFilesDir,
                    assetConfigDir = assetConfigDir,
                    callbacks = callbacks,
                    nanoHost = nanoHost,
                )
            }
            isInitialized = true
        }
    }

    suspend fun loadBootstrapState(): BootstrapState = withContext(dispatcher) {
        requireInitialized()
        CoreJson.decodeBootstrapState(nativeCore.loadBootstrapState())
    }

    suspend fun loadOverviewSnapshot(): OverviewSnapshot = withContext(dispatcher) {
        requireInitialized()
        CoreJson.decodeOverviewSnapshot(nativeCore.loadOverviewState())
    }

    suspend fun submitUserTurn(
        turnId: String,
        text: String,
        inputSource: InputSource = InputSource.TEXT,
    ) {
        withContext(dispatcher) {
            requireInitialized()
            Log.i(TAG, "submit turnId=$turnId source=${inputSource.name.lowercase()}")
            nativeCore.submitUserTurn(
                turnId = turnId,
                text = text,
                inputSource = inputSource.name.lowercase(),
            )
        }
    }

    suspend fun handleSpotifyAuthCallback(
        turnId: String,
        callbackUrl: String,
    ) {
        withContext(dispatcher) {
            requireInitialized()
            nativeCore.handleSpotifyAuthCallback(turnId, callbackUrl)
        }
    }

    suspend fun handleToolAuthCallback(
        tool: String,
        turnId: String,
        callbackUrl: String,
    ) {
        withContext(dispatcher) {
            requireInitialized()
            Log.i(TAG, "handle auth callback tool=$tool turnId=$turnId")
            nativeCore.handleToolAuthCallback(tool, turnId, callbackUrl)
        }
    }

    suspend fun executeToolAction(
        tool: String,
        action: String,
        argumentsJson: String = "{}",
    ): ToolExecutionResult = withContext(dispatcher) {
        requireInitialized()
        CoreJson.decodeToolExecutionResult(
            nativeCore.executeToolAction(
                tool = tool,
                action = action,
                argumentsJson = argumentsJson,
            ),
        )
    }

    suspend fun beginToolAuth(tool: String): ToolExecutionResult = withContext(dispatcher) {
        requireInitialized()
        Log.i(TAG, "begin tool auth tool=$tool")
        CoreJson.decodeToolExecutionResult(nativeCore.beginToolAuth(tool))
    }

    suspend fun disconnectTool(tool: String): ToolExecutionResult = withContext(dispatcher) {
        requireInitialized()
        Log.i(TAG, "disconnect tool=$tool")
        CoreJson.decodeToolExecutionResult(nativeCore.disconnectTool(tool))
    }

    suspend fun refreshToolState(tool: String): ToolExecutionResult = withContext(dispatcher) {
        requireInitialized()
        Log.i(TAG, "refresh tool tool=$tool")
        CoreJson.decodeToolExecutionResult(nativeCore.refreshToolState(tool))
    }

    suspend fun setPreviousContext(level: ContextLevel) {
        withContext(dispatcher) {
            requireInitialized()
            nativeCore.setPreviousContext(level.wireName)
        }
        eventsFlow.emit(CoreEvent.ConfigUpdated(level))
    }

    suspend fun setCloudProfile(profileId: String) {
        withContext(dispatcher) {
            requireInitialized()
            nativeCore.setCloudProfile(profileId)
        }
    }

    suspend fun submitAmbientEvent(
        turnId: String,
        eventType: String,
        payloadJson: String = "{}",
    ) {
        withContext(dispatcher) {
            requireInitialized()
            nativeCore.submitAmbientEvent(turnId, eventType, payloadJson)
        }
    }

    suspend fun recordNoteActivity(event: NoteActivityEvent) {
        withContext(dispatcher) {
            requireInitialized()
            nativeCore.recordNoteActivity(
                noteKey = event.noteKey,
                relativePath = event.relativePath,
                titleSnapshot = event.titleSnapshot,
                eventType = event.eventType,
                occurredAt = event.occurredAt,
            )
        }
    }

    override fun close() {
        dispatcher.close()
    }

    private fun requireInitialized() {
        check(isInitialized) {
            "Baby Gervaise core is not initialized."
        }
    }
}
