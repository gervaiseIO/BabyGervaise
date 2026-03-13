package io.gervaise.babygervaise.bridge

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
) : Closeable {
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
                nativeCore.init(
                    appFilesDir = appFilesDir,
                    assetConfigDir = assetConfigDir,
                    callbacks = callbacks,
                )
            }
            isInitialized = true
        }
    }

    suspend fun loadBootstrapState(): BootstrapState = withContext(dispatcher) {
        CoreJson.decodeBootstrapState(nativeCore.loadBootstrapState())
    }

    suspend fun loadOverviewSnapshot(): OverviewSnapshot = withContext(dispatcher) {
        CoreJson.decodeOverviewSnapshot(nativeCore.loadOverviewState())
    }

    suspend fun submitUserTurn(
        turnId: String,
        text: String,
        inputSource: InputSource = InputSource.TEXT,
    ) {
        withContext(dispatcher) {
            nativeCore.submitUserTurn(
                turnId = turnId,
                text = text,
                inputSource = inputSource.name.lowercase(),
            )
        }
    }

    suspend fun setPreviousContext(level: ContextLevel) {
        withContext(dispatcher) {
            nativeCore.setPreviousContext(level.wireName)
        }
        eventsFlow.emit(CoreEvent.ConfigUpdated(level))
    }

    override fun close() {
        dispatcher.close()
    }
}
