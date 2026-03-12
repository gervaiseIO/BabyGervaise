package io.gervaise.babygervaise

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import io.gervaise.babygervaise.bridge.CoreEventForwarder
import io.gervaise.babygervaise.bridge.NativeBabyGervaise
import io.gervaise.babygervaise.bridge.WebAppBridge
import io.gervaise.babygervaise.bridge.dispatchCoreEvent

class MainActivity : ComponentActivity() {
    private val nativeCore = NativeBabyGervaise()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val configDir = AssetConfigInstaller(this).install()

        setContent {
            MaterialTheme {
                BabyGervaiseHost(
                    nativeCore = nativeCore,
                    appFilesDir = filesDir.absolutePath,
                    configDir = configDir.absolutePath,
                )
            }
        }
    }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun BabyGervaiseHost(
    nativeCore: NativeBabyGervaise,
    appFilesDir: String,
    configDir: String,
) {
    val snackbarHostState = remember { SnackbarHostState() }
    val webViewHolder = remember { mutableStateOf<WebView?>(null) }

    LaunchedEffect(nativeCore, appFilesDir, configDir) {
        runCatching {
            nativeCore.init(
                appFilesDir = appFilesDir,
                assetConfigDir = configDir,
                callbacks = CoreEventForwarder { eventType, payloadJson ->
                    webViewHolder.value?.post {
                        webViewHolder.value?.dispatchCoreEvent(eventType, payloadJson)
                    }
                },
            )
        }.onFailure { error ->
            snackbarHostState.showSnackbar(error.message ?: "Failed to initialize Baby Gervaise core.")
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
    ) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                WebView(context).apply {
                    webViewHolder.value = this
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    settings.allowFileAccess = true
                    webChromeClient = WebChromeClient()
                    webViewClient = WebViewClient()
                    addJavascriptInterface(
                        WebAppBridge(
                            nativeCore = nativeCore,
                            emitToWeb = { eventType, payloadJson ->
                                post { dispatchCoreEvent(eventType, payloadJson) }
                            },
                        ),
                        "BabyGervaiseBridge",
                    )
                    loadUrl("file:///android_asset/ui/index.html")
                }
            },
            update = { webView ->
                webViewHolder.value = webView
            },
        )
    }
}
