package io.gervaise.babygervaise

import android.annotation.SuppressLint
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewClientCompat
import io.gervaise.babygervaise.bridge.CoreEventForwarder
import io.gervaise.babygervaise.bridge.NativeBabyGervaise
import io.gervaise.babygervaise.bridge.WebAppBridge
import io.gervaise.babygervaise.bridge.dispatchCoreEvent

private const val WEB_UI_URL = "https://appassets.androidplatform.net/assets/ui/index.html"

class MainActivity : ComponentActivity() {
    private val nativeCore = NativeBabyGervaise()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.light(
                scrim = Color.TRANSPARENT,
                darkScrim = Color.TRANSPARENT,
            ),
            navigationBarStyle = SystemBarStyle.light(
                scrim = Color.TRANSPARENT,
                darkScrim = Color.TRANSPARENT,
            ),
        )
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
    val context = LocalContext.current
    val snackbarHostState = remember { SnackbarHostState() }
    val webViewHolder = remember { mutableStateOf<WebView?>(null) }
    val assetLoader = remember(context) {
        WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(context))
            .build()
    }

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
                    WebView.setWebContentsDebuggingEnabled(true)
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    settings.allowFileAccess = false
                    setBackgroundColor(Color.TRANSPARENT)
                    ViewCompat.setOnApplyWindowInsetsListener(this) { _, windowInsets -> windowInsets }
                    ViewCompat.requestApplyInsets(this)
                    webChromeClient = object : WebChromeClient() {
                        override fun onConsoleMessage(consoleMessage: android.webkit.ConsoleMessage): Boolean {
                            Log.d(
                                "BabyGervaiseWebView",
                                "${consoleMessage.messageLevel()}: ${consoleMessage.message()} @${consoleMessage.sourceId()}:${consoleMessage.lineNumber()}",
                            )
                            return super.onConsoleMessage(consoleMessage)
                        }
                    }
                    webViewClient = object : WebViewClientCompat() {
                        override fun shouldInterceptRequest(
                            view: WebView,
                            request: WebResourceRequest,
                        ): WebResourceResponse? = assetLoader.shouldInterceptRequest(request.url)
                    }
                    addJavascriptInterface(
                        WebAppBridge(
                            nativeCore = nativeCore,
                            emitToWeb = { eventType, payloadJson ->
                                post { dispatchCoreEvent(eventType, payloadJson) }
                            },
                        ),
                        "BabyGervaiseBridge",
                    )
                    loadUrl(WEB_UI_URL)
                }
            },
            update = { webView ->
                webViewHolder.value = webView
            },
        )
    }
}
