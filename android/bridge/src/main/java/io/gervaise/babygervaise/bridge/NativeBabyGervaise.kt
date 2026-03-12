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
    )

    private external fun nativeSubmitUserTurn(
        turnId: String,
        text: String,
        inputSource: String,
    )

    private external fun nativeLoadBootstrapState(): String

    private external fun nativeLoadOverviewState(): String

    private external fun nativeSetPreviousContext(level: String)

    fun init(
        appFilesDir: String,
        assetConfigDir: String,
        callbacks: CoreCallbackChannel,
    ) {
        requireLoaded()
        nativeInit(appFilesDir, assetConfigDir, callbacks)
    }

    fun submitUserTurn(
        turnId: String,
        text: String,
        inputSource: String = "text",
    ) {
        requireLoaded()
        nativeSubmitUserTurn(turnId, text, inputSource)
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

    private fun requireLoaded() {
        check(loaded) {
            "Rust library baby_gervaise_core is not loaded. Build rust_core before launching the Android app."
        }
    }
}

