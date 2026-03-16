package io.gervaise.babygervaise.overlay

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View
import com.google.android.material.color.MaterialColors
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin

enum class OverlayBubbleState {
    IDLE,
    LISTENING,
    THINKING,
    SPEAKING,
    SUGGESTION,
}

class GervaiseBubbleView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {
    var state: OverlayBubbleState = OverlayBubbleState.IDLE
        set(value) {
            if (field == value) {
                return
            }
            field = value
            restartAnimator()
            invalidate()
        }

    private val bubblePaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val accentPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val linePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
    }
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        isFakeBoldText = true
    }
    private val animationAnimator = ValueAnimator.ofFloat(0f, 1f).apply {
        repeatCount = ValueAnimator.INFINITE
        addUpdateListener {
            invalidate()
        }
    }

    init {
        isClickable = true
        restartAnimator()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!animationAnimator.isStarted) {
            animationAnimator.start()
        }
    }

    override fun onDetachedFromWindow() {
        animationAnimator.cancel()
        super.onDetachedFromWindow()
    }

    override fun onMeasure(
        widthMeasureSpec: Int,
        heightMeasureSpec: Int,
    ) {
        val defaultSize = (56 * resources.displayMetrics.density).roundToInt()
        val measuredWidth = resolveSize(defaultSize, widthMeasureSpec)
        val measuredHeight = resolveSize(defaultSize, heightMeasureSpec)
        val size = min(measuredWidth, measuredHeight)
        setMeasuredDimension(size, size)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        val size = min(width, height).toFloat()
        val centerX = width / 2f
        val centerY = height / 2f
        val progress = animationAnimator.animatedFraction
        val pulse = sin(progress * 2f * PI).toFloat()
        val surface = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorSurfaceContainerHighest,
            0xFFDDDDDD.toInt(),
        )
        val surfaceAlt = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorSurfaceContainerHigh,
            surface,
        )
        val primary = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorPrimary,
            0xFF444444.toInt(),
        )
        val onPrimary = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorOnPrimary,
            0xFFFFFFFF.toInt(),
        )
        val outline = MaterialColors.getColor(
            this,
            com.google.android.material.R.attr.colorOutline,
            primary,
        )

        val baseRadius = size * 0.34f
        val animatedRadius = when (state) {
            OverlayBubbleState.IDLE -> baseRadius * (1f + 0.025f * pulse)
            OverlayBubbleState.LISTENING -> baseRadius * (1f + 0.08f * pulse)
            OverlayBubbleState.SUGGESTION -> baseRadius * (1f + 0.045f * pulse)
            else -> baseRadius
        }

        bubblePaint.color = when (state) {
            OverlayBubbleState.LISTENING -> primary
            OverlayBubbleState.SPEAKING -> primary
            OverlayBubbleState.THINKING -> surfaceAlt
            OverlayBubbleState.SUGGESTION -> surfaceAlt
            OverlayBubbleState.IDLE -> surface
        }
        accentPaint.color = primary
        accentPaint.style = Paint.Style.FILL
        linePaint.color = if (state == OverlayBubbleState.SPEAKING) onPrimary else primary
        linePaint.strokeWidth = size * 0.07f
        textPaint.color = if (state == OverlayBubbleState.LISTENING || state == OverlayBubbleState.SPEAKING) {
            onPrimary
        } else {
            primary
        }
        textPaint.textSize = size * 0.26f

        if (state == OverlayBubbleState.LISTENING) {
            val ringRadius = animatedRadius + size * 0.08f * (0.5f + 0.5f * pulse)
            bubblePaint.color = primary
            bubblePaint.alpha = (44 + abs(pulse) * 42).roundToInt()
            bubblePaint.style = Paint.Style.STROKE
            bubblePaint.strokeWidth = size * 0.06f
            canvas.drawCircle(centerX, centerY, ringRadius, bubblePaint)
            bubblePaint.style = Paint.Style.FILL
        }

        if (state == OverlayBubbleState.THINKING) {
            val orbitRadius = animatedRadius + size * 0.11f
            val angle = progress * 2f * PI
            bubblePaint.color = surface
            bubblePaint.alpha = 120
            bubblePaint.style = Paint.Style.STROKE
            bubblePaint.strokeWidth = size * 0.04f
            canvas.drawCircle(centerX, centerY, orbitRadius, bubblePaint)
            bubblePaint.style = Paint.Style.FILL
            canvas.drawCircle(
                centerX + cos(angle).toFloat() * orbitRadius,
                centerY + sin(angle).toFloat() * orbitRadius,
                size * 0.05f,
                accentPaint,
            )
        }

        bubblePaint.color = when (state) {
            OverlayBubbleState.LISTENING -> primary
            OverlayBubbleState.SPEAKING -> primary
            OverlayBubbleState.THINKING -> surfaceAlt
            OverlayBubbleState.SUGGESTION -> surfaceAlt
            OverlayBubbleState.IDLE -> surface
        }
        bubblePaint.alpha = 255
        bubblePaint.style = Paint.Style.FILL
        canvas.drawCircle(centerX, centerY, animatedRadius, bubblePaint)

        bubblePaint.color = outline
        bubblePaint.alpha = 72
        bubblePaint.style = Paint.Style.STROKE
        bubblePaint.strokeWidth = size * 0.025f
        canvas.drawCircle(centerX, centerY, animatedRadius, bubblePaint)
        bubblePaint.style = Paint.Style.FILL

        when (state) {
            OverlayBubbleState.SPEAKING -> drawWaveform(canvas, centerX, centerY, animatedRadius, progress)
            OverlayBubbleState.SUGGESTION -> {
                canvas.drawCircle(
                    centerX + animatedRadius * 0.72f,
                    centerY - animatedRadius * 0.72f,
                    size * 0.08f,
                    accentPaint,
                )
                drawLabel(canvas, centerX, centerY, size)
            }

            else -> drawLabel(canvas, centerX, centerY, size)
        }
    }

    private fun drawLabel(
        canvas: Canvas,
        centerX: Float,
        centerY: Float,
        size: Float,
    ) {
        val baseline = centerY - (textPaint.descent() + textPaint.ascent()) / 2f
        canvas.drawText("G", centerX, baseline + size * 0.01f, textPaint)
    }

    private fun drawWaveform(
        canvas: Canvas,
        centerX: Float,
        centerY: Float,
        radius: Float,
        progress: Float,
    ) {
        val barSpacing = radius * 0.26f
        val barWidth = radius * 0.1f
        val amplitudes = listOf(0.38f, 0.62f, 0.86f, 0.54f)
        amplitudes.forEachIndexed { index, baseAmplitude ->
            val phase = progress * 2f * PI + index * 0.65f
            val height = radius * (0.42f + 0.42f * abs(sin(phase).toFloat()) * baseAmplitude)
            val x = centerX + (index - 1.5f) * barSpacing
            canvas.drawLine(
                x,
                centerY - height / 2f,
                x,
                centerY + height / 2f,
                linePaint.apply { strokeWidth = barWidth },
            )
        }
    }

    private fun restartAnimator() {
        animationAnimator.cancel()
        animationAnimator.duration = when (state) {
            OverlayBubbleState.IDLE -> 2600L
            OverlayBubbleState.LISTENING -> 850L
            OverlayBubbleState.THINKING -> 1400L
            OverlayBubbleState.SPEAKING -> 650L
            OverlayBubbleState.SUGGESTION -> 1050L
        }
        if (isAttachedToWindow) {
            animationAnimator.start()
        }
    }
}
