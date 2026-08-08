package com.ashairfoil.chloevibes

import android.app.Application
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import com.ashairfoil.chloevibes.audio.AudioCaptureManager
import com.ashairfoil.chloevibes.audio.AudioSourceMode
import com.ashairfoil.chloevibes.device.BleDeviceManager
import com.ashairfoil.chloevibes.device.ConnectionState
import kotlin.math.roundToInt

/** UI-facing safety snapshot for the primary audio path and emergency stop. */
data class SafetyUiState(
    val watchdogTripped: Boolean = false,
    val message: String? = null,
    val motorsForcedZero: Boolean = false
)

class ChloeVibesApplication : Application() {
    lateinit var audioCaptureManager: AudioCaptureManager
        private set
    lateinit var bleDeviceManager: BleDeviceManager
        private set

    private val outputLock = Any()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val audioWatchdog = AudioPipelineWatchdog()

    @Volatile
    private var outputOwner = OutputOwner.Idle

    @Volatile
    private var companionSessionId = NO_SESSION

    @Volatile
    private var isCompanionArmed = false

    @Volatile
    private var safetyUiState = SafetyUiState()

    /** Optional UI listener; always invoked on the main thread. */
    @Volatile
    var onSafetyStateChanged: ((SafetyUiState) -> Unit)? = null

