package io.gervaise.babygervaise.overlay

import android.content.Context
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import java.util.Locale
import java.util.UUID

class GervaiseTextToSpeechController(
    context: Context,
    private val callback: Callback,
) : TextToSpeech.OnInitListener {
    interface Callback {
        fun onSpeakingStateChanged(isSpeaking: Boolean)
        fun onError(message: String)
    }

    private val applicationContext = context.applicationContext
    private var textToSpeech: TextToSpeech? = TextToSpeech(applicationContext, this)
    private var pendingText: String? = null
    private var isReady = false

    override fun onInit(status: Int) {
        if (status != TextToSpeech.SUCCESS) {
            callback.onError("Text to speech is unavailable.")
            return
        }

        val engine = textToSpeech ?: return
        isReady = true
        configureVoice(engine)
        engine.setOnUtteranceProgressListener(
            object : UtteranceProgressListener() {
                override fun onStart(utteranceId: String?) {
                    callback.onSpeakingStateChanged(true)
                }

                override fun onDone(utteranceId: String?) {
                    callback.onSpeakingStateChanged(false)
                }

                @Deprecated("Deprecated in Java")
                override fun onError(utteranceId: String?) {
                    callback.onSpeakingStateChanged(false)
                }

                override fun onError(
                    utteranceId: String?,
                    errorCode: Int,
                ) {
                    callback.onSpeakingStateChanged(false)
                }
            },
        )

        pendingText?.let {
            pendingText = null
            speak(it)
        }
    }

    fun speak(text: String) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) {
            return
        }

        val engine = textToSpeech ?: return
        if (!isReady) {
            pendingText = trimmed
            return
        }

        engine.speak(
            trimmed,
            TextToSpeech.QUEUE_FLUSH,
            null,
            UUID.randomUUID().toString(),
        )
    }

    fun stop() {
        pendingText = null
        textToSpeech?.stop()
    }

    fun shutdown() {
        pendingText = null
        textToSpeech?.stop()
        textToSpeech?.shutdown()
        textToSpeech = null
    }

    private fun configureVoice(engine: TextToSpeech) {
        val locale = Locale.getDefault()
        val availability = engine.setLanguage(locale)
        if (availability == TextToSpeech.LANG_MISSING_DATA || availability == TextToSpeech.LANG_NOT_SUPPORTED) {
            callback.onError("The current system voice is not available.")
            return
        }

        val currentVoice = engine.voice
        if (currentVoice?.locale?.language == locale.language) {
            return
        }

        bestVoiceForLocale(engine, locale)?.let { voice ->
            engine.voice = voice
        }
    }

    private fun bestVoiceForLocale(
        engine: TextToSpeech,
        locale: Locale,
    ): Voice? = engine.voices
        ?.filter { voice ->
            voice.locale.language == locale.language &&
                !voice.isNetworkConnectionRequired
        }
        ?.sortedWith(compareByDescending<Voice> { it.quality }.thenBy { it.latency })
        ?.firstOrNull()
}
