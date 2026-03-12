package io.gervaise.babygervaise.bridge

import android.webkit.WebView
import org.json.JSONObject

class CoreEventForwarder(
    private val forward: (eventType: String, payloadJson: String) -> Unit,
) : CoreCallbackChannel {
    override fun onCoreEvent(eventType: String, payloadJson: String) {
        forward(eventType, payloadJson)
    }
}

fun WebView.dispatchCoreEvent(eventType: String, payloadJson: String) {
    val script = """
        window.dispatchEvent(new CustomEvent("baby-gervaise-event", {
          detail: {
            type: ${JSONObject.quote(eventType)},
            payload: JSON.parse(${JSONObject.quote(payloadJson)})
          }
        }));
    """.trimIndent()
    evaluateJavascript(script, null)
}

