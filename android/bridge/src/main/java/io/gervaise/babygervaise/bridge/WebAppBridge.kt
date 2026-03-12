package io.gervaise.babygervaise.bridge

import android.webkit.JavascriptInterface
import org.json.JSONObject
import java.util.UUID

class WebAppBridge(
    private val nativeCore: NativeBabyGervaise,
    private val emitToWeb: (eventType: String, payloadJson: String) -> Unit,
) {
    @JavascriptInterface
    fun postMessage(payloadJson: String) {
        runCatching {
            val command = parseCommand(payloadJson)
            when (command.type) {
                "bootstrap" -> emitToWeb("bootstrap_state", nativeCore.loadBootstrapState())
                "request_overview" -> emitToWeb("overview_state", nativeCore.loadOverviewState())
                "set_context_level" -> {
                    nativeCore.setPreviousContext(command.payload.optString("level", "medium"))
                    emitToWeb("config_updated", JSONObject(mapOf("level" to command.payload.optString("level", "medium"))).toString())
                    emitToWeb("overview_state", nativeCore.loadOverviewState())
                }
                "send_message" -> {
                    val turnId = command.payload.optString("turnId", UUID.randomUUID().toString())
                    val text = command.payload.getString("text")
                    val inputSource = command.payload.optString("inputSource", "text")
                    nativeCore.submitUserTurn(turnId, text, inputSource)
                }
            }
        }.onFailure { error ->
            emitToWeb(
                "assistant_error",
                JSONObject(
                    mapOf(
                        "turnId" to JSONObject.NULL,
                        "error" to (error.message ?: "Unknown bridge failure"),
                    ),
                ).toString(),
            )
        }
    }

    internal fun parseCommand(payloadJson: String): WebCommand {
        val json = JSONObject(payloadJson)
        return WebCommand(
            type = json.getString("command"),
            payload = json.optJSONObject("payload") ?: JSONObject(),
        )
    }
}

internal data class WebCommand(
    val type: String,
    val payload: JSONObject,
)