    private val watchdogPoll = object : Runnable {
        override fun run() {
            val now = SystemClock.elapsedRealtime()
            // Feed the latest processing-frame stamp, then age it.
            val hb = audioCaptureManager.lastPipelineHeartbeatMs
            if (hb > 0L) {
                audioWatchdog.heartbeat(hb)
            }
            if (audioWatchdog.poll(now)) {
                handleAudioDeadmanTrip(
                    "Pipeline heartbeat stale (>${AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS}ms); motors stopped"
                )
                return
            }
            if (audioWatchdog.isArmed()) {
                mainHandler.postDelayed(this, AudioPipelineWatchdog.POLL_INTERVAL_MS)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        audioCaptureManager = AudioCaptureManager(applicationContext)
        bleDeviceManager = BleDeviceManager(applicationContext)

        // The Application owns the only audio-to-device route. This prevents
        // an Activity callback from racing a bound companion session.
        // Writes are fenced under outputLock so a post-stop frame can never
        // re-arm motors (WAVE-002 M5 lease fence via OutputOwner).
        audioCaptureManager.onOutputUpdate = { output ->
            synchronized(outputLock) {
                if (outputOwner != OutputOwner.Audio) return@synchronized
                bleDeviceManager.setIntensity(output)
            }
        }
        audioCaptureManager.onDualOutputUpdate = { motor1, motor2 ->
            synchronized(outputLock) {
                if (outputOwner != OutputOwner.Audio) return@synchronized
                if (bleDeviceManager.isDualMotor) {
                    bleDeviceManager.setDualIntensity(motor1, motor2)
                } else {
                    bleDeviceManager.setIntensity(motor1)
                }
            }
        }
        // Fail-closed: persistent processing errors must zero motors.
        audioCaptureManager.onPipelineFailClosed = {
            mainHandler.post {
                handleAudioDeadmanTrip("Processing loop fail-closed; motors stopped")
            }
        }
    }

    fun currentSafetyUiState(): SafetyUiState = safetyUiState

    /** User intent from the Connect action; arming never initiates a connection. */
    fun armConnectedDeviceForCompanion() {
        isCompanionArmed = true
    }

    fun disarmCompanion() {
        synchronized(outputLock) {
            isCompanionArmed = false
            if (outputOwner == OutputOwner.Companion) {
                stopMotorsSafely()
                companionSessionId = NO_SESSION
                outputOwner = OutputOwner.Idle
            }
        }
    }

    fun startAudioCapture(): Boolean {
        synchronized(outputLock) {
            if (outputOwner == OutputOwner.Companion) return false
            // Claim the route before starting the producer so the first frame
            // is admitted; revoke first on any start failure.
            outputOwner = OutputOwner.Audio
            clearSafetyBannerLocked()
        }
        val started = audioCaptureManager.start(AudioSourceMode.SystemAudio) ||
            audioCaptureManager.start(AudioSourceMode.Microphone)
        var killOrphanProducer = false
        var ok = false
        synchronized(outputLock) {
            if (!started) {
                if (outputOwner == OutputOwner.Audio) {
                    outputOwner = OutputOwner.Idle
                }
                stopMotorsSafely()
                publishSafetyLocked(
                    SafetyUiState(
                        watchdogTripped = false,
                        message = "Audio capture failed to start",
                        motorsForcedZero = true
                    )
                )
            } else if (outputOwner == OutputOwner.Audio) {
                // Only arm if we still own the route (companion may have raced).
                val now = SystemClock.elapsedRealtime()
                audioWatchdog.arm(now)
                scheduleWatchdogLocked()
                ok = true
            } else {
                // Ownership lost during start — kill the producer outside lock.
                killOrphanProducer = true
            }
        }
        if (killOrphanProducer) stopAudioProducerSafely()
        return ok
    }

    fun stopAudioCapture() {
        val stopProducer: Boolean
        synchronized(outputLock) {
            if (outputOwner != OutputOwner.Audio) return
            // Revoke the route before stopping the producer so its final frame
            // can never overwrite the safety stop.
            outputOwner = OutputOwner.Idle
            mainHandler.removeCallbacks(watchdogPoll)
            audioWatchdog.disarm()
            stopMotorsSafely()
            stopProducer = true
        }
        // Join the processing thread outside outputLock to avoid deadlock
        // with a frame blocked on the same lock in onOutputUpdate.
        if (stopProducer) stopAudioProducerSafely()
    }

    /**
     * Red-button stop: zero motors, kill audio producer, drop companion lease.
     * Bound Finally sessions fail closed on the next position write.
     */
    fun emergencyStopAll() {
        val stopProducer: Boolean
        synchronized(outputLock) {
            mainHandler.removeCallbacks(watchdogPoll)
            audioWatchdog.disarm()
            companionSessionId = NO_SESSION
            stopProducer = outputOwner == OutputOwner.Audio || audioCaptureManager.isRunning
            outputOwner = OutputOwner.Idle
            val stopped = stopMotorsSafely()
            publishSafetyLocked(
                SafetyUiState(
                    watchdogTripped = false,
                    message = if (stopped) "All devices stopped" else "Stop sent (verify device is still)",
                    motorsForcedZero = true
                )
            )
        }
        if (stopProducer) stopAudioProducerSafely()
    }

    /**
     * Atomically preempt audio and establish a companion output fence.
     * The user must have armed the currently connected device first.
     */
    fun acquireCompanionOutput(sessionId: Long): Boolean {
        val stopProducer: Boolean
        val acquired: Boolean
        synchronized(outputLock) {
            if (!isCompanionArmed || bleDeviceManager.connectionState != ConnectionState.Ready) {
                return false
            }
            // Companion owns the device: disarm audio dead-man so it cannot
            // fight the bridge's own 1.75s heartbeat lease.
            mainHandler.removeCallbacks(watchdogPoll)
            audioWatchdog.disarm()
            stopProducer = outputOwner == OutputOwner.Audio || audioCaptureManager.isRunning
            outputOwner = OutputOwner.Companion
            companionSessionId = sessionId
            val stopped = stopMotorsSafely()
            if (!stopped) {
                companionSessionId = NO_SESSION
                outputOwner = OutputOwner.Idle
                acquired = false
            } else {
                acquired = true
            }
        }
        if (stopProducer) stopAudioProducerSafely()
        return acquired
    }

    fun setCompanionPosition(sessionId: Long, positionMilli: Int): Boolean =
        synchronized(outputLock) {
            if (outputOwner != OutputOwner.Companion || companionSessionId != sessionId) {
                return@synchronized false
            }
            if (bleDeviceManager.connectionState != ConnectionState.Ready) {
                stopMotorsSafely()
                return@synchronized false
            }
            runCatching {
                bleDeviceManager.setIntensity(
                    HereSphereInversePositionScalarFallback.intensity(positionMilli)
                )
            }.isSuccess
        }

    fun stopCompanionOutput(sessionId: Long): Boolean = synchronized(outputLock) {
        if (outputOwner != OutputOwner.Companion || companionSessionId != sessionId) {
            return@synchronized false
        }
        stopMotorsSafely()
    }

    fun releaseCompanionOutput(sessionId: Long): Boolean = synchronized(outputLock) {
        if (outputOwner != OutputOwner.Companion || companionSessionId != sessionId) {
            return@synchronized false
        }
        val stopped = stopMotorsSafely()
        companionSessionId = NO_SESSION
        outputOwner = OutputOwner.Idle
        stopped
    }

    /** Preserve a live bound session when the ChloeVibes UI is closed. */
    fun disconnectIfNoCompanionSession() {
        val stopProducer: Boolean
        synchronized(outputLock) {
            if (outputOwner == OutputOwner.Companion) return
            mainHandler.removeCallbacks(watchdogPoll)
            audioWatchdog.disarm()
            stopProducer = outputOwner == OutputOwner.Audio || audioCaptureManager.isRunning
            outputOwner = OutputOwner.Idle
            stopMotorsSafely()
            runCatching { bleDeviceManager.disconnect() }
            isCompanionArmed = false
        }
        if (stopProducer) stopAudioProducerSafely()
    }

    private fun handleAudioDeadmanTrip(reason: String) {
        val stopProducer: Boolean
        synchronized(outputLock) {
            // Only the audio path is supervised by this watchdog. Companion
            // sessions use CompanionSessionController and must not be torn
            // down here if ownership already moved.
            mainHandler.removeCallbacks(watchdogPoll)
            stopProducer = outputOwner == OutputOwner.Audio
            if (stopProducer) {
                // Revoke route first so a late processing frame cannot re-arm.
                outputOwner = OutputOwner.Idle
            }
            // Always attempt zero when the audio dead-man fires; harmless if
            // already idle / disconnected.
            stopMotorsSafely()
            audioWatchdog.acknowledgeTrip()
            publishSafetyLocked(
                SafetyUiState(
                    watchdogTripped = true,
                    message = reason,
                    motorsForcedZero = true
                )
            )
            Log.w("ChloeVibes", reason)
        }
        if (stopProducer) stopAudioProducerSafely()
    }

    private fun scheduleWatchdogLocked() {
        mainHandler.removeCallbacks(watchdogPoll)
        mainHandler.postDelayed(watchdogPoll, AudioPipelineWatchdog.POLL_INTERVAL_MS)
    }

    private fun clearSafetyBannerLocked() {
        if (safetyUiState.watchdogTripped || safetyUiState.message != null) {
            publishSafetyLocked(SafetyUiState())
        }
    }

    private fun publishSafetyLocked(state: SafetyUiState) {
        safetyUiState = state
        val listener = onSafetyStateChanged
        mainHandler.post { listener?.invoke(state) }
    }

    private fun stopAudioProducerSafely() {
        runCatching { audioCaptureManager.stop() }
    }

    private fun stopMotorsSafely(): Boolean =
        runCatching { bleDeviceManager.stopMotors() }.isSuccess

    private enum class OutputOwner {
        Idle,
        Audio,
        Companion
    }

    private companion object {
        const val NO_SESSION = Long.MIN_VALUE
    }
}

/**
 * Pure, clock-injected dead-man for the primary audio→BLE path.
 * Mirrors desktop [WATCHDOG_TIMEOUT_MS] = 2s. The Finally companion path
 * keeps its own [CompanionSessionController] lease (1.75s) and is not
 * driven by this class.
 */
internal class AudioPipelineWatchdog(
    private val timeoutMs: Long = DEFAULT_TIMEOUT_MS
) {
    init {
        require(timeoutMs in 1L..2_000L)
    }

    private var armed = false
    private var lastHeartbeatMs = 0L
    private var tripped = false

    fun arm(nowMs: Long) {
        require(nowMs >= 0L)
        armed = true
        tripped = false
        lastHeartbeatMs = nowMs
    }

    fun disarm() {
        armed = false
        tripped = false
        lastHeartbeatMs = 0L
    }

    /** After a trip has been handled, stop polling without clearing sticky UI. */
    fun acknowledgeTrip() {
        armed = false
        // leave tripped=true for observers until next arm()
    }

    /**
     * Stamp a successful processing frame.
     * Rejects clock regression (fail closed) and stamps after trip (no extend).
     */
    fun heartbeat(nowMs: Long): Boolean {
        if (!armed || tripped) return false
        if (nowMs < lastHeartbeatMs) {
            tripped = true
            return false
        }
        lastHeartbeatMs = nowMs
        return true
    }

    /**
     * @return true when motors must be forced to zero (sticky until re-arm).
     */
    fun poll(nowMs: Long): Boolean {
        if (!armed && !tripped) return false
        if (tripped) return true
        if (!armed) return false
        if (nowMs < lastHeartbeatMs) {
            tripped = true
            return true
        }
        if (nowMs - lastHeartbeatMs >= timeoutMs) {
            tripped = true
            return true
        }
        return false
    }

    fun isArmed(): Boolean = armed
    fun isTripped(): Boolean = tripped
    fun lastHeartbeatMs(): Long = lastHeartbeatMs

    companion object {
        /** Desktop parity: 2s pipeline stall → stop devices. */
        const val DEFAULT_TIMEOUT_MS = 2_000L
        const val POLL_INTERVAL_MS = 250L
    }
}

/**
 * Compatibility fallback observed in HereSphere's legacy Lovense path.
 * Funscript position remains a position everywhere else in the bridge.
 */
internal object HereSphereInversePositionScalarFallback {
    fun intensity(positionMilli: Int): Float =
        positionMilli.let {
            require(it in 0..CompanionSessionController.MAX_POSITION_MILLI)
            1f - it / CompanionSessionController.MAX_POSITION_MILLI.toFloat()
        }
}

internal data class CompanionOutput(
    val sessionId: Long,
    val positionMilli: Int,
    val shouldRelease: Boolean = false
)

internal enum class CompanionMoveResult {
    Accepted,
    Stale,
    Invalid
}

/** Pure, clock-injected session and ramp state machine used by the bound service. */
internal class CompanionSessionController(
    private val heartbeatTimeoutMs: Long = DEFAULT_HEARTBEAT_TIMEOUT_MS
) {
    init {
        require(heartbeatTimeoutMs in 1L..2_000L)
    }

    private var activeSessionId = NO_SESSION
    private var activeGeneration = NO_GENERATION
    private var lastHeartbeatMs = 0L
    private var ramp: Ramp? = null

    fun arm(sessionId: Long, nowMs: Long) {
        require(sessionId > 0L)
        require(nowMs >= 0L)
        activeSessionId = sessionId
        activeGeneration = NO_GENERATION
        lastHeartbeatMs = nowMs
        ramp = null
    }

    fun adoptGenerationAndStop(sessionId: Long, generation: Long, nowMs: Long): Boolean {
        if (sessionId != activeSessionId || generation == NO_GENERATION) return false
        if (generation < activeGeneration || nowMs < lastHeartbeatMs) return false
        activeGeneration = generation
        lastHeartbeatMs = nowMs
        ramp = null
        return true
    }

    fun move(
        sessionId: Long,
        generation: Long,
        currentPositionMilli: Int,
        targetPositionMilli: Int,
        durationMs: Long,
        nowMs: Long
    ): CompanionMoveResult {
        if (!isCurrentGeneration(sessionId, generation)) return CompanionMoveResult.Stale
        if (nowMs < lastHeartbeatMs ||
            currentPositionMilli !in 0..MAX_POSITION_MILLI ||
            targetPositionMilli !in 0..MAX_POSITION_MILLI ||
            durationMs < 0L
        ) {
            clear()
            return CompanionMoveResult.Invalid
        }
        lastHeartbeatMs = nowMs
        ramp = Ramp(
            generation = generation,
            startPositionMilli = currentPositionMilli,
            targetPositionMilli = targetPositionMilli,
            startTimeMs = nowMs,
            durationMs = durationMs
        )
        return CompanionMoveResult.Accepted
    }

    fun isCurrentGeneration(sessionId: Long, generation: Long): Boolean =
        activeGeneration != NO_GENERATION &&
            sessionId == activeSessionId &&
            generation == activeGeneration

    fun heartbeat(sessionId: Long, generation: Long, nowMs: Long): Boolean {
        if (!isCurrentGeneration(sessionId, generation)) return false
        if (nowMs < lastHeartbeatMs) return false
        lastHeartbeatMs = nowMs
        return true
    }

    fun stop(sessionId: Long, generation: Long, nowMs: Long): Boolean {
        if (!isCurrentGeneration(sessionId, generation)) return false
        if (nowMs < lastHeartbeatMs) return false
        lastHeartbeatMs = nowMs
        ramp = null
        return true
    }

    fun release(sessionId: Long, generation: Long): Boolean {
        if (sessionId != activeSessionId || generation != activeGeneration) return false
        clear()
        return true
    }

    fun forceRelease(): Long? {
        if (activeSessionId == NO_SESSION) return null
        val released = activeSessionId
        clear()
        return released
    }

    fun poll(nowMs: Long): CompanionOutput? {
        if (activeSessionId == NO_SESSION) return null
        if (nowMs < lastHeartbeatMs) {
            val invalidSession = activeSessionId
            clear()
            return CompanionOutput(invalidSession, MAX_POSITION_MILLI, shouldRelease = true)
        }
        if (nowMs - lastHeartbeatMs >= heartbeatTimeoutMs) {
            val expiredSession = activeSessionId
            clear()
            return CompanionOutput(expiredSession, MAX_POSITION_MILLI, shouldRelease = true)
        }

        val activeRamp = ramp ?: return null
        if (activeRamp.generation != activeGeneration) {
            ramp = null
            return null
        }
        val elapsedMs = (nowMs - activeRamp.startTimeMs).coerceAtLeast(0L)
        val fraction = if (activeRamp.durationMs == 0L) {
            1f
        } else {
            (elapsedMs.toDouble() / activeRamp.durationMs.toDouble())
                .coerceIn(0.0, 1.0)
                .toFloat()
        }
        val position = (
            activeRamp.startPositionMilli +
                (activeRamp.targetPositionMilli - activeRamp.startPositionMilli) * fraction
            ).roundToInt().coerceIn(0, MAX_POSITION_MILLI)
        if (fraction >= 1f) ramp = null
        return CompanionOutput(activeSessionId, position)
    }

    private fun clear() {
        activeSessionId = NO_SESSION
        activeGeneration = NO_GENERATION
        lastHeartbeatMs = 0L
        ramp = null
    }

    private data class Ramp(
        val generation: Long,
        val startPositionMilli: Int,
        val targetPositionMilli: Int,
        val startTimeMs: Long,
        val durationMs: Long
    )

    companion object {
        const val MAX_POSITION_MILLI = 100_000
        const val RAMP_INTERVAL_MS = 50L
        const val DEFAULT_HEARTBEAT_TIMEOUT_MS = 1_750L
        // Sparse scripts and very slow playback can legitimately produce huge
        // wall-clock segments. A ramp is O(1) state and remains heartbeat
        // leased, so duration itself needs no artificial upper ceiling.
        const val MAX_RAMP_DURATION_MS = Long.MAX_VALUE
        private const val NO_SESSION = Long.MIN_VALUE
        private const val NO_GENERATION = Long.MIN_VALUE
    }
}
