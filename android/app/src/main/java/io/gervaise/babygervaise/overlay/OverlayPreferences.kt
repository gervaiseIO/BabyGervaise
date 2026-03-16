package io.gervaise.babygervaise.overlay

import android.content.Context

data class OverlayPosition(
    val x: Int,
    val y: Int,
)

class OverlayPreferences(context: Context) {
    private val preferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    fun loadPosition(): OverlayPosition? {
        if (!preferences.contains(KEY_X) || !preferences.contains(KEY_Y)) {
            return null
        }
        return OverlayPosition(
            x = preferences.getInt(KEY_X, 0),
            y = preferences.getInt(KEY_Y, 0),
        )
    }

    fun savePosition(
        x: Int,
        y: Int,
    ) {
        preferences.edit()
            .putInt(KEY_X, x)
            .putInt(KEY_Y, y)
            .apply()
    }

    fun loadMuted(): Boolean = preferences.getBoolean(KEY_MUTED, false)

    fun saveMuted(isMuted: Boolean) {
        preferences.edit()
            .putBoolean(KEY_MUTED, isMuted)
            .apply()
    }

    private companion object {
        const val PREFS_NAME = "gervaise_overlay"
        const val KEY_X = "overlay_x"
        const val KEY_Y = "overlay_y"
        const val KEY_MUTED = "overlay_muted"
    }
}
