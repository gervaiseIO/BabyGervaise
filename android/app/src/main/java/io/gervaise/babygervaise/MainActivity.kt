package io.gervaise.babygervaise

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import io.gervaise.babygervaise.theme.BabyGervaiseTheme

class MainActivity : ComponentActivity() {
    private val viewModel: BabyGervaiseViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        handleSpotifyAuthIntent(intent)

        setContent {
            BabyGervaiseTheme {
                BabyGervaiseRoute(viewModel = viewModel)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleSpotifyAuthIntent(intent)
    }

    private fun handleSpotifyAuthIntent(intent: Intent?) {
        intent?.dataString
            ?.takeIf { url -> url.startsWith("babygervaise://spotify/callback") }
            ?.let(viewModel::handleSpotifyAuthRedirect)
    }
}
