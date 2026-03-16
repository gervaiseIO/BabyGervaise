package io.gervaise.babygervaise

import android.util.Log
import com.google.mlkit.genai.common.DownloadStatus
import com.google.mlkit.genai.common.FeatureStatus
import com.google.mlkit.genai.prompt.GenerateContentRequest
import com.google.mlkit.genai.prompt.Generation
import com.google.mlkit.genai.prompt.GenerativeModel
import com.google.mlkit.genai.prompt.TextPart
import io.gervaise.babygervaise.bridge.CoreJson
import io.gervaise.babygervaise.bridge.NanoHost
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

class MlKitNanoHost : NanoHost {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val model: GenerativeModel = Generation.getClient()

    @Volatile
    private var snapshot = NanoSnapshot(
        availability = "unavailable",
        detail = "Checking Gemini Nano availability.",
    )

    init {
        scope.launch {
            refreshSnapshot(startDownload = true)
        }
    }

    override fun loadNanoSnapshot(): String {
        if (snapshot.availability != "available") {
            runBlocking {
                refreshSnapshot(startDownload = false)
            }
        }
        return snapshot.toJson()
    }

    override fun runNanoPrompt(requestJson: String): String {
        val startedAt = System.currentTimeMillis()
        val request = runCatching { parseRequest(requestJson) }.getOrElse { error ->
            Log.e(TAG, "Failed to parse Nano prompt request.", error)
            return buildNanoErrorPayload(
                prompt = "",
                startedAt = startedAt,
                error = error,
                requestedMaxOutputTokens = null,
                effectiveMaxOutputTokens = null,
            )
        }
        val effectiveMaxOutputTokens = normalizeNanoMaxOutputTokens(
            requested = request.maxOutputTokens,
            mode = request.mode,
        )
        Log.i(
            TAG,
            "Nano prompt mode=${request.mode} requestedMaxOutputTokens=${request.maxOutputTokens} effectiveMaxOutputTokens=$effectiveMaxOutputTokens",
        )
        return runBlocking {
            runCatching {
                ensureAvailable()
                val promptRequest = GenerateContentRequest.Builder(TextPart(request.prompt)).apply {
                    request.temperature?.let { temperature = it }
                    maxOutputTokens = effectiveMaxOutputTokens
                }.build()
                val response = model.generateContent(promptRequest)
                buildNanoSuccessPayload(
                    text = response.candidates.firstOrNull()?.text.orEmpty(),
                    prompt = request.prompt,
                    startedAt = startedAt,
                    requestedMaxOutputTokens = request.maxOutputTokens,
                    effectiveMaxOutputTokens = effectiveMaxOutputTokens,
                )
            }.getOrElse { error ->
                Log.e(TAG, "Nano prompt failed for mode=${request.mode}.", error)
                buildNanoErrorPayload(
                    prompt = request.prompt,
                    startedAt = startedAt,
                    error = error,
                    requestedMaxOutputTokens = request.maxOutputTokens,
                    effectiveMaxOutputTokens = effectiveMaxOutputTokens,
                )
            }
        }
    }

    private suspend fun ensureAvailable() {
        when (refreshSnapshot(startDownload = true)) {
            FeatureStatus.AVAILABLE -> return
            FeatureStatus.DOWNLOADING,
            FeatureStatus.DOWNLOADABLE -> error("Gemini Nano is still downloading.")
            FeatureStatus.UNAVAILABLE -> error("Gemini Nano is unavailable on this device.")
            else -> error("Gemini Nano is not ready.")
        }
    }

    private suspend fun refreshSnapshot(startDownload: Boolean): Int {
        val status = runCatching { model.checkStatus() }.getOrElse { error ->
            snapshot = snapshot.copy(
                availability = "error",
                detail = error.message ?: "Gemini Nano status check failed.",
            )
            return FeatureStatus.UNAVAILABLE
        }

        snapshot = snapshotForStatus(status)
        if (status == FeatureStatus.DOWNLOADABLE && startDownload) {
            snapshot = snapshot.copy(
                availability = "downloading",
                detail = "Downloading Gemini Nano.",
            )
            model.download().collect { downloadStatus ->
                snapshot = when (downloadStatus) {
                    is DownloadStatus.DownloadStarted -> snapshot.copy(
                        availability = "downloading",
                        detail = "Downloading Gemini Nano (${downloadStatus.bytesToDownload} bytes).",
                    )

                    is DownloadStatus.DownloadProgress -> snapshot.copy(
                        availability = "downloading",
                        detail = "Downloading Gemini Nano (${downloadStatus.totalBytesDownloaded} bytes).",
                    )

                    is DownloadStatus.DownloadCompleted -> snapshot.copy(
                        availability = "available",
                        detail = "Gemini Nano is ready.",
                    )

                    is DownloadStatus.DownloadFailed -> snapshot.copy(
                        availability = "error",
                        detail = downloadStatus.e.message ?: "Gemini Nano download failed.",
                    )

                    else -> snapshot
                }
            }
            val refreshed = model.checkStatus()
            snapshot = snapshotForStatus(refreshed)
            return refreshed
        }
        return status
    }

