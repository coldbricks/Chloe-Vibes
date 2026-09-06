package com.ashairfoil.chloevibes.audio

import java.util.concurrent.atomic.AtomicBoolean

/**
 * One monotonic epoch for the lifetime of the processing state, including
 * capture stop/start. Keep absolute nanoseconds integral until subtraction.
 */
internal class AudioProcessingClock(private val originNanos: Long = System.nanoTime()) {
    private val climaxResetPending = AtomicBoolean(false)

    fun elapsedMs(nowNanos: Long): Float =
        ((nowNanos - originNanos) / 1_000_000.0).toFloat()

    /** UI may request while stopped; the next processing frame owns the reset. */
    fun requestClimaxReset() {
        climaxResetPending.set(true)
    }

    /** A resumed capture starts a fresh cycle; failed starts retain pending UI requests. */
    fun onCaptureStartResult(succeeded: Boolean) {
        if (succeeded) requestClimaxReset()
    }

    fun applyPendingClimaxReset(engine: ClimaxEngine, frameNanos: Long): Boolean {
        if (!climaxResetPending.getAndSet(false)) return false
        engine.reset(elapsedMs(frameNanos))
        return true
    }
}
