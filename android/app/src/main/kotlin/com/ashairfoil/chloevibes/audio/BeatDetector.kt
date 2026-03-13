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

import kotlin.math.sqrt

/**
 * Onset/beat detector using adaptive thresholding on spectral flux.
 */
class BeatDetector {

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
        } else {
            // Faster decay back to baseline -- recover sensitivity between beats
            adaptiveThreshold = (adaptiveThreshold * 0.985f).coerceAtLeast(0.12f)
        }

        val strength = if (threshold > 0f) spectralFlux / threshold else 0f

        return Pair(isOnset, strength)
    }
}
