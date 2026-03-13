// ==========================================================================
// ClimaxEngine.kt -- Time-domain escalation with tease/surge cycle
// Ported from audio.rs ClimaxEngine
//
// Adds a slow time-based "build -> tease -> surge" layer on top of the
// audio-reactive envelope output. Features:
//   - Slowly raising effective intensity over a cycle
//   - Controlled dip near the end (tease)
//   - Surge back up with faster micro-pulses
//   - Triple-oscillator detuned micro-pulse (prevents single-freq adaptation)
//   - Edge-and-deny: forces intensity dips after sustained high output
//     to prevent plateau adaptation
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.*

/**
 * Climax engine -- slow modulation layer over audio-reactive output.
 */
class ClimaxEngine {

    private var cycleAnchorMs: Float = 0f
    private var lastTimeMs: Float = 0f
    private var microPhase: Float = 0f
    private var microPhase2: Float = 0f
    private var microPhase3: Float = 0f
    private var onsetBoost: Float = 0f

    // Edge tracking -- forces intensity dips to prevent plateau adaptation
    private var highOutputMs: Float = 0f
    private var denyActive: Boolean = false
    private var denyStartMs: Float = 0f
    private var denyDurationMs: Float = 0f

    fun reset(currentTimeMs: Float) {
        cycleAnchorMs = currentTimeMs
        lastTimeMs = currentTimeMs
        microPhase = 0f
        microPhase2 = 0f
        microPhase3 = 0f
        onsetBoost = 0f
        highOutputMs = 0f
        denyActive = false
        denyStartMs = 0f
        denyDurationMs = 0f
    }

    /** Returns current cycle progress in [0, 1). */
    fun phaseProgress(currentTimeMs: Float, buildUpMs: Float): Float {
        val cycleLen = buildUpMs.coerceIn(8_000f, 240_000f)
        if (cycleLen <= 0f) return 0f
        val raw = (currentTimeMs - cycleAnchorMs) / cycleLen
        return (raw - floor(raw)).coerceAtLeast(0f)
    }

