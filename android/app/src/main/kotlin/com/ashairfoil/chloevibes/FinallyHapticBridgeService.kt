package com.ashairfoil.chloevibes

import android.app.Service
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.Message
import android.os.Messenger
import android.os.RemoteException
import android.os.SystemClock

/** Bound-only, signature-protected bridge for Finally's direct build. */
class FinallyHapticBridgeService : Service() {
    private lateinit var app: ChloeVibesApplication
    private lateinit var bridgeThread: HandlerThread
    private lateinit var bridgeHandler: Handler
    private lateinit var incomingMessenger: Messenger
    private val controller = CompanionSessionController()

    @Volatile
    private var activeSessionId = NO_SESSION

    private var activeClientUid = NO_UID

    private val outputTick = object : Runnable {
        override fun run() {
            val output = controller.poll(SystemClock.elapsedRealtime())
            if (output != null) {
                if (output.shouldRelease) {
                    app.releaseCompanionOutput(output.sessionId)
                    clearIdentity()
                } else if (!app.setCompanionPosition(output.sessionId, output.positionMilli)) {
                    forceRelease()
                }
            }
            bridgeHandler.postDelayed(this, CompanionSessionController.RAMP_INTERVAL_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        app = application as ChloeVibesApplication
        bridgeThread = HandlerThread("Finally-Haptic-Bridge").apply { start() }
        bridgeHandler = IncomingHandler(bridgeThread.looper)
        incomingMessenger = Messenger(bridgeHandler)
        bridgeHandler.post(outputTick)
    }

    override fun onBind(intent: Intent?): IBinder = incomingMessenger.binder

    override fun onUnbind(intent: Intent?): Boolean {
        bridgeHandler.post { forceRelease() }
        return false
    }

    override fun onDestroy() {
        val sessionId = activeSessionId
        activeSessionId = NO_SESSION
        if (sessionId != NO_SESSION) {
            app.releaseCompanionOutput(sessionId)
        }
        bridgeHandler.removeCallbacksAndMessages(null)
        bridgeThread.quitSafely()
        super.onDestroy()
    }

    private inner class IncomingHandler(looper: android.os.Looper) : Handler(looper) {
        override fun handleMessage(message: Message) {
            try {
                when (message.what) {
                    MSG_HELLO -> handleHello(message)
                    MSG_ADOPT_AND_STOP -> handleAdopt(message)
                    MSG_MOVE -> handleMove(message)
                    MSG_STOP -> handleStop(message)
                    MSG_HEARTBEAT -> handleHeartbeat(message)
                    MSG_RELEASE -> handleRelease(message)
                    else -> super.handleMessage(message)
                }
            } catch (_: RuntimeException) {
                // A malformed Bundle or platform failure may never strand a
                // previous motor command or kill the bridge's safety looper.
                forceRelease()
            }
        }
    }

    private fun handleHello(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        val requestId = data.getLong(KEY_REQUEST_ID, NO_REQUEST)
        val callerUid = message.sendingUid
        if (message.replyTo == null) return
        if (data.getInt(KEY_PROTOCOL_VERSION, -1) != PROTOCOL_VERSION) {
            reply(message, sessionId, requestId, false, "unsupported_protocol")
            return
        }
        if (sessionId <= 0L || requestId == NO_REQUEST || callerUid <= 0) {
            reply(message, sessionId, requestId, false, "invalid_hello")
            return
        }
        if (activeClientUid != NO_UID && activeClientUid != callerUid) {
            reply(message, sessionId, requestId, false, "bridge_busy")
            return
        }

        forceRelease()
        if (!app.acquireCompanionOutput(sessionId)) {
            reply(message, sessionId, requestId, false, "device_not_armed_or_ready")
            return
        }
        activeClientUid = callerUid
        activeSessionId = sessionId
        controller.arm(sessionId, SystemClock.elapsedRealtime())
        reply(message, sessionId, requestId, true, "")
    }

    private fun handleAdopt(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        val requestId = data.getLong(KEY_REQUEST_ID, NO_REQUEST)
        if (!isAuthorized(message, sessionId) || requestId == NO_REQUEST) {
            reply(message, sessionId, requestId, false, "unauthorized_session")
            return
        }
        val generation = data.getLong(KEY_GENERATION, NO_GENERATION)
        val generationAccepted = generation != NO_GENERATION && controller.adoptGenerationAndStop(
            sessionId,
            generation,
            SystemClock.elapsedRealtime()
        )
        val accepted = generationAccepted && app.stopCompanionOutput(sessionId)
        if (generationAccepted && !accepted) forceRelease()
        reply(
            message,
            sessionId,
            requestId,
            accepted,
            if (accepted) "" else if (generationAccepted) "output_stop_failed" else "stale_generation"
        )
    }

    private fun handleMove(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        if (!isAuthorized(message, sessionId)) return
        val generation = data.getLong(KEY_GENERATION, NO_GENERATION)
        if (!controller.isCurrentGeneration(sessionId, generation)) return
        if (!data.containsKey(KEY_CURRENT_POSITION_MILLI) ||
            !data.containsKey(KEY_TARGET_POSITION_MILLI) ||
            !data.containsKey(KEY_DURATION_MS) ||
            !data.containsKey(KEY_MEDIA_POSITION_MS) ||
            data.getLong(KEY_MEDIA_POSITION_MS, -1L) < 0L
        ) {
            forceRelease()
            return
        }
        val result = controller.move(
            sessionId = sessionId,
            generation = generation,
            currentPositionMilli = data.getInt(KEY_CURRENT_POSITION_MILLI),
            targetPositionMilli = data.getInt(KEY_TARGET_POSITION_MILLI),
            durationMs = data.getLong(KEY_DURATION_MS),
            nowMs = SystemClock.elapsedRealtime()
        )
        if (result == CompanionMoveResult.Invalid) forceRelease()
    }

    private fun handleStop(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        val requestId = data.getLong(KEY_REQUEST_ID, NO_REQUEST)
        if (!isAuthorized(message, sessionId) || requestId == NO_REQUEST) {
            reply(message, sessionId, requestId, false, "unauthorized_session")
            return
        }
        val generationAccepted = controller.stop(
            sessionId,
            data.getLong(KEY_GENERATION, NO_GENERATION),
            SystemClock.elapsedRealtime()
        )
        val accepted = generationAccepted && app.stopCompanionOutput(sessionId)
        if (generationAccepted && !accepted) forceRelease()
        reply(
            message,
            sessionId,
            requestId,
            accepted,
            if (accepted) "" else if (generationAccepted) "output_stop_failed" else "stale_generation"
        )
    }

    private fun handleHeartbeat(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        if (!isAuthorized(message, sessionId)) return
        controller.heartbeat(
            sessionId,
            data.getLong(KEY_GENERATION, NO_GENERATION),
            SystemClock.elapsedRealtime()
        )
    }

    private fun handleRelease(message: Message) {
        val data = message.data
        val sessionId = data.getLong(KEY_SESSION_ID, NO_SESSION)
        val requestId = data.getLong(KEY_REQUEST_ID, NO_REQUEST)
        if (!isAuthorized(message, sessionId) || requestId == NO_REQUEST) {
            reply(message, sessionId, requestId, false, "unauthorized_session")
            return
        }
        val generationAccepted = controller.release(
            sessionId,
            data.getLong(KEY_GENERATION, NO_GENERATION)
        )
        val accepted = generationAccepted && app.releaseCompanionOutput(sessionId)
        if (generationAccepted) {
            clearIdentity()
        }
        reply(
            message,
            sessionId,
            requestId,
            accepted,
            if (accepted) "" else if (generationAccepted) "output_stop_failed" else "stale_generation"
        )
    }

    private fun isAuthorized(message: Message, sessionId: Long): Boolean =
        activeClientUid != NO_UID &&
            message.sendingUid == activeClientUid &&
            sessionId == activeSessionId

    private fun forceRelease() {
        val releasedSession = controller.forceRelease() ?: activeSessionId.takeIf {
            it != NO_SESSION
        }
        if (releasedSession != null) app.releaseCompanionOutput(releasedSession)
        clearIdentity()
    }

    private fun clearIdentity() {
        activeSessionId = NO_SESSION
        activeClientUid = NO_UID
    }

    private fun reply(
        source: Message,
        sessionId: Long,
        requestId: Long,
        ok: Boolean,
        error: String
    ) {
        val target = source.replyTo ?: return
        val response = Message.obtain(null, MSG_ACK).apply {
            data = Bundle().apply {
                putInt(KEY_PROTOCOL_VERSION, PROTOCOL_VERSION)
                putLong(KEY_SESSION_ID, sessionId)
                putLong(KEY_REQUEST_ID, requestId)
                putBoolean(KEY_OK, ok)
                putString(KEY_ERROR, error)
            }
        }
        try {
            target.send(response)
        } catch (_: RemoteException) {
            if (source.sendingUid == activeClientUid && sessionId == activeSessionId) {
                forceRelease()
            }
        }
    }

    companion object {
        const val PROTOCOL_VERSION = 1

        const val MSG_HELLO = 1
        const val MSG_ADOPT_AND_STOP = 2
        const val MSG_MOVE = 3
        const val MSG_STOP = 4
        const val MSG_HEARTBEAT = 5
        const val MSG_RELEASE = 6
        const val MSG_ACK = 100

        const val KEY_PROTOCOL_VERSION = "protocol_version"
        const val KEY_SESSION_ID = "session_id"
        const val KEY_REQUEST_ID = "request_id"
        const val KEY_GENERATION = "generation"
        const val KEY_REASON = "reason"
        const val KEY_MEDIA_POSITION_MS = "media_position_ms"
        const val KEY_CURRENT_POSITION_MILLI = "current_position_milli"
        const val KEY_TARGET_POSITION_MILLI = "target_position_milli"
        const val KEY_DURATION_MS = "duration_ms"
        const val KEY_OK = "ok"
        const val KEY_ERROR = "error"

        private const val NO_SESSION = Long.MIN_VALUE
        private const val NO_GENERATION = Long.MIN_VALUE
        private const val NO_REQUEST = Long.MIN_VALUE
        private const val NO_UID = -1
    }
}
