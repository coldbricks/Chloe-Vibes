// ==========================================================================
// BeatDetector.kt -- Onset detection via spectral flux
// Ported from audio.rs BeatDetector
//
// Uses adaptive thresholding on spectral flux to detect transients
// (drum hits, note onsets). The threshold adapts to local dynamics --
// grows after an onset to prevent double-triggering, then decays back
// to catch the next beat.
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.pow
import kotlin.math.sqrt

/**
 * Onset/beat detector using adaptive thresholding on spectral flux.
 */
class BeatDetector {

    companion object {
        /**
         * Predictive onset lead (ms). ~50ms matches fixed transport lag better
         * than the historical 76ms (ghosty on residual gate).
         */
        const val PREFIRE_LEAD_MS: Float = 50f
    }

    /** Rolling history of spectral flux values. */
    private val fluxHistory = FloatArray(43) // ~1 second at 43Hz

    /** Current index into the circular history buffer. */
    private var historyIndex: Int = 0

    /** Adaptive threshold multiplier. */
    private var adaptiveThreshold: Float = 0.55f

    /** Cooldown timestamp. */
    private var lastOnsetTimeMs: Float = 0f

    /** Minimum time between detected onsets (ms). */
    private val cooldownMs: Float = 55f // ~270 BPM 16th notes max

    /** Recent onset strength for velocity tracking. */
    private var recentOnsetStrength: Float = 0f

    // Tempo tracking for predictive onset
    private val onsetTimestamps = FloatArray(16)
    private var onsetTsIndex: Int = 0
    private var onsetTsCount: Int = 0

    /** Estimated inter-onset interval in ms (0 = no estimate). */
    @Volatile var tempoIntervalMs: Float = 0f
        private set

    /** Confidence in tempo estimate (0.0 = none, 1.0 = locked). */
    @Volatile var tempoConfidence: Float = 0f
        private set

    /** Predicted time of next onset in ms (0 = no prediction). */
    @Volatile var predictedNextOnsetMs: Float = 0f
        private set

    /**
     * Process spectral flux and detect onsets.
     * @return Pair of (isOnset, onsetStrength)
     */
    fun process(spectralFlux: Float, currentTimeMs: Float): Pair<Boolean, Float> {
        // Update history
        fluxHistory[historyIndex] = spectralFlux
        historyIndex = (historyIndex + 1) % fluxHistory.size

        // Calculate local statistics
        val mean = fluxHistory.sum() / fluxHistory.size
        var variance = 0f
        for (v in fluxHistory) {
            val diff = v - mean
            variance += diff * diff
        }
        variance /= fluxHistory.size
        val stdDev = sqrt(variance)

        // Adaptive threshold
        val threshold = mean + adaptiveThreshold * stdDev

        // Detect onset
        val isOnset = spectralFlux > threshold &&
                (currentTimeMs - lastOnsetTimeMs) > cooldownMs

        if (isOnset) {
            lastOnsetTimeMs = currentTimeMs
            // Moderate growth after onset (prevents rapid double-triggering)
            adaptiveThreshold = (adaptiveThreshold * 1.06f).coerceAtMost(1.8f)

            // Track onset velocity -- how much the flux exceeded threshold
            // gives us a "how hard was this hit" metric for envelope velocity
            val rawStrength = if (threshold > 0f) spectralFlux / threshold else 0f
            recentOnsetStrength = recentOnsetStrength * 0.3f + rawStrength * 0.7f

            // Record timestamp for tempo tracking
            onsetTimestamps[onsetTsIndex] = currentTimeMs
            onsetTsIndex = (onsetTsIndex + 1) % onsetTimestamps.size
            if (onsetTsCount < onsetTimestamps.size) onsetTsCount++

            // Update tempo prediction after accumulating enough onsets
            if (onsetTsCount >= 4) {
                updateTempoPrediction(currentTimeMs)
            }
        } else {
            // Faster decay back to baseline -- recover sensitivity between beats
            adaptiveThreshold = (adaptiveThreshold * 0.985f).coerceAtLeast(0.12f)
            recentOnsetStrength *= 0.98f
            // Tempo confidence must decay without new onsets
            decayTempoConfidence(currentTimeMs)
        }

        val strength = if (threshold > 0f) spectralFlux / threshold else 0f

        return Pair(isOnset, strength)
    }

    /** Smoothed onset strength for velocity / prefire synthetic floor. */
    fun recentOnsetStrength(): Float = recentOnsetStrength