    private fun snapshotForStatus(status: Int): NanoSnapshot =
        when (status) {
            FeatureStatus.AVAILABLE -> NanoSnapshot(
                availability = "available",
                detail = "Gemini Nano is ready.",
            )

            FeatureStatus.DOWNLOADABLE -> NanoSnapshot(
                availability = "downloading",
                detail = "Gemini Nano can be downloaded for this device.",
            )

            FeatureStatus.DOWNLOADING -> NanoSnapshot(
                availability = "downloading",
                detail = "Gemini Nano is downloading.",
            )

            else -> NanoSnapshot(
                availability = "unavailable",
                detail = "Gemini Nano is unavailable on this device.",
            )
        }

    private fun parseRequest(requestJson: String): NanoPromptRequest {
        val json = CoreJson.json.parseToJsonElement(requestJson).jsonObject
        return NanoPromptRequest(
            mode = json["mode"]?.jsonPrimitive?.contentOrNull ?: NanoPromptModeWire.FULL_REPLY,
            prompt = json.getValue("prompt").jsonPrimitive.content,
            temperature = json["temperature"]?.jsonPrimitive?.floatOrNull,
            maxOutputTokens = json["max_output_tokens"]?.jsonPrimitive?.intOrNull,
        )
    }

    private fun buildNanoSuccessPayload(
        text: String,
        prompt: String,
        startedAt: Long,
        requestedMaxOutputTokens: Int?,
        effectiveMaxOutputTokens: Int,
    ): String = buildJsonObject {
        put("text", JsonPrimitive(text))
        put("snapshot", snapshot.asJson())
        put("prompt", JsonPrimitive(prompt))
        put("latency_ms", JsonPrimitive(System.currentTimeMillis() - startedAt))
        putNullableInt("requested_max_output_tokens", requestedMaxOutputTokens)
        put("effective_max_output_tokens", JsonPrimitive(effectiveMaxOutputTokens))
        put("error_text", JsonNull)
    }.toString()

    private fun buildNanoErrorPayload(
        prompt: String,
        startedAt: Long,
        error: Throwable,
        requestedMaxOutputTokens: Int?,
        effectiveMaxOutputTokens: Int?,
    ): String = buildJsonObject {
        put("text", JsonPrimitive(""))
        put("snapshot", snapshot.asJson())
        put("prompt", JsonPrimitive(prompt))
        put("latency_ms", JsonPrimitive(System.currentTimeMillis() - startedAt))
        putNullableInt("requested_max_output_tokens", requestedMaxOutputTokens)
        putNullableInt("effective_max_output_tokens", effectiveMaxOutputTokens)
        put("error_text", JsonPrimitive(error.message ?: "Gemini Nano request failed."))
    }.toString()
}

private data class NanoPromptRequest(
    val mode: String,
    val prompt: String,
    val temperature: Float? = null,
    val maxOutputTokens: Int? = null,
)

private data class NanoSnapshot(
    val availability: String,
    val detail: String,
    val provider: String = "gemini",
    val model: String = "gemini-nano",
) {
    fun asJson() = buildJsonObject {
        put("availability", JsonPrimitive(availability))
        put("detail", JsonPrimitive(detail))
        put("provider", JsonPrimitive(provider))
        put("model", JsonPrimitive(model))
    }

    fun toJson(): String = asJson().toString()
}

internal object NanoPromptModeWire {
    const val FIRST_BEAT = "first_beat"
    const val FULL_REPLY = "full_reply"
    const val AMBIENT = "ambient"
}

internal fun normalizeNanoMaxOutputTokens(
    requested: Int?,
    mode: String,
): Int {
    val fallback = when (mode) {
        NanoPromptModeWire.FIRST_BEAT -> 48
        NanoPromptModeWire.AMBIENT -> 40
        else -> 192
    }
    return when {
        requested == null -> fallback
        requested in 1..256 -> requested
        requested > 256 -> 256
        else -> fallback
    }
}

private const val TAG = "MlKitNanoHost"

private fun kotlinx.serialization.json.JsonObjectBuilder.putNullableInt(
    key: String,
    value: Int?,
) {
    put(key, value?.let(::JsonPrimitive) ?: JsonNull)
}
