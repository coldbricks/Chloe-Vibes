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

import kotlin.math.abs
import kotlin.math.pow
import kotlin.math.roundToInt
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

        /**
         * Fold a raw inter-onset interval into the perceptual beat octave
         * (70-180 BPM => 333-857ms). Onset detectors track the subdivision grid,
         * but the envelope and predictive pre-fire must lock to the felt beat.
         */
        @JvmStatic
        fun foldToPerceptualBeat(ioiMs: Float): Float {
            var t = ioiMs
            if (t <= 0f) return t
            while (t < 333f) t *= 2f
            while (t > 857f) t /= 2f
            return t
        }
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

    private var tempoConfidenceAtOnset: Float = 0f
    private var pendingPrefire: Pair<Float, Float>? = null
    private var playedPrefireMs: Float? = null
    private var matchedPrefireOnsetMs: Float? = null
    private var manualIntervalMs: Float? = null

    /** Tempo hint only; a real onset must anchor phase before prediction runs. */
    fun setManualTempo(bpm: Float?) {
        val interval = bpm?.takeIf { it.isFinite() && it in 30f..300f }?.let { 60_000f / it }
        if (interval != manualIntervalMs) {
            clearTempoLock()
            manualIntervalMs = interval
        }
    }

    /**
     * Process spectral flux and detect onsets.
     * @return Pair of (isOnset, onsetStrength)
     */
    fun process(spectralFlux: Float, currentTimeMs: Float): Pair<Boolean, Float> {
        if (!currentTimeMs.isFinite()) return false to 0f
        advanceTime(currentTimeMs)
        // A corrupt capture frame must not poison the rolling statistics.
        val flux = if (spectralFlux.isFinite()) spectralFlux.coerceAtLeast(0f) else 0f
        // Update history
        fluxHistory[historyIndex] = flux
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
        val isOnset = flux > threshold &&
                (currentTimeMs - lastOnsetTimeMs) > cooldownMs

        if (isOnset) {
            matchedPrefireOnsetMs = playedPrefireMs?.let { expected ->
                if (kotlin.math.abs(currentTimeMs - expected) <= PREFIRE_LEAD_MS) currentTimeMs else null
            }
            if (matchedPrefireOnsetMs != null) playedPrefireMs = null
            lastOnsetTimeMs = currentTimeMs
            // Moderate growth after onset (prevents rapid double-triggering)
            adaptiveThreshold = (adaptiveThreshold * 1.06f).coerceAtMost(1.8f)

            // Track onset velocity -- how much the flux exceeded threshold
            // gives us a "how hard was this hit" metric for envelope velocity
            val rawStrength = if (threshold > 0f) flux / threshold else 0f
            recentOnsetStrength = recentOnsetStrength * 0.3f + rawStrength * 0.7f

            // Record timestamp for tempo tracking
            onsetTimestamps[onsetTsIndex] = currentTimeMs
            onsetTsIndex = (onsetTsIndex + 1) % onsetTimestamps.size
            if (onsetTsCount < onsetTimestamps.size) onsetTsCount++

            // Update tempo prediction after accumulating enough onsets
            val manual = manualIntervalMs
            if (manual != null) {
                tempoIntervalMs = manual
                tempoConfidence = 1f
                tempoConfidenceAtOnset = 1f
                predictedNextOnsetMs = currentTimeMs + manual
            } else if (onsetTsCount >= 4) {
                updateTempoPrediction(currentTimeMs)
            }
        } else {
            // Faster decay back to baseline -- recover sensitivity between beats
            adaptiveThreshold = (adaptiveThreshold * 0.985f).coerceAtLeast(0.12f)
            recentOnsetStrength *= 0.98f
            // Tempo confidence must decay without new onsets
            decayTempoConfidence(currentTimeMs)
        }

        val strength = if (threshold > 0f) flux / threshold else 0f

        return Pair(isOnset, strength)
    }

    /** Smoothed onset strength for velocity / prefire synthetic floor. */
    fun recentOnsetStrength(): Float = recentOnsetStrength

    /**
     * Whether predictive pre-fire is allowed right now.
     * Requires a fresh tempo lock and a recent real onset (recency guard).
     */
    fun prefireOk(currentTimeMs: Float): Boolean {
        if (!currentTimeMs.isFinite()) return false
        if (tempoConfidence <= 0.6f || tempoIntervalMs <= 0f) return false
        if (predictedNextOnsetMs <= 0f) return false
        // Recency: last real onset within ~1.75 beats (tightened from 2.0).
        val sinceOnset = currentTimeMs - lastOnsetTimeMs
        if (sinceOnset < 0f || sinceOnset > tempoIntervalMs * 1.75f) return false
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
        pendingPrefire = currentTimeMs to predictedNextOnsetMs
        predictedNextOnsetMs += tempoIntervalMs.coerceAtLeast(1f)
        return recentOnsetStrength.coerceAtLeast(1.15f).coerceAtMost(1.35f)
    }

    /** Confirm only after the host's envelope accepted the synthetic trigger. */
    fun confirmPrefire(currentTimeMs: Float) {
        val pending = pendingPrefire ?: return
        if (pending.first == currentTimeMs) {
            playedPrefireMs = pending.second
            pendingPrefire = null
        }
    }

    /** Real onsets still train tempo; their envelope attack is already played. */
    fun isPrefiredOnset(currentTimeMs: Float): Boolean =
        matchedPrefireOnsetMs == currentTimeMs

    /** Advance prediction age without adding a duplicate captured spectrum. */
    fun advanceTime(currentTimeMs: Float) {
        if (currentTimeMs.isFinite()) decayTempoConfidence(currentTimeMs)
    }

    /** Clear tempo lock and onset history so a dead lock cannot resurrect. */
    fun clearTempoLock() {
        tempoConfidence = 0f
        tempoIntervalMs = 0f
        predictedNextOnsetMs = 0f
        onsetTsCount = 0
        onsetTsIndex = 0
        onsetTimestamps.fill(0f)
        tempoConfidenceAtOnset = 0f
        pendingPrefire = null
        playedPrefireMs = null
        matchedPrefireOnsetMs = null
    }

    private fun decayTempoConfidence(currentTimeMs: Float) {
        if (tempoConfidence <= 0f || tempoIntervalMs <= 0f) return
        val since = currentTimeMs - lastOnsetTimeMs
        if (since >= tempoIntervalMs * 3f) {
            clearTempoLock()
            return
        }
        val grace = tempoIntervalMs * 1.5f
        if (since <= grace) {
            // Still advance the prediction so the next beat stays on-grid.
            if (tempoConfidence > 0.5f && predictedNextOnsetMs > 0f) {
                while (predictedNextOnsetMs <= currentTimeMs) {
                    predictedNextOnsetMs += tempoIntervalMs
                }
            }
            return
        }
        val staleBeats = (since - grace) / tempoIntervalMs.coerceAtLeast(1f)
        // Slightly faster bleed than 0.55 — stale locks die before the next track.
        tempoConfidence = (tempoConfidenceAtOnset * 0.50f.pow(staleBeats)).coerceAtLeast(0f)
        if (tempoConfidence < 0.5f) {
            predictedNextOnsetMs = 0f
            if (tempoConfidence < 0.05f) {
                clearTempoLock()
            }
        } else if (predictedNextOnsetMs > 0f) {
            while (predictedNextOnsetMs <= currentTimeMs) {
                predictedNextOnsetMs += tempoIntervalMs
            }
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

        // 1. Raw interval statistics
        var rawMean = 0f
        for (i in 0 until count) rawMean += intervals[i]
        rawMean /= count

        var rawVariance = 0f
        for (i in 0 until count) {
            val diff = intervals[i] - rawMean
            rawVariance += diff * diff
        }
        rawVariance /= count
        val rawStdDev = sqrt(rawVariance)
        val rawCv = if (rawMean > 0f) rawStdDev / rawMean else 1f

        // 2. Folded perceptual beat statistics (70-180 BPM => 333-857ms)
        val foldedIntervals = FloatArray(count)
        for (i in 0 until count) {
            foldedIntervals[i] = foldToPerceptualBeat(intervals[i])
        }
        var foldedMean = 0f
        for (i in 0 until count) foldedMean += foldedIntervals[i]
        foldedMean /= count

        var foldedVariance = 0f
        for (i in 0 until count) {
            val diff = foldedIntervals[i] - foldedMean
            foldedVariance += diff * diff
        }
        foldedVariance /= count
        val foldedStdDev = sqrt(foldedVariance)
        val foldedCv = if (foldedMean > 0f) foldedStdDev / foldedMean else 1f

        // 3. Selection: if raw has high jitter (due to subdivisions or syncopation)
        // and folding recovers a tight rhythmic lock, or if raw is an eighth-note
        // subdivision grid (mean < 333ms), use folded.
        val useFolded = (rawCv > 0.12f && foldedCv < rawCv * 0.75f && foldedCv < 0.25f)
            || (rawMean < 333f && foldedCv < 0.20f)
            || (rawCv > 0.20f && foldedCv < 0.20f)

        val (chosenInterval, chosenCv) = if (useFolded) {
            Pair(foldedMean, foldedCv)
        } else {
            Pair(rawMean, rawCv)
        }

        // Confidence: low coefficient of variation = high confidence
        tempoConfidence = (1f - chosenCv * 4f).coerceIn(0f, 1f)
        tempoConfidenceAtOnset = tempoConfidence
        tempoIntervalMs = chosenInterval

        if (tempoConfidence > 0.5f) {
            // Beat-grid phase alignment:
            // Find which recent onset best anchors the grid phase.
            // On-grid hits have |t_j - t_anchor| ~= k * chosenInterval.
            val len = onsetTimestamps.size
            var bestAnchor = onsetTimestamps[(onsetTsIndex - 1 + len) % len]
            var bestScore = -1

            val numRecent = minOf(onsetTsCount, len)
            for (i in 1..numRecent) {
                val cand = onsetTimestamps[(onsetTsIndex - i + len) % len]
                var score = 0
                for (j in 1..numRecent) {
                    val other = onsetTimestamps[(onsetTsIndex - j + len) % len]
                    val diff = abs(other - cand)
                    val q = (diff / chosenInterval).roundToInt()
                    val err = abs(diff - q * chosenInterval)
                    if (err <= chosenInterval * 0.15f) {
                        score++
                    }
                }
                if (score > bestScore) {
                    bestScore = score
                    bestAnchor = cand
                }
            }

            val elapsed = maxOf(0f, currentTimeMs - bestAnchor)
            val intervalsElapsed = (elapsed / chosenInterval).toInt()
            var next = bestAnchor + (intervalsElapsed + 1) * chosenInterval
            while (next <= currentTimeMs) {
                next += chosenInterval
            }
            predictedNextOnsetMs = next
        } else {
            predictedNextOnsetMs = 0f
        }
    }
}
