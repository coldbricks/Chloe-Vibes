package com.ashairfoil.chloevibes.audio

/** Retry the selected Visualizer source after sustained silent callbacks. */
internal class SilentSystemAudioRecovery(
    private val silentFramesBeforeRetry: Int = 240,
    private val maxRetries: Int = 3,
    private val retrySpacingMs: Long = 5_000L,
) {
    var attempts: Int = 0
        private set
    val exhausted: Boolean get() = attempts >= maxRetries
    private var silentFrames = 0
    private var lastRetryMs: Long? = null

    /** Held/missing frames are not evidence of a new silent capture callback. */
    fun observe(nowMs: Long, freshFrame: Boolean, hasSignal: Boolean): Boolean {
        if (!freshFrame) return false
        if (hasSignal) {
            reset()
            return false
        }
        if (silentFrames < silentFramesBeforeRetry) silentFrames++
        if (silentFrames < silentFramesBeforeRetry || exhausted) return false
        val previous = lastRetryMs
        if (previous != null && nowMs - previous < retrySpacingMs) return false
        attempts++
        silentFrames = 0
        lastRetryMs = nowMs
        return true
    }

    fun reset() {
        attempts = 0
        silentFrames = 0
        lastRetryMs = null
    }
}
