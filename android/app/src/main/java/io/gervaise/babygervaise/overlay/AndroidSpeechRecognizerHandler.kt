package io.gervaise.babygervaise.overlay

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import java.util.Locale

class AndroidSpeechRecognizerHandler(
    private val context: Context,
    private val callback: Callback,
) : RecognitionListener {
    interface Callback {
        fun onReadyForSpeech()
        fun onPartialTranscript(text: String)
        fun onFinalTranscript(text: String)
        fun onListeningStopped()
        fun onError(message: String)
    }

    private var speechRecognizer: SpeechRecognizer? = null
    private var isListening = false

    init {
        if (SpeechRecognizer.isRecognitionAvailable(context)) {
            speechRecognizer = SpeechRecognizer.createSpeechRecognizer(context).also {
                it.setRecognitionListener(this)
            }
        }
    }

    fun isAvailable(): Boolean = speechRecognizer != null

    fun startListening(): Boolean {
        val recognizer = speechRecognizer ?: return false
        if (isListening) {
            return true
        }
        recognizer.startListening(buildRecognizerIntent())
        isListening = true
        return true
    }

    fun stopListening() {
        if (!isListening) {
            return
        }
        speechRecognizer?.stopListening()
    }

    fun cancel() {
        if (!isListening) {
            return
        }
        isListening = false
        speechRecognizer?.cancel()
        callback.onListeningStopped()
    }

    fun destroy() {
        speechRecognizer?.destroy()
        speechRecognizer = null
        isListening = false
    }

    override fun onReadyForSpeech(params: Bundle?) {
        callback.onReadyForSpeech()
    }

    override fun onBeginningOfSpeech() = Unit

    override fun onRmsChanged(rmsdB: Float) = Unit

    override fun onBufferReceived(buffer: ByteArray?) = Unit

    override fun onEndOfSpeech() = Unit

    override fun onError(error: Int) {
        isListening = false
        callback.onListeningStopped()
        if (error == SpeechRecognizer.ERROR_CLIENT) {
            return
        }
        callback.onError(errorMessage(error))
    }

    override fun onResults(results: Bundle?) {
        isListening = false
        callback.onListeningStopped()
        val transcript = results
            ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
            ?.firstOrNull()
            ?.trim()
            .orEmpty()
        if (transcript.isBlank()) {
            callback.onError("I didn't catch that.")
            return
        }
        callback.onFinalTranscript(transcript)
    }

    override fun onPartialResults(partialResults: Bundle?) {
        val transcript = partialResults
            ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
            ?.firstOrNull()
            ?.trim()
            .orEmpty()
        if (transcript.isNotBlank()) {
            callback.onPartialTranscript(transcript)
        }
    }

    override fun onEvent(
        eventType: Int,
        params: Bundle?,
    ) = Unit

    private fun buildRecognizerIntent(): Intent =
        Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(
                RecognizerIntent.EXTRA_LANGUAGE_MODEL,
                RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
            )
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.getDefault().toLanguageTag())
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, false)
        }

    private fun errorMessage(code: Int): String =
        when (code) {
            SpeechRecognizer.ERROR_AUDIO -> "Audio capture failed."
            SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> "Microphone permission is missing."
            SpeechRecognizer.ERROR_NETWORK -> "Speech service network error."
            SpeechRecognizer.ERROR_NETWORK_TIMEOUT -> "Speech service timed out."
            SpeechRecognizer.ERROR_NO_MATCH -> "I didn't catch that."
            SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> "Speech recognition is busy."
            SpeechRecognizer.ERROR_SERVER -> "Speech service failed."
            SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> "No speech detected."
            else -> "Speech recognition is unavailable right now."
        }
}
