// ==========================================================================
// ClimaxEngine.kt -- Time-domain escalation with tease/surge cycle
// Synced with audio.rs ClimaxEngine
//
// Adds a slow time-based "build -> tease -> surge" layer on top of the
// audio-reactive envelope output. Features:
//   - Non-overlapping terminal arc: tease THEN surge
//   - Accelerating surge curve (slow build → explosive finish)
//   - Progression-scaled chaos, sub-harmonics, and onset response
//   - Breathing-rate modulation (couples to arousal breathing ~0.18Hz)
//   - Escalating edge-and-deny with true-rest silence-class
//   - Motor-expressible micro-pulse ≤5 Hz (depth budget over rate)
//   - Arousal momentum: aggressive escalation compensates desensitization
//   - Dual-motor phasing: spatial movement between independent motors
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.PI
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.sin

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

    // Breathing-rate modulation: couples to involuntary arousal breathing
    private var breathingPhase: Float = 0f

    // Dual motor phasing
    /** Secondary motor output (0.0 - 1.0) for dual-motor devices. */
    var motor2Output: Float = 0f
        private set
    private var motor2Phase: Float = 0f

    /** Silence-class this frame (deep deny rest / dry zero). Hosts hard-snap. */
    var silenceEvent: Boolean = false
        private set

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
        breathingPhase = 0f
        motor2Output = 0f
        motor2Phase = 0f
        silenceEvent = false
    }

    /** Returns current cycle progress in [0, 1). */
    fun phaseProgress(currentTimeMs: Float, buildUpMs: Float): Float {
        val cycleLen = buildUpMs.coerceIn(8_000f, 240_000f)
        if (cycleLen <= 0f) return 0f
        val raw = (currentTimeMs - cycleAnchorMs) / cycleLen
        return (raw - floor(raw)).coerceAtLeast(0f)
    }

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
        // Allow envelope velocity overshoot headroom through when climax is off.
        val dry = input.coerceIn(0f, 1.20f)
        silenceEvent = false
        if (!enabled) {
            reset(currentTimeMs)
            // Passthrough keeps punch headroom; mapOutput clamps device range.
            return dry
        }

        if (lastTimeMs <= 0f) {
            reset(currentTimeMs)
        }

        val cycleLen = buildUpMs.coerceIn(8_000f, 240_000f)
        val dt = ((currentTimeMs - lastTimeMs) * 0.001f).coerceIn(0f, 0.2f)
        lastTimeMs = currentTimeMs

        // Wrap cycle and track arousal momentum across completed cycles.
        if (currentTimeMs - cycleAnchorMs >= cycleLen) {
            val cycles = floor((currentTimeMs - cycleAnchorMs) / cycleLen).coerceAtLeast(1f)
            cycleAnchorMs += cycles * cycleLen
            // Advance by full wraps (not +1) so large dt jumps stay honest.
            val n = cycles.toInt().coerceAtLeast(1)
            cycleCount = (cycleCount + n).coerceAtMost(Int.MAX_VALUE)
            arousalMomentum = (arousalMomentum + 0.12f * cycles).coerceAtMost(0.75f)
        }
        // Slow momentum decay during silence
        if (!gateOpen) {
            arousalMomentum = (arousalMomentum - dt * 0.008f).coerceAtLeast(0f)
        }

        val progress = ((currentTimeMs - cycleAnchorMs) / cycleLen).coerceIn(0f, 1f)
        val intensityClamped = intensity.coerceIn(0f, 1f)
        // Maturity: fast ramp over first 6 cycles, then slow log growth past 1.0
        val cycleMaturity = if (cycleCount <= 6) {
            cycleCount / 6f
        } else {
            val extra = ln((cycleCount - 6).toFloat() + 1f)
            (1f + 0.12f * extra).coerceAtMost(1.20f)
        }

        val ramp = when (pattern) {
            ClimaxPattern.Wave -> smoothStep(progress)
            ClimaxPattern.Stairs -> {
                val steps = 6f
                (floor(progress * steps) / steps).coerceIn(0f, 1f)
            }
            ClimaxPattern.Surge -> progress.toDouble().pow(0.6).toFloat()
        }

        // ---- Terminal arc: tease THEN surge (non-overlapping) ----
        val surgeStart = 0.88f
        val teaseWindow = teaseRatio.coerceIn(0.05f, 0.5f)
        val teaseStart = (surgeStart - teaseWindow).coerceAtLeast(0.45f)
        val teaseFactor = if (progress >= teaseStart && progress < surgeStart) {
            val span = (surgeStart - teaseStart).coerceAtLeast(0.01f)
            val t = ((progress - teaseStart) / span).coerceIn(0f, 1f)
            val escalatingDrop =
                teaseDrop.coerceIn(0f, 0.9f) * (0.65f + 0.45f * cycleMaturity.coerceAtMost(1f))
            val envelope = when {
                t < 0.12f -> smoothStep(t / 0.12f)
                t < 0.62f -> 1f
                else -> {
                    val rebuildT = (t - 0.62f) / 0.38f
                    val ss = smoothStep(rebuildT)
                    1f - ss * ss
                }
            }
            1f - escalatingDrop * envelope
        } else {
            1f
        }

        // ---- Surge factor: accelerating curve after tease completes ----
        val surgeFactor = if (progress >= surgeStart) {
            val t = ((progress - surgeStart) / (1f - surgeStart)).coerceIn(0f, 1f)
            val ss = smoothStep(t)
            1f + surgeBoost.coerceIn(0f, 1.5f) * ss * ss
        } else {
            1f
        }

        // ---- Onset boost: scales with cycle progression ----
        if (isOnset && gateOpen && dry > 0.001f) {
            val onsetScale = 0.14f + 0.22f * ramp
            onsetBoost =
                (onsetBoost + onsetScale * onsetStrength.coerceIn(0f, 2.5f)).coerceAtMost(0.70f)
        }
        onsetBoost = (onsetBoost - dt * 0.7f).coerceAtLeast(0f)

        // ---- 5-oscillator detuned micro-pulse (motor-expressible ≤5 Hz) ----
        val pd = (pulseDepth.coerceIn(0f, 0.55f) * (1f + 0.20f * ramp)).coerceAtMost(0.62f)
        val maxPulseHz = 5f
        val pulseRateHz =
            (1.6f + intensityClamped * 1.8f + energy * 1.0f + ramp * 0.6f).coerceAtMost(maxPulseHz)
        val detune1 = 0.07f
        val detune2 = 0.13f
        val tau = 2f * PI.toFloat()
        microPhase = wrapPhase(microPhase + dt * pulseRateHz * tau)
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

        // ---- Sub-harmonic resonance: scales with progression ----
        val subFreqHz = (1.2f + ramp * 2.0f + energy * 0.4f).coerceAtMost(5f)
        subHarmonicPhase = wrapPhase(subHarmonicPhase + dt * subFreqHz * tau)
        val subDepth = 0.10f + 0.18f * ramp // 10% → 28%
        val subResonance = 1f + subDepth * intensityClamped * sin(subHarmonicPhase)

        // ---- Chaos layer (Lorenz attractor): scales with progression ----
        val sigma = 10f; val rho = 28f; val beta = 8f / 3f
        val chaosStep = dt * 0.8f
        val cdx = sigma * (chaosY - chaosX) * chaosStep
        val cdy = (chaosX * (rho - chaosZ) - chaosY) * chaosStep
        val cdz = (chaosX * chaosY - beta * chaosZ) * chaosStep
        chaosX = (chaosX + cdx).coerceIn(-30f, 30f)
        chaosY = (chaosY + cdy).coerceIn(-30f, 30f)
        chaosZ = (chaosZ + cdz).coerceIn(0f, 50f)
        val chaosDepth = 0.06f + 0.12f * ramp
        val chaosMod = 1f + chaosDepth * intensityClamped * (chaosX / 30f)

        // ---- Breathing-rate modulation ----
        val breathingHz = 0.18f
        breathingPhase = wrapPhase(breathingPhase + dt * breathingHz * tau)
        val breathingDepth = 0.06f + 0.10f * ramp
        val breathingMod = 1f + breathingDepth * sin(breathingPhase)

        // ---- Arousal gain: aggressive escalation ----
        val momentumBonus = arousalMomentum * 0.7f
        val arousalGain =
            (1f + (1.2f + momentumBonus) * ramp) * (1f + intensityClamped * 0.40f)
        // Dry silence (micro-pause / boom rest): no residual boost re-injection.
        val gatedBoost = if (gateOpen && dry > 0.001f) onsetBoost else 0f

        if (dry <= 0.001f) {
            motor2Output = 0f
            silenceEvent = true
        }

        // Soft ceiling when post-deny / onset boost is live so additive punch
        // is not clamp-killed on already-loud dry plateaus.
        val peakCap = if (gatedBoost > 0.05f) 1.12f else 1f
        val rawOutput = if (dry <= 0.001f) {
            0f
        } else {
            (dry * arousalGain * teaseFactor * surgeFactor * pulse * subResonance * chaosMod
                    * breathingMod + gatedBoost).coerceIn(0f, peakCap)
        }

        // ---- Dual-motor spatial contrast ----
        val phaseOffsetHz = 0.3f + ramp * 1.7f
        motor2Phase = wrapPhase(motor2Phase + dt * phaseOffsetHz * tau)
        val phaseMod = 0.5f + 0.5f * sin(motor2Phase)
        val antiPhaseDepth = rawOutput.coerceIn(0f, 1f) * 0.85f
        val motor2Factor = lerp(1f, 0.15f + 0.85f * phaseMod, antiPhaseDepth)
        motor2Output = (rawOutput * motor2Factor).coerceIn(0f, 1f)

        // ---- Edge-and-deny: escalating across cycles ----
        // Freeze high_output dwell while deny is active — count felt output only.
        if (!denyActive) {
            if (rawOutput > 0.75f) {
                highOutputMs += dt * 1000f
            } else {
                highOutputMs = (highOutputMs - dt * 400f).coerceAtLeast(0f)
            }
        }

        val maturityForTiming = cycleMaturity.coerceAtMost(1.15f)
        val denyTriggerMs = 6000f - 3200f * maturityForTiming
        if (!denyActive && highOutputMs > denyTriggerMs) {
            denyActive = true
            denyStartMs = currentTimeMs
            val baseDuration = 700f + 2200f * maturityForTiming
            val jitter = 0.5f + 0.5f * sin(currentTimeMs * 0.00137f)
            denyDurationMs = baseDuration + 500f * jitter
            highOutputMs = 0f
        }

        if (denyActive) {
            val denyElapsed = currentTimeMs - denyStartMs
            if (denyElapsed >= denyDurationMs) {
                denyActive = false
                // Post-deny return punch: additive boost survives loud dry plateaus.
                val postDenyBoost = 0.40f + 0.35f * maturityForTiming
                onsetBoost = (onsetBoost + postDenyBoost).coerceAtMost(0.85f)
            } else {
                val denyT = denyElapsed / denyDurationMs.coerceAtLeast(1f)
                val denyDepthVal = (0.70f + 0.28f * maturityForTiming).coerceAtMost(0.95f)
                val denyEnvelope = when {
                    denyT < 0.10f -> denyDepthVal * smoothStep(denyT / 0.10f)
                    denyT < 0.75f -> denyDepthVal
                    else -> {
                        val returnT = (denyT - 0.75f) / 0.25f
                        denyDepthVal * (1f - smoothStep(returnT))
                    }
                }
                // Deep deny (≥30% cut) → true motor rest (silence-class).
                if (denyEnvelope >= 0.30f) {
                    silenceEvent = true
                    motor2Output = 0f
                    return 0f
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
