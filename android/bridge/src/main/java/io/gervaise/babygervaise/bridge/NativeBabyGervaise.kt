package io.gervaise.babygervaise.bridge

fun interface CoreCallbackChannel {
    fun onCoreEvent(eventType: String, payloadJson: String)
}

class NativeBabyGervaise {
    companion object {
        private val loaded = runCatching { System.loadLibrary("baby_gervaise_core") }.isSuccess
    }

    private external fun nativeInit(
        appFilesDir: String,
        assetConfigDir: String,
        callbacks: CoreCallbackChannel,
        nanoHost: NanoHost,
    )

    private external fun nativeSubmitUserTurn(
        turnId: String,
        text: String,
        inputSource: String,
    )

    private external fun nativeHandleSpotifyAuthCallback(
        turnId: String,
        callbackUrl: String,
    )

    private external fun nativeHandleToolAuthCallback(
        tool: String,
        turnId: String,
        callbackUrl: String,
    )

    private external fun nativeExecuteToolAction(
        tool: String,
        action: String,
        argumentsJson: String,
    ): String

    private external fun nativeBeginToolAuth(tool: String): String

    private external fun nativeDisconnectTool(tool: String): String

    private external fun nativeRefreshToolState(tool: String): String

    private external fun nativeLoadBootstrapState(): String

    private external fun nativeLoadOverviewState(): String

    private external fun nativeSetPreviousContext(level: String)

    private external fun nativeSetCloudProfile(profileId: String)

    private external fun nativeSubmitAmbientEvent(
        turnId: String,
        eventType: String,
        payloadJson: String,
    )

    private external fun nativeRecordNoteActivity(
        noteKey: String,
        relativePath: String,
        titleSnapshot: String,
        eventType: String,
        occurredAt: String,
    )

    fun init(
        appFilesDir: String,
        assetConfigDir: String,
        callbacks: CoreCallbackChannel,
        nanoHost: NanoHost,
    ) {
        requireLoaded()
        nativeInit(appFilesDir, assetConfigDir, callbacks, nanoHost)
    }

    fun submitUserTurn(
        turnId: String,
        text: String,
        inputSource: String = "text",
    ) {
        requireLoaded()
        nativeSubmitUserTurn(turnId, text, inputSource)
    }

    fun handleSpotifyAuthCallback(
        turnId: String,
        callbackUrl: String,
    ) {
        requireLoaded()
        nativeHandleSpotifyAuthCallback(turnId, callbackUrl)
    }

    fun handleToolAuthCallback(
        tool: String,
        turnId: String,
        callbackUrl: String,
    ) {
        requireLoaded()
        nativeHandleToolAuthCallback(tool, turnId, callbackUrl)
    }

    fun executeToolAction(
        tool: String,
        action: String,
        argumentsJson: String = "{}",
    ): String {
        requireLoaded()
        return nativeExecuteToolAction(tool, action, argumentsJson)
    }

    fun beginToolAuth(tool: String): String {
        requireLoaded()
        return nativeBeginToolAuth(tool)
    }

    fun disconnectTool(tool: String): String {
        requireLoaded()
        return nativeDisconnectTool(tool)
    }

    fun refreshToolState(tool: String): String {
        requireLoaded()
        return nativeRefreshToolState(tool)
    }

    fun loadBootstrapState(): String {
        requireLoaded()
        return nativeLoadBootstrapState()
    }

    fun loadOverviewState(): String {
        requireLoaded()
        return nativeLoadOverviewState()
    }

    fun setPreviousContext(level: String) {
        requireLoaded()
        nativeSetPreviousContext(level)
    }

    fun setCloudProfile(profileId: String) {
        requireLoaded()
        nativeSetCloudProfile(profileId)
    }

    fun submitAmbientEvent(
        turnId: String,
        eventType: String,
        payloadJson: String = "{}",
    ) {
        requireLoaded()
        nativeSubmitAmbientEvent(turnId, eventType, payloadJson)
    }

    fun recordNoteActivity(
        noteKey: String,
        relativePath: String,
        titleSnapshot: String,
        eventType: String,
        occurredAt: String,
    ) {
        requireLoaded()
        nativeRecordNoteActivity(
            noteKey = noteKey,
            relativePath = relativePath,
            titleSnapshot = titleSnapshot,
            eventType = eventType,
            occurredAt = occurredAt,
        )
    }

    private fun requireLoaded() {
        check(loaded) {
            "Rust library baby_gervaise_core is not loaded. Build rust_core before launching the Android app."
        }
    }
}
