package io.gervaise.babygervaise.overlay

import android.content.Context
import android.content.res.ColorStateList
import android.text.TextUtils
import android.util.AttributeSet
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import com.google.android.material.button.MaterialButton
import com.google.android.material.color.MaterialColors
import com.google.android.material.shape.MaterialShapeDrawable
import com.google.android.material.shape.ShapeAppearanceModel

data class GervaiseOverlayPanelState(
    val bubbleState: OverlayBubbleState,
    val statusText: String,
    val detailText: String,
    val isMuted: Boolean,
    val isListening: Boolean,
    val isReady: Boolean,
    val isThinking: Boolean,
)

class GervaiseOverlayPanelView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : LinearLayout(context, attrs) {
    interface Listener {
        fun onHeaderTapped()
        fun onMicTapped()
        fun onMuteTapped()
        fun onOpenAppTapped()
        fun onCloseTapped()
    }

    val dragHandle: View
        get() = headerRow

    private val headerRow = LinearLayout(context).apply {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(16), dp(16), dp(16), dp(8))
    }
    private val bubbleView = GervaiseBubbleView(context).apply {
        layoutParams = LayoutParams(dp(44), dp(44))
    }
    private val textColumn = LinearLayout(context).apply {
        orientation = VERTICAL
        setPadding(dp(12), 0, 0, 0)
        layoutParams = LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f)
    }
    private val titleView = TextView(context).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
        setTypeface(typeface, android.graphics.Typeface.BOLD)
        text = "Gervaise"
    }
    private val statusView = TextView(context).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        maxLines = 1
        ellipsize = TextUtils.TruncateAt.END
    }
    private val detailView = TextView(context).apply {
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        maxLines = 2
        ellipsize = TextUtils.TruncateAt.END
        setPadding(dp(16), 0, dp(16), dp(12))
    }
    private val buttonRow = LinearLayout(context).apply {
        orientation = HORIZONTAL
        gravity = Gravity.CENTER
        setPadding(dp(12), 0, dp(12), dp(12))
    }
    private val micButton = createButton()
    private val muteButton = createButton()
    private val openAppButton = createButton()
    private val closeButton = createButton()

    init {
        orientation = VERTICAL
        minimumWidth = dp(264)
        background = createBackground()
        clipToPadding = false
        clipChildren = false
        ViewCompat.setElevation(this, dp(12).toFloat())

        textColumn.addView(titleView)
        textColumn.addView(statusView)
        headerRow.addView(bubbleView)
        headerRow.addView(textColumn)
        addView(headerRow, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
        addView(detailView, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))

        buttonRow.addView(micButton, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        buttonRow.addView(muteButton, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        buttonRow.addView(openAppButton, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        buttonRow.addView(closeButton, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        addView(buttonRow, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
    }

    fun setListener(listener: Listener) {
        headerRow.setOnClickListener { listener.onHeaderTapped() }
        micButton.setOnClickListener { listener.onMicTapped() }
        muteButton.setOnClickListener { listener.onMuteTapped() }
        openAppButton.setOnClickListener { listener.onOpenAppTapped() }
        closeButton.setOnClickListener { listener.onCloseTapped() }
    }

    fun render(state: GervaiseOverlayPanelState) {
        bubbleView.state = state.bubbleState
        statusView.text = state.statusText
        detailView.text = state.detailText
        micButton.text = if (state.isListening) "Stop" else "Mic"
        micButton.isEnabled = state.isListening || (state.isReady && !state.isThinking)
        muteButton.text = if (state.isMuted) "Unmute" else "Mute"
        openAppButton.text = "Open"
        closeButton.text = "Close"
    }

    private fun createButton(): MaterialButton =
        MaterialButton(context).apply {
            insetTop = 0
            insetBottom = 0
            minHeight = dp(40)
            cornerRadius = dp(20)
            strokeWidth = dp(1)
            setPadding(dp(8), dp(10), dp(8), dp(10))
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            val outline = MaterialColors.getColor(
                this,
                com.google.android.material.R.attr.colorOutline,
                0xFFAAAAAA.toInt(),
            )
            val surface = MaterialColors.getColor(
                this,
                com.google.android.material.R.attr.colorSurface,
                0xFFFFFFFF.toInt(),
            )
            setTextColor(
                MaterialColors.getColor(
                    this,
                    com.google.android.material.R.attr.colorOnSurface,
                    0xFF111111.toInt(),
                ),
            )
            backgroundTintList = ColorStateList.valueOf(surface)
            strokeColor = ColorStateList.valueOf(outline)
        }

    private fun createBackground(): MaterialShapeDrawable {
        val shape = ShapeAppearanceModel.builder()
            .setAllCornerSizes(dp(24).toFloat())
            .build()
        val surfaceColor = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorSurfaceContainerHigh,
            0xFFFFFFFF.toInt(),
        )
        val outline = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorOutlineVariant,
            surfaceColor,
        )
        return MaterialShapeDrawable(shape).apply {
            fillColor = ColorStateList.valueOf(surfaceColor)
            strokeColor = ColorStateList.valueOf(outline)
            strokeWidth = dp(1).toFloat()
        }
    }

    private fun dp(value: Int): Int =
        (value * resources.displayMetrics.density).toInt()
}
