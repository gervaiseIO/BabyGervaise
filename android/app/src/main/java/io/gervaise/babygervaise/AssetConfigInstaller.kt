package io.gervaise.babygervaise

import android.content.Context
import java.io.File

class AssetConfigInstaller(private val context: Context) {
    fun install(): File {
        val outputDir = File(context.filesDir, "runtime-config").apply { mkdirs() }
        listOf("model_config.json", "prompt_config.json", "app_config.json").forEach { fileName ->
            context.assets.open(fileName).use { input ->
                File(outputDir, fileName).outputStream().use { output ->
                    input.copyTo(output)
                }
            }
        }
        return outputDir
    }
}

