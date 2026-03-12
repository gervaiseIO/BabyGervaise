package io.gervaise.babygervaise

import android.content.Context
import java.io.File

class AssetConfigInstaller(private val context: Context) {
    fun install(): File {
        val outputDir = File(context.filesDir, "runtime-config").apply { mkdirs() }
        val availableAssets = context.assets.list("")?.toSet().orEmpty()
        listOf(
            "model_config.json",
            "model_config.local.json",
            "prompt_config.json",
            "prompt_config.local.json",
            "app_config.json",
            "app_config.local.json",
        )
            .filter { fileName -> fileName in availableAssets }
            .forEach { fileName ->
                context.assets.open(fileName).use { input ->
                    File(outputDir, fileName).outputStream().use { output ->
                        input.copyTo(output)
                    }
                }
            }
        return outputDir
    }
}
