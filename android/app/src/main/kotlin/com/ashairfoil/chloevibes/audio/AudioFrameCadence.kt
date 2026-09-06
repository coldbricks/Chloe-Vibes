package com.ashairfoil.chloevibes.audio

/** Capture cadence is independent of the 60 Hz envelope/output clock. */
internal class AudioFrameCadence(private val holdMs: Long = 120L) {
    enum class Frame { Fresh, Held, Missing }

    private var capturedAtMs: Long? = null
    private var fresh = false

    fun received(nowMs: Long) {
        capturedAtMs = nowMs
        fresh = true
    }

    fun poll(nowMs: Long): Frame {
        val captured = capturedAtMs ?: return Frame.Missing
        if (nowMs < captured || nowMs - captured > holdMs) {
            fresh = false
            return Frame.Missing
        }
        return if (fresh) {
            fresh = false
            Frame.Fresh
        } else {
            Frame.Held
        }
    }

    fun clear() {
        capturedAtMs = null
        fresh = false
    }
}

/** Both motors participate in delivery and in the final transition to zero. */
internal class AudioOutputDispatch {
    private var wasActive = false

    fun shouldSend(motor1: Float, motor2: Float): Boolean {
        val active = motor1 > 0.001f || motor2 > 0.001f
        val send = active || wasActive
        wasActive = active
        return send
    }

    fun reset() {
        wasActive = false
    }
}