    /**
     * Process one frame.
     *
     * @param input dry envelope output (0.0 - 1.0)
     * @param energy current audio energy
     * @param gateOpen whether the noise gate is open
     * @param isOnset whether a beat onset was detected
     * @param onsetStrength strength of the detected onset
     * @param currentTimeMs current time in milliseconds
     * @param enabled whether the climax engine is active
     * @param intensity overall strength of climax modulation
     * @param buildUpMs duration of one full build cycle in ms
     * @param teaseRatio fraction of cycle used for tease behavior
     * @param teaseDrop depth of the tease dip
     * @param surgeBoost end-of-cycle surge boost amount
     * @param pulseDepth depth of fast micro-pulse modulation
     * @param pattern modulation pattern shape
     * @return modulated output (0.0 - 1.0)
     */
    fun process(
        input: Float,
        energy: Float,
        gateOpen: Boolean,
        isOnset: Boolean,
        onsetStrength: Float,
        currentTimeMs: Float,
        enabled: Boolean,
        intensity: Float,
        buildUpMs: Float,
        teaseRatio: Float,
        teaseDrop: Float,
        surgeBoost: Float,
        pulseDepth: Float,
        pattern: ClimaxPattern
    ): Float {
        val dry = input.coerceIn(0f, 1f)
        if (!enabled) {
            reset(currentTimeMs)
            return dry
        }

        if (lastTimeMs <= 0f) {
            reset(currentTimeMs)
        }

        val cycleLen = buildUpMs.coerceIn(8_000f, 240_000f)
        val dt = ((currentTimeMs - lastTimeMs) * 0.001f).coerceIn(0f, 0.2f)
        lastTimeMs = currentTimeMs

        // Wrap cycle
        if (currentTimeMs - cycleAnchorMs >= cycleLen) {
            val cycles = floor((currentTimeMs - cycleAnchorMs) / cycleLen).coerceAtLeast(1f)
            cycleAnchorMs += cycles * cycleLen
        }

        val progress = ((currentTimeMs - cycleAnchorMs) / cycleLen).coerceIn(0f, 1f)
        val intensityClamped = intensity.coerceIn(0f, 1f)

        // Ramp shape based on pattern
        val ramp = when (pattern) {
            ClimaxPattern.Wave -> smoothStep(progress)
            ClimaxPattern.Stairs -> {
                val steps = 6f
                (floor(progress * steps) / steps).coerceIn(0f, 1f)
            }
            ClimaxPattern.Surge -> progress.toDouble().pow(0.6).toFloat()
        }

        // Tease factor: controlled dip near end of cycle
        val teaseStart = 1f - teaseRatio.coerceIn(0.05f, 0.5f)
        val teaseFactor = if (progress >= teaseStart) {
            val t = ((progress - teaseStart) / (1f - teaseStart)).coerceIn(0f, 1f)
            val envelope = 1f - abs(2f * t - 1f)
            1f - teaseDrop.coerceIn(0f, 0.9f) * envelope
        } else {
            1f
        }

        // Surge factor: aggressive boost in final 16% of cycle
        val surgeFactor = if (progress >= 0.84f) {
            val t = ((progress - 0.84f) / 0.16f).coerceIn(0f, 1f)
            // Steeper power curve (0.3 vs 0.5) -- hits harder at the end
            1f + surgeBoost.coerceIn(0f, 1.2f) * t.toDouble().pow(0.3).toFloat()
        } else {
            1f
        }

        // Onset boost: accumulates from beat detections
        if (isOnset && gateOpen) {
            onsetBoost = (onsetBoost + 0.08f * onsetStrength.coerceIn(0f, 2f)).coerceAtMost(0.30f)
        }
        onsetBoost = (onsetBoost - dt * 0.9f).coerceAtLeast(0f)

        // Triple-oscillator detuned micro-pulse
        val pd = pulseDepth.coerceIn(0f, 0.45f)
        val maxPulseHz = if (progress >= 0.84f) 8f else 6f
        val pulseRateHz = (2f + intensityClamped * 2f + energy * 1.5f + ramp * 0.5f).coerceAtMost(maxPulseHz)
        val detune = 0.07f // +/-7% frequency spread
        val tau = 2f * PI.toFloat()
        microPhase = (microPhase + dt * pulseRateHz * tau).rem(tau).let { if (it < 0) it + tau else it }
        microPhase2 = (microPhase2 + dt * pulseRateHz * (1f + detune) * tau).rem(tau).let { if (it < 0) it + tau else it }
        microPhase3 = (microPhase3 + dt * pulseRateHz * (1f - detune) * tau).rem(tau).let { if (it < 0) it + tau else it }
        val pulseRaw = 0.5f * sin(microPhase) +
                0.3f * sin(microPhase2) +
                0.2f * sin(microPhase3)
        val pulse = 1f - pd + pd * (0.5f + 0.5f * pulseRaw)

        // Arousal gain: build UP from the audio-reactive base
        // At ramp=0: gain = 1.0 (passthrough)
        // At ramp=1: gain = up to 1.85 (amplified)
        val arousalGain = (1f + 0.85f * ramp) * (1f + intensityClamped * 0.20f)
        val gatedBoost = if (gateOpen) onsetBoost else 0f

        val rawOutput = (dry * arousalGain * teaseFactor * surgeFactor * pulse + gatedBoost)
            .coerceIn(0f, 1f)

        // Edge-and-deny: when output has been >0.8 for >3 seconds, force a dip
        // to 60% for 2-4 seconds, then surge back. Prevents plateau adaptation.
        if (rawOutput > 0.8f) {
            highOutputMs += dt * 1000f
        } else {
            highOutputMs = (highOutputMs - dt * 500f).coerceAtLeast(0f)
        }

        if (!denyActive && highOutputMs > 3000f) {
            denyActive = true
            denyStartMs = currentTimeMs
            // Randomized deny duration: 2000-4000ms using cheap pseudo-random
            denyDurationMs = 2000f + 2000f * (0.5f + 0.5f * sin(currentTimeMs * 0.00137f))
            highOutputMs = 0f
        }

        if (denyActive) {
            val denyElapsed = currentTimeMs - denyStartMs
            if (denyElapsed >= denyDurationMs) {
                denyActive = false
            } else {
                // Smooth envelope: fade down then back up
                val denyT = denyElapsed / denyDurationMs
                // Parabolic dip: peaks at center of deny window
                val denyDepth = 0.40f // 40% reduction at deepest point
                val x = 2f * denyT - 1f
                val denyEnvelope = denyDepth * (1f - x * x)
                return (rawOutput * (1f - denyEnvelope)).coerceIn(0f, 1f)
            }
        }

        return rawOutput
    }
}
