package io.gervaise.babygervaise.overlay

import android.Manifest
import android.animation.ValueAnimator
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.PixelFormat
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import android.view.ContextThemeWrapper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.google.android.material.color.DynamicColors
import io.gervaise.babygervaise.BabyGervaiseApplication
import io.gervaise.babygervaise.BabyGervaiseCoreState
import io.gervaise.babygervaise.MainActivity
import io.gervaise.babygervaise.R
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

class GervaiseOverlayService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var runtimeState: BabyGervaiseCoreState
    private lateinit var windowManager: WindowManager
    private lateinit var preferences: OverlayPreferences
    private lateinit var speechHandler: AndroidSpeechRecognizerHandler
    private lateinit var textToSpeech: GervaiseTextToSpeechController
    private lateinit var themedContext: Context

    private var overlayRoot: FrameLayout? = null
    private var layoutParams: WindowManager.LayoutParams? = null
    private var bubbleView: GervaiseBubbleView? = null
    private var panelView: GervaiseOverlayPanelView? = null
    private var isExpanded = false
    private var isListening = false
    private var isSpeaking = false
    private var isMuted = false
    private var partialTranscript: String? = null
    private var latestResponseTurnId: String? = null
    private var latestAcknowledgedTurnId: String? = null
    private var snapAnimator: ValueAnimator? = null

    override fun onCreate() {
        super.onCreate()
        runtimeState = (application as BabyGervaiseApplication).runtime.state.value
        windowManager = getSystemService(Context.WINDOW_SERVICE) as WindowManager
        preferences = OverlayPreferences(this)
        isMuted = preferences.loadMuted()
        themedContext = DynamicColors.wrapContextIfAvailable(
            ContextThemeWrapper(this, R.style.Theme_BabyGervaise),
        )

        speechHandler = AndroidSpeechRecognizerHandler(themedContext, speechCallback)
        textToSpeech = GervaiseTextToSpeechController(themedContext, ttsCallback)

        scope.launch {
            (application as BabyGervaiseApplication).runtime.state.collectLatest { state ->
                val previousTurnId = latestResponseTurnId
                runtimeState = state
                latestResponseTurnId = state.latestAssistantTurnId
                if (state.latestAssistantTurnId != null && state.latestAssistantTurnId != previousTurnId) {
                    partialTranscript = null
                    if (isExpanded) {
                        acknowledgeLatestResponse()
                    }
                    maybeSpeakLatestResponse()
                }
                renderOverlay()
            }
        }
        _isRunning.value = true
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        if (!Settings.canDrawOverlays(this)) {
            stopSelf()
            return START_NOT_STICKY
        }

        startInForeground()
        showOverlay()
        renderOverlay()
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        stopSpeaking()
        speechHandler.cancel()
        speechHandler.destroy()
        textToSpeech.shutdown()
        snapAnimator?.cancel()
        removeOverlay()
        _isRunning.value = false
        scope.cancel()
        super.onDestroy()
    }

    private fun showOverlay() {
        if (overlayRoot != null) {
            return
        }

        val root = FrameLayout(themedContext).apply {
            clipChildren = false
            clipToPadding = false
        }
        overlayRoot = root
        layoutParams = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            val storedPosition = preferences.loadPosition()
            x = storedPosition?.x ?: defaultX()
            y = storedPosition?.y ?: defaultY()
        }
        windowManager.addView(root, layoutParams)
        renderOverlay()
    }

    private fun removeOverlay() {
        overlayRoot?.let { root ->
            if (root.isAttachedToWindow) {
                windowManager.removeViewImmediate(root)
            }
        }
        overlayRoot = null
        bubbleView = null
        panelView = null
    }

    private fun renderOverlay() {
        val root = overlayRoot ?: return
        val currentState = resolveBubbleState()
        if (isExpanded) {
            val panel = panelView ?: createPanelView().also { panelView = it }
            panel.render(
                GervaiseOverlayPanelState(
                    bubbleState = currentState,
                    statusText = runtimeState.statusText,
                    detailText = detailText(),
                    isMuted = isMuted,
                    isListening = isListening,
                    isReady = runtimeState.isCoreReady && speechHandler.isAvailable() &&
                        hasMicrophonePermission(),
                    isThinking = runtimeState.pendingTurnId != null,
                ),
            )
            swapOverlayChild(root, panel)
        } else {
            val bubble = bubbleView ?: createBubbleView().also { bubbleView = it }
            bubble.state = currentState
            swapOverlayChild(root, bubble)
        }

        root.post {
            constrainWithinScreen()
        }
    }

    private fun createBubbleView(): GervaiseBubbleView =
        GervaiseBubbleView(themedContext).apply {
            layoutParams = FrameLayout.LayoutParams(dp(60), dp(60))
            setOnTouchListener(createDragTouchListener(onTap = ::expandPanel))
        }

    private fun createPanelView(): GervaiseOverlayPanelView =
        GervaiseOverlayPanelView(themedContext).apply {
            setListener(
                object : GervaiseOverlayPanelView.Listener {
                    override fun onHeaderTapped() {
                        collapsePanel()
                    }

                    override fun onMicTapped() {
                        toggleListening()
                    }

                    override fun onMuteTapped() {
                        isMuted = !isMuted
                        preferences.saveMuted(isMuted)
                        if (isMuted) {
                            stopSpeaking()
                        } else {
                            maybeSpeakLatestResponse()
                        }
                        renderOverlay()
                    }

                    override fun onOpenAppTapped() {
                        acknowledgeLatestResponse()
                        stopSpeaking()
                        startActivity(
                            Intent(this@GervaiseOverlayService, MainActivity::class.java).apply {
                                addFlags(
                                    Intent.FLAG_ACTIVITY_NEW_TASK or
                                        Intent.FLAG_ACTIVITY_SINGLE_TOP or
                                        Intent.FLAG_ACTIVITY_CLEAR_TOP,
                                )
                            },
                        )
                    }

                    override fun onCloseTapped() {
                        stopSelf()
                    }
                },
            )
            dragHandle.setOnTouchListener(createDragTouchListener(onTap = ::collapsePanel))
        }

    private fun toggleListening() {
        stopSpeaking()
        acknowledgeLatestResponse()

        if (!hasMicrophonePermission()) {
            renderOverlay()
            return
        }

        if (isListening) {
            speechHandler.stopListening()
            return
        }

        if (runtimeState.pendingTurnId != null || !runtimeState.isCoreReady) {
            renderOverlay()
            return
        }

        partialTranscript = "Listening..."
        isListening = speechHandler.startListening()
        renderOverlay()
    }

    private fun maybeSpeakLatestResponse() {
        val latestMessage = runtimeState.latestAssistantMessage ?: return
        if (latestMessage.content.isBlank()) {
            return
        }
        if (isMuted) {
            return
        }
        textToSpeech.speak(latestMessage.content)
    }

    private fun stopSpeaking() {
        textToSpeech.stop()
    }

    private fun resolveBubbleState(): OverlayBubbleState =
        when {
            isListening -> OverlayBubbleState.LISTENING
            isSpeaking -> OverlayBubbleState.SPEAKING
            runtimeState.pendingTurnId != null -> OverlayBubbleState.THINKING
            latestResponseTurnId != null && latestResponseTurnId != latestAcknowledgedTurnId -> {
                OverlayBubbleState.SUGGESTION
            }

            else -> OverlayBubbleState.IDLE
        }

    private fun detailText(): String {
        val latestMessage = runtimeState.latestAssistantMessage?.content.orEmpty()
        return when {
            !hasMicrophonePermission() -> "Enable microphone access in the app to talk from the overlay."
            !speechHandler.isAvailable() -> "Speech services are unavailable on this device."
            isListening && !partialTranscript.isNullOrBlank() -> partialTranscript.orEmpty()
            runtimeState.pendingTurnId != null -> runtimeState.statusText
            latestMessage.isNotBlank() -> latestMessage
            else -> "Tap the bubble to talk to Gervaise."
        }
    }

    private fun acknowledgeLatestResponse() {
        latestAcknowledgedTurnId = latestResponseTurnId
        renderOverlay()
    }

    private fun expandPanel() {
        isExpanded = true
        stopSpeaking()
        acknowledgeLatestResponse()
        renderOverlay()
    }

    private fun collapsePanel() {
        isExpanded = false
        stopSpeaking()
        renderOverlay()
    }

    private fun swapOverlayChild(
        root: FrameLayout,
        child: View,
    ) {
        val currentParent = child.parent as? ViewGroup
        if (currentParent === root) {
            return
        }
        currentParent?.removeView(child)
        root.removeAllViews()
        root.addView(child)
    }

    private fun createDragTouchListener(onTap: () -> Unit): View.OnTouchListener {
        val touchSlop = ViewConfiguration.get(this).scaledTouchSlop
        return object : View.OnTouchListener {
            var startX = 0
            var startY = 0
            var downRawX = 0f
            var downRawY = 0f
            var dragging = false

            override fun onTouch(
                view: View,
                event: MotionEvent,
            ): Boolean {
                val params = layoutParams ?: return false
                when (event.actionMasked) {
                    MotionEvent.ACTION_DOWN -> {
                        stopSpeaking()
                        startX = params.x
                        startY = params.y
                        downRawX = event.rawX
                        downRawY = event.rawY
                        dragging = false
                        return true
                    }

                    MotionEvent.ACTION_MOVE -> {
                        val deltaX = (event.rawX - downRawX).roundToInt()
                        val deltaY = (event.rawY - downRawY).roundToInt()
                        if (!dragging && (abs(deltaX) > touchSlop || abs(deltaY) > touchSlop)) {
                            dragging = true
                        }
                        if (dragging) {
                            params.x = startX + deltaX
                            params.y = startY + deltaY
                            updateOverlayPosition()
                        }
                        return true
                    }

                    MotionEvent.ACTION_UP,
                    MotionEvent.ACTION_CANCEL,
                    -> {
                        if (dragging) {
                            snapToNearestEdge()
                        } else {
                            onTap()
                        }
                        return true
                    }
                }
                return false
            }
        }
    }

    private fun updateOverlayPosition() {
        val root = overlayRoot ?: return
        val params = layoutParams ?: return
        constrainWithinScreen()
        if (root.isAttachedToWindow) {
            windowManager.updateViewLayout(root, params)
        }
    }

    private fun constrainWithinScreen() {
        val root = overlayRoot ?: return
        val params = layoutParams ?: return
        val size = screenSize()
        val maxX = (size.first - root.measuredWidth).coerceAtLeast(0)
        val maxY = (size.second - root.measuredHeight).coerceAtLeast(0)
        params.x = params.x.coerceIn(0, maxX)
        params.y = params.y.coerceIn(0, maxY)
        preferences.savePosition(params.x, params.y)
        if (root.isAttachedToWindow) {
            windowManager.updateViewLayout(root, params)
        }
    }

    private fun snapToNearestEdge() {
        val params = layoutParams ?: return
        val root = overlayRoot ?: return
        val size = screenSize()
        val maxX = (size.first - root.width).coerceAtLeast(0)
        val targetX = if (params.x < maxX / 2) 0 else maxX
        animatePositionTo(targetX, params.y.coerceIn(0, (size.second - root.height).coerceAtLeast(0)))
    }

    private fun animatePositionTo(
        targetX: Int,
        targetY: Int,
    ) {
        val params = layoutParams ?: return
        val startX = params.x
        val startY = params.y
        snapAnimator?.cancel()
        snapAnimator = ValueAnimator.ofFloat(0f, 1f).apply {
            duration = 180L
            addUpdateListener { animator ->
                val fraction = animator.animatedFraction
                params.x = startX + ((targetX - startX) * fraction).roundToInt()
                params.y = startY + ((targetY - startY) * fraction).roundToInt()
                updateOverlayPosition()
            }
            start()
        }
    }

    private fun screenSize(): Pair<Int, Int> {
        val metrics = resources.displayMetrics
        return metrics.widthPixels to metrics.heightPixels
    }

    private fun hasMicrophonePermission(): Boolean =
        ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.RECORD_AUDIO,
        ) == PackageManager.PERMISSION_GRANTED

    private fun defaultX(): Int = screenSize().first - dp(76)

    private fun defaultY(): Int = screenSize().second / 3

    private fun dp(value: Int): Int =
        (value * resources.displayMetrics.density).roundToInt()

    private fun startInForeground() {
        createNotificationChannel()
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setContentTitle(getString(R.string.app_name))
            .setContentText("Floating overlay active")
            .setOngoing(true)
            .setSilent(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfoCompat.microphoneForegroundServiceType(),
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }
        val notificationManager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Gervaise overlay",
            NotificationManager.IMPORTANCE_LOW,
        )
        notificationManager.createNotificationChannel(channel)
    }

    private val speechCallback = object : AndroidSpeechRecognizerHandler.Callback {
        override fun onReadyForSpeech() {
            isListening = true
            partialTranscript = "Listening..."
            renderOverlay()
        }

        override fun onPartialTranscript(text: String) {
            partialTranscript = text
            renderOverlay()
        }

        override fun onFinalTranscript(text: String) {
            isListening = false
            partialTranscript = text
            (application as BabyGervaiseApplication).runtime.submitUserTurn(
                text = text,
                inputSource = io.gervaise.babygervaise.bridge.InputSource.VOICE,
            )
            renderOverlay()
        }

        override fun onListeningStopped() {
            isListening = false
            renderOverlay()
        }

        override fun onError(message: String) {
            isListening = false
            partialTranscript = message
            renderOverlay()
        }
    }

    private val ttsCallback = object : GervaiseTextToSpeechController.Callback {
        override fun onSpeakingStateChanged(isSpeaking: Boolean) {
            this@GervaiseOverlayService.isSpeaking = isSpeaking
            renderOverlay()
        }

        override fun onError(message: String) {
            partialTranscript = message
            renderOverlay()
        }
    }

    companion object {
        private const val CHANNEL_ID = "gervaise_overlay"
        private const val NOTIFICATION_ID = 1201
        private val _isRunning = MutableStateFlow(false)
        val isRunning = _isRunning.asStateFlow()

        fun show(context: Context) {
            val intent = Intent(context, GervaiseOverlayService::class.java)
            ContextCompat.startForegroundService(context, intent)
        }

        fun hide(context: Context) {
            context.stopService(Intent(context, GervaiseOverlayService::class.java))
        }
    }
}

private object ServiceInfoCompat {
    fun microphoneForegroundServiceType(): Int =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        } else {
            0
        }
}
