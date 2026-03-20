// ==========================================================================
// ClimaxEngine.kt -- Time-domain escalation with tease/surge cycle
// Ported from audio.rs ClimaxEngine
//
// Adds a slow time-based "build -> tease -> surge" layer on top of the
// audio-reactive envelope output. Features:
//   - Slowly raising effective intensity over a cycle
//   - Controlled dip near the end (tease)
//   - Surge back up with faster micro-pulses
//   - 5-oscillator detuned micro-pulse (prevents single-freq adaptation)
//   - Edge-and-deny: forces intensity dips after sustained high output
//     to prevent plateau adaptation
//   - Arousal momentum: tracks cumulative stimulation history for
//     escalating sensitivity across cycles
//   - Sub-harmonic resonance: injects low-frequency flutter that
//     couples with the device's mechanical resonance
//   - Chaos layer: Lorenz-inspired aperiodic modulation prevents
//     the nervous system from predicting and filtering patterns
//   - Dual-motor phasing: generates offset signals for devices with
//     two independent motors, creating spatial movement
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
    private var microPhase4: Float = 0f
    private var microPhase5: Float = 0f
    private var onsetBoost: Float = 0f

    // Edge tracking -- forces intensity dips to prevent plateau adaptation
    private var highOutputMs: Float = 0f
    private var denyActive: Boolean = false
    private var denyStartMs: Float = 0f
    private var denyDurationMs: Float = 0f

    // Arousal momentum -- tracks cumulative stimulation across cycles
    private var arousalMomentum: Float = 0f
    private var cycleCount: Int = 0

    // Chaos oscillator state (Lorenz attractor, simplified)
    private var chaosX: Float = 0.1f
    private var chaosY: Float = 0.0f
    private var chaosZ: Float = 0.0f

    // Sub-harmonic resonance phase
    private var subHarmonicPhase: Float = 0f

    // Dual motor phasing
    /** Secondary motor output (0.0 - 1.0) for dual-motor devices. */
    var motor2Output: Float = 0f
        private set
    private var motor2Phase: Float = 0f

    fun reset(currentTimeMs: Float) {
        cycleAnchorMs = currentTimeMs
        lastTimeMs = currentTimeMs
        microPhase = 0f
        microPhase2 = 0f
        microPhase3 = 0f
        microPhase4 = 0f
        microPhase5 = 0f
        onsetBoost = 0f
        highOutputMs = 0f
        denyActive = false
        denyStartMs = 0f
        denyDurationMs = 0f
        arousalMomentum = 0f
        cycleCount = 0
        chaosX = 0.1f
        chaosY = 0.0f
        chaosZ = 0.0f
        subHarmonicPhase = 0f
        motor2Output = 0f
        motor2Phase = 0f
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

        // Wrap cycle and track momentum
        if (currentTimeMs - cycleAnchorMs >= cycleLen) {
            val cycles = floor((currentTimeMs - cycleAnchorMs) / cycleLen).coerceAtLeast(1f)
            cycleAnchorMs += cycles * cycleLen
            cycleCount++
            // Each completed cycle increases arousal momentum --
            // sensitization builds faster than it decays
            arousalMomentum = (arousalMomentum + 0.08f).coerceAtMost(0.50f)
        }
        // Slow momentum decay between peaks of activity
        if (!gateOpen) {
            arousalMomentum = (arousalMomentum - dt * 0.01f).coerceAtLeast(0f)
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

        // Surge factor: aggressive boost in final 20% of cycle (was 16%)
        // Wider surge window + steeper curve = more dramatic finish
        val surgeStart = 0.80f
        val surgeFactor = if (progress >= surgeStart) {
            val t = ((progress - surgeStart) / (1f - surgeStart)).coerceIn(0f, 1f)
            // Very steep power curve (0.2) -- exponential slam at the end
            val surgeAmount = surgeBoost.coerceIn(0f, 1.5f)
            1f + surgeAmount * t.toDouble().pow(0.2).toFloat()
        } else {
            1f
        }

        // Onset boost: accumulates from beat detections -- higher cap, faster build
        if (isOnset && gateOpen) {
            onsetBoost = (onsetBoost + 0.14f * onsetStrength.coerceIn(0f, 2.5f)).coerceAtMost(0.50f)
        }
        onsetBoost = (onsetBoost - dt * 0.7f).coerceAtLeast(0f)

        // ---- 5-oscillator detuned micro-pulse ----
        // More oscillators = richer harmonic content, harder to adapt to
        val pd = pulseDepth.coerceIn(0f, 0.55f)
        val maxPulseHz = if (progress >= surgeStart) 10f else 7f
        val pulseRateHz = (2f + intensityClamped * 3f + energy * 2f + ramp * 1f).coerceAtMost(maxPulseHz)
        val detune1 = 0.07f
        val detune2 = 0.13f  // wider spread for outer oscillators
        val tau = 2f * PI.toFloat()
        microPhase  = wrapPhase(microPhase  + dt * pulseRateHz * tau)
        microPhase2 = wrapPhase(microPhase2 + dt * pulseRateHz * (1f + detune1) * tau)
        microPhase3 = wrapPhase(microPhase3 + dt * pulseRateHz * (1f - detune1) * tau)
        microPhase4 = wrapPhase(microPhase4 + dt * pulseRateHz * (1f + detune2) * tau)
        microPhase5 = wrapPhase(microPhase5 + dt * pulseRateHz * (1f - detune2) * tau)
        val pulseRaw = 0.35f * sin(microPhase) +
                0.22f * sin(microPhase2) +
                0.22f * sin(microPhase3) +
                0.11f * sin(microPhase4) +
                0.10f * sin(microPhase5)
        val pulse = 1f - pd + pd * (0.5f + 0.5f * pulseRaw)

        // ---- Sub-harmonic resonance ----
        // Low-frequency flutter (1.5-4 Hz) that couples with the device motor's
        // mechanical resonance. Most vibrator motors have a resonant frequency
        // around 150-200Hz; modulating amplitude at sub-harmonic rates creates
        // a "throbbing" sensation that penetrates deeper tissue.
        val subFreqHz = 1.5f + ramp * 2.5f + energy * 0.5f
        subHarmonicPhase = wrapPhase(subHarmonicPhase + dt * subFreqHz * tau)
        val subResonance = 1f + 0.12f * intensityClamped * sin(subHarmonicPhase)

        // ---- Chaos layer (simplified Lorenz attractor) ----
        // Aperiodic modulation prevents the nervous system from predicting
        // the pattern and filtering it out. The Lorenz system generates
        // deterministic but non-repeating waveforms.
        val sigma = 10f; val rho = 28f; val beta = 8f / 3f
        val chaosStep = dt * 0.8f  // slow the chaos for musical timing
        val dx = sigma * (chaosY - chaosX) * chaosStep
        val dy = (chaosX * (rho - chaosZ) - chaosY) * chaosStep
        val dz = (chaosX * chaosY - beta * chaosZ) * chaosStep
        chaosX = (chaosX + dx).coerceIn(-30f, 30f)
        chaosY = (chaosY + dy).coerceIn(-30f, 30f)
        chaosZ = (chaosZ + dz).coerceIn(0f, 50f)
        // Normalize chaos to a subtle modulation factor
        val chaosMod = 1f + 0.06f * intensityClamped * (chaosX / 30f)

        // Arousal gain: build UP from the audio-reactive base
        // Momentum from previous cycles increases the ceiling
        // At ramp=0: gain = 1.0 (passthrough)
        // At ramp=1: gain = up to 2.4 with max momentum (was 1.85)
        val momentumBonus = arousalMomentum * 0.5f
        val arousalGain = (1f + (1.0f + momentumBonus) * ramp) * (1f + intensityClamped * 0.35f)
        val gatedBoost = if (gateOpen) onsetBoost else 0f

        val rawOutput = (dry * arousalGain * teaseFactor * surgeFactor * pulse * subResonance * chaosMod + gatedBoost)
            .coerceIn(0f, 1f)

        // ---- Dual motor spatial contrast ----
        // At high intensity: strong anti-phase creates a "traveling wave"
        // sensation as vibration moves physically between motors.
        // At low intensity: motors stay closer to unison for raw power.
        // Phase rate scales with progression: slow build, fast surge.
        val phaseOffsetHz = 0.3f + ramp * 1.7f  // 0.3-2.0 Hz
        motor2Phase = wrapPhase(motor2Phase + dt * phaseOffsetHz * tau)
        val phaseMod = 0.5f + 0.5f * sin(motor2Phase) // 0 to 1
        // Anti-phase depth scales with output level — more contrast when loud
        val antiPhaseDepth = rawOutput.coerceIn(0f, 1f) * 0.85f
        // Blend between unison (both at rawOutput) and dramatic alternation
        val motor2Factor = lerp(1f, 0.15f + 0.85f * phaseMod, antiPhaseDepth)
        motor2Output = (rawOutput * motor2Factor).coerceIn(0f, 1f)

        // Edge-and-deny: when output has been >0.75 for long enough, force
        // a sharp dip then surge back HARDER. Psychophysiology: optimal
        // denial window is 4-8s of high stimulation (after anticipation
        // peaks but before adaptation sets in). Short deny (0.8-2s) —
        // long enough for contrast, short enough to preserve momentum.
        if (rawOutput > 0.75f) {
            highOutputMs += dt * 1000f
        } else {
            highOutputMs = (highOutputMs - dt * 400f).coerceAtLeast(0f)
        }

        if (!denyActive && highOutputMs > 5000f) {
            denyActive = true
            denyStartMs = currentTimeMs
            // Shorter deny window: 800-2000ms (was 1500-3500ms)
            denyDurationMs = 800f + 1200f * (0.5f + 0.5f * sin(currentTimeMs * 0.00137f))
            highOutputMs = 0f
        }

        if (denyActive) {
            val denyElapsed = currentTimeMs - denyStartMs
            if (denyElapsed >= denyDurationMs) {
                denyActive = false
                // Post-deny surge: overshoot to 120% of pre-deny level briefly.
                // The cliff-to-peak contrast creates the "gasp" moment.
                onsetBoost = (onsetBoost + 0.35f).coerceAtMost(0.55f)
            } else {
                val denyT = denyElapsed / denyDurationMs
                // Asymmetric deny envelope: sharp cliff down, hold at floor,
                // rapid ramp back. NOT a gradual V — the body wants a sudden
                // drop followed by a delayed return.
                val denyDepth = 0.85f
                val denyEnvelope = when {
                    denyT < 0.12f -> {
                        // Sharp exponential drop (50-100ms to floor)
                        val dropProgress = denyT / 0.12f
                        denyDepth * smoothStep(dropProgress)
                    }
                    denyT < 0.82f -> {
                        // Hold at floor — nerve endings reset
                        denyDepth
                    }
                    else -> {
                        // Rapid return (ramp back in ~18% of deny time)
                        val returnProgress = (denyT - 0.82f) / 0.18f
                        denyDepth * (1f - smoothStep(returnProgress))
                    }
                }
                val denied = (rawOutput * (1f - denyEnvelope)).coerceIn(0f, 1f)
                motor2Output = (motor2Output * (1f - denyEnvelope * 0.7f)).coerceIn(0f, 1f)
                return denied
            }
        }

        return rawOutput
    }

    private fun wrapPhase(phase: Float): Float {
        val tau = 2f * PI.toFloat()
        val wrapped = phase.rem(tau)
        return if (wrapped < 0f) wrapped + tau else wrapped
    }
}
