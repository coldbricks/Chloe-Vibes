// ==========================================================================
// Gate.kt -- Noise gate with hysteresis and auto-gate
// Ported from audio.rs Gate
//
// Threshold gate with soft-knee hysteresis to prevent chattering, plus an
// auto-gate mode that adapts the threshold based on a rolling energy
// histogram (targeting ~25% open time).
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.roundToInt

/**
 * Noise gate with hysteresis and auto-gate capability.
 */
class Gate {

    /** Was the gate open last frame? */
    private var wasOpen: Boolean = false

    /** Smoothed gate signal (0.0 = closed, 1.0 = open). */
    var smoothed: Float = 0f
        private set

    /** Rolling histogram of energy levels (100 bins, 0-100%). */
    private val histogram = FloatArray(100)

    /** Total samples in histogram. */
    private var histogramSamples: Float = 0f

    /** Auto-calculated optimal threshold. */
    private var optimalThreshold: Float = 0.2f

    /** Frame counter for periodic histogram recalculation. */
    private var frameCount: Int = 0

    /**
     * Process one frame. Returns whether the gate is open.
     *
     * @param energy current audio energy level (0.0 - 1.0)
     * @param manualThreshold user-set threshold (0.0 - 1.0)
     * @param autoGateAmount blend between manual and auto (0.0 = manual, 1.0 = auto)
     * @param smoothing gate smoothing amount (0.0 = instant, 1.0 = very smooth)
     * @param thresholdKnee width of soft threshold region (0.0 = hard edge)
     */
    fun process(
        energy: Float,
        manualThreshold: Float,
        autoGateAmount: Float,
        smoothing: Float,
        thresholdKnee: Float
    ): Boolean {
        // Auto-gate: maintain energy histogram and calculate optimal threshold
        if (autoGateAmount > 0f) {
            val bin = (energy * 99f).roundToInt().coerceIn(0, 99)
            histogram[bin] += 1f
            histogramSamples += 1f
            frameCount++

            // Recalculate every ~86 frames (~2 seconds at 43Hz update rate)
            if (frameCount >= 86) {
                frameCount = 0

                // Find threshold that keeps gate open ~25% of the time
                val targetOpenTime = 0.25f
                var cumulative = 0f
                var optimalBin = 99

                for (i in 99 downTo 0) {
                    cumulative += histogram[i]
                    val percentOpen = cumulative / histogramSamples.coerceAtLeast(1f)
                    if (percentOpen >= targetOpenTime) {
                        optimalBin = i
                        break
                    }
                }

                val calculated = optimalBin / 100f
                // Smooth threshold changes to avoid jumps
                optimalThreshold = optimalThreshold * 0.7f + calculated * 0.3f

                // Decay histogram for rolling window effect
                for (i in histogram.indices) {
                    histogram[i] *= 0.5f
                }
                histogramSamples *= 0.5f
            }
        } else {
            // Reset histogram when auto-gate is off
            histogram.fill(0f)
            histogramSamples = 0f
            optimalThreshold = 0.2f
        }

        // Blend manual and auto thresholds
        val effectiveThreshold = lerp(manualThreshold, optimalThreshold, autoGateAmount)

        // Hysteresis: close threshold is slightly below open threshold to
        // prevent chattering.  The gap scales with the threshold itself so
        // low thresholds get a tight band and high thresholds get a wider one.
        val hysteresis = (effectiveThreshold * 0.25f).coerceIn(0.005f, 0.08f)
        val openThreshold = effectiveThreshold
        val closeThreshold = (effectiveThreshold - hysteresis).coerceAtLeast(0f)
        val isAbove = if (!wasOpen) {
            energy > openThreshold
        } else {
            energy > closeThreshold
        }

        // Asymmetric smoothing: instant open, smooth close.
        // Opening speed is critical for transient response — every ms
        // of gate delay eats the attack phase. Closing smoothness
        // prevents chatter without impacting onset timing.
        val gateSignal = if (isAbove) 1f else 0f
        if (smoothing > 0f) {
            val alpha = if (gateSignal > smoothed) {
                1f  // Instant open — don't filter the rising edge
            } else {
                1f - smoothing.coerceIn(0f, 0.98f)  // Smooth close
            }
            smoothed = smoothed * (1f - alpha) + gateSignal * alpha
        } else {
            smoothed = gateSignal
        }

        val open = smoothed > 0.5f
        wasOpen = open
        return open
    }

    /** Get the effective threshold from manual + auto blend. */
    fun effectiveThreshold(manual: Float, autoAmount: Float): Float {
        return lerp(manual, optimalThreshold, autoAmount)
    }
}