    /**
     * Whether predictive pre-fire is allowed right now.
     * Requires a fresh tempo lock and a recent real onset (recency guard).
     */
    fun prefireOk(currentTimeMs: Float): Boolean {
        if (tempoConfidence <= 0.6f || tempoIntervalMs <= 0f) return false
        if (predictedNextOnsetMs <= 0f) return false
        // Recency: last real onset within ~1.75 beats (tightened from 2.0).
        val sinceOnset = currentTimeMs - lastOnsetTimeMs
        if (sinceOnset > tempoIntervalMs * 1.75f) return false
        val timeTo = predictedNextOnsetMs - currentTimeMs
        return timeTo in 0f..PREFIRE_LEAD_MS
    }

    /**
     * One-shot predictive onset for the motor path.
     * Returns synthetic strength floor so envelope drive accepts prefire while
     * flux is still low. Consumes the prediction (multi-frame latch).
     */
    fun takePrefire(currentTimeMs: Float): Float? {
        if (!prefireOk(currentTimeMs)) return null
        predictedNextOnsetMs += tempoIntervalMs.coerceAtLeast(1f)
        return recentOnsetStrength.coerceAtLeast(1.15f).coerceAtMost(1.35f)
    }

    /** Clear tempo lock and onset history so a dead lock cannot resurrect. */
    private fun clearTempoLock() {
        tempoConfidence = 0f
        tempoIntervalMs = 0f
        predictedNextOnsetMs = 0f
        onsetTsCount = 0
        onsetTsIndex = 0
        onsetTimestamps.fill(0f)
    }

    private fun decayTempoConfidence(currentTimeMs: Float) {
        if (tempoConfidence <= 0f || tempoIntervalMs <= 0f) return
        val since = currentTimeMs - lastOnsetTimeMs
        val grace = tempoIntervalMs * 1.5f
        if (since <= grace) {
            if (tempoConfidence > 0.5f) {
                val intervalsElapsed = (since / tempoIntervalMs).toInt()
                predictedNextOnsetMs =
                    lastOnsetTimeMs + (intervalsElapsed + 1) * tempoIntervalMs
            }
            return
        }
        val staleBeats = (since - grace) / tempoIntervalMs.coerceAtLeast(1f)
        // Slightly faster bleed than 0.55 — stale locks die before the next track.
        tempoConfidence = (tempoConfidence * 0.50f.pow(staleBeats)).coerceAtLeast(0f)
        if (tempoConfidence < 0.5f) {
            predictedNextOnsetMs = 0f
            if (tempoConfidence < 0.05f) {
                clearTempoLock()
            }
        } else {
            val intervalsElapsed = (since / tempoIntervalMs).toInt()
            predictedNextOnsetMs =
                lastOnsetTimeMs + (intervalsElapsed + 1) * tempoIntervalMs
        }
    }

    private fun updateTempoPrediction(currentTimeMs: Float) {
        // Collect inter-onset intervals from recent timestamps
        val intervals = FloatArray(onsetTsCount - 1)
        var count = 0
        for (i in 1 until onsetTsCount) {
            val curr = onsetTimestamps[(onsetTsIndex - i + onsetTimestamps.size) % onsetTimestamps.size]
            val prev = onsetTimestamps[(onsetTsIndex - i - 1 + onsetTimestamps.size) % onsetTimestamps.size]
            val interval = curr - prev
            if (interval in 150f..2000f) { // 30-400 BPM range
                intervals[count] = interval
                count++
            }
        }

        if (count < 3) {
            tempoConfidence = 0f
            predictedNextOnsetMs = 0f
            return
        }

        var mean = 0f
        for (i in 0 until count) mean += intervals[i]
        mean /= count

        var variance = 0f
        for (i in 0 until count) {
            val diff = intervals[i] - mean
            variance += diff * diff
        }
        variance /= count
        val stdDev = sqrt(variance)

        // Confidence: low coefficient of variation = high confidence
        val cv = if (mean > 0f) stdDev / mean else 1f
        tempoConfidence = (1f - cv * 4f).coerceIn(0f, 1f)
        tempoIntervalMs = mean

        if (tempoConfidence > 0.5f) {
            val lastOnset = onsetTimestamps[(onsetTsIndex - 1 + onsetTimestamps.size) % onsetTimestamps.size]
            val elapsed = currentTimeMs - lastOnset
            val intervalsElapsed = (elapsed / mean).toInt()
            predictedNextOnsetMs = lastOnset + (intervalsElapsed + 1) * mean
        } else {
            predictedNextOnsetMs = 0f
        }
    }
}
