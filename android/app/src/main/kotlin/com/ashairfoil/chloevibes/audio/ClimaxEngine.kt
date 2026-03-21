// ==========================================================================
// ClimaxEngine.kt -- Time-domain escalation with tease/surge cycle
// Synced with audio.rs ClimaxEngine
//
// Adds a slow time-based "build -> tease -> surge" layer on top of the
// audio-reactive envelope output. Features:
//   - Asymmetric tease: fast cliff → hold → slow sensual rebuild
//   - Accelerating surge curve (slow build → explosive finish)
//   - Progression-scaled chaos, sub-harmonics, and onset response
//   - Breathing-rate modulation (couples to arousal breathing ~0.18Hz)
//   - Escalating edge-and-deny across cycles
//   - Arousal momentum: aggressive escalation compensates desensitization
//   - Dual-motor phasing: spatial movement between independent motors
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

    // Breathing-rate modulation: couples to involuntary arousal breathing
    private var breathingPhase: Float = 0f

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
        breathingPhase = 0f
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

        // Wrap cycle and track arousal momentum across completed cycles.
        // Momentum grows faster than it decays -- sessions escalate over time,
        // compensating for neural desensitization from prolonged stimulation.
        if (currentTimeMs - cycleAnchorMs >= cycleLen) {
            val cycles = floor((currentTimeMs - cycleAnchorMs) / cycleLen).coerceAtLeast(1f)
            cycleAnchorMs += cycles * cycleLen
            cycleCount++
            arousalMomentum = (arousalMomentum + 0.12f).coerceAtMost(0.75f)
        }
        // Slow momentum decay during silence
        if (!gateOpen) {
            arousalMomentum = (arousalMomentum - dt * 0.008f).coerceAtLeast(0f)
        }

        val progress = ((currentTimeMs - cycleAnchorMs) / cycleLen).coerceIn(0f, 1f)
        val intensityClamped = intensity.coerceIn(0f, 1f)
        val cycleMaturity = (cycleCount / 6f).coerceAtMost(1f) // 0→1 over first 6 cycles

        val ramp = when (pattern) {
            ClimaxPattern.Wave -> smoothStep(progress)
            ClimaxPattern.Stairs -> {
                val steps = 6f
                (floor(progress * steps) / steps).coerceIn(0f, 1f)
            }
            ClimaxPattern.Surge -> progress.toDouble().pow(0.6).toFloat()
        }

        // ---- Tease factor: asymmetric dip near end of cycle ----
        // Fast cliff down → hold at floor → slow sensual rebuild.
        // Tease depth escalates across cycles -- first tease is gentle,
        // later ones are devastating.
        val teaseStart = 1f - teaseRatio.coerceIn(0.05f, 0.5f)
        val teaseFactor = if (progress >= teaseStart) {
            val t = ((progress - teaseStart) / (1f - teaseStart)).coerceIn(0f, 1f)
            val escalatingDrop = teaseDrop.coerceIn(0f, 0.9f) * (0.6f + 0.4f * cycleMaturity)
            val envelope = when {
                t < 0.10f -> {
                    // Sharp cliff down (first 10% of tease window)
                    smoothStep(t / 0.10f)
                }
                t < 0.55f -> {
                    // Hold at floor -- nerve endings reset, anticipation builds
                    1f
                }
                else -> {
                    // Slow curved rebuild (last 45%) -- agonizingly gradual
                    val rebuildT = (t - 0.55f) / 0.45f
                    val ss = smoothStep(rebuildT)
                    1f - ss * ss
                }
            }
            1f - escalatingDrop * envelope
        } else {
            1f
        }

        // ---- Surge factor: accelerating curve (slow build → explosive finish) ----
        // smooth_step² starts almost flat, then rockets upward in the final moments.
        val surgeStart = 0.80f
        val surgeFactor = if (progress >= surgeStart) {
            val t = ((progress - surgeStart) / (1f - surgeStart)).coerceIn(0f, 1f)
            val ss = smoothStep(t)
            1f + surgeBoost.coerceIn(0f, 1.5f) * ss * ss
        } else {
            1f
        }

        // ---- Onset boost: scales with cycle progression ----
        // A drum hit during surge should feel like being pushed over the edge.
        if (isOnset && gateOpen) {
            val onsetScale = 0.14f + 0.22f * ramp // 0.14 → 0.36 across cycle
            onsetBoost = (onsetBoost + onsetScale * onsetStrength.coerceIn(0f, 2.5f)).coerceAtMost(0.60f)
        }
        onsetBoost = (onsetBoost - dt * 0.7f).coerceAtLeast(0f)

        // ---- 5-oscillator detuned micro-pulse ----
        val pd = pulseDepth.coerceIn(0f, 0.55f)
        val maxPulseHz = if (progress >= surgeStart) 10f else 7f
        val pulseRateHz = (2f + intensityClamped * 3f + energy * 2f + ramp * 1f).coerceAtMost(maxPulseHz)
        val detune1 = 0.07f
        val detune2 = 0.13f
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

        // ---- Sub-harmonic resonance: scales with progression ----
        // Base 8% depth, building to 24% during surge.
        val subFreqHz = 1.5f + ramp * 2.5f + energy * 0.5f
        subHarmonicPhase = wrapPhase(subHarmonicPhase + dt * subFreqHz * tau)
        val subDepth = 0.08f + 0.16f * ramp // 8% → 24%
        val subResonance = 1f + subDepth * intensityClamped * sin(subHarmonicPhase)

        // ---- Chaos layer (Lorenz attractor): scales with progression ----
        // Barely noticeable at cycle start (6%), unmistakable at surge (18%).
        val sigma = 10f; val rho = 28f; val beta = 8f / 3f
        val chaosStep = dt * 0.8f
        val cdx = sigma * (chaosY - chaosX) * chaosStep
        val cdy = (chaosX * (rho - chaosZ) - chaosY) * chaosStep
        val cdz = (chaosX * chaosY - beta * chaosZ) * chaosStep
        chaosX = (chaosX + cdx).coerceIn(-30f, 30f)
        chaosY = (chaosY + cdy).coerceIn(-30f, 30f)
        chaosZ = (chaosZ + cdz).coerceIn(0f, 50f)
        val chaosDepth = 0.06f + 0.12f * ramp // 6% → 18%
        val chaosMod = 1f + chaosDepth * intensityClamped * (chaosX / 30f)

        // ---- Breathing-rate modulation ----
        // Human arousal breathing settles at ~0.15-0.25 Hz. This very slow
        // sine couples with the user's involuntary breathing pattern,
        // amplifying the physiological feedback loop between body and device.
        val breathingHz = 0.18f
        breathingPhase = wrapPhase(breathingPhase + dt * breathingHz * tau)
        val breathingDepth = 0.06f + 0.10f * ramp // 6% → 16%
        val breathingMod = 1f + breathingDepth * sin(breathingPhase)

        // ---- Arousal gain: aggressive escalation ----
        // At ramp=0: gain = 1.0 (passthrough).
        // At ramp=1 with max momentum: up to 3.8x -- overwhelming crescendo.
        val momentumBonus = arousalMomentum * 0.7f
        val arousalGain = (1f + (1.2f + momentumBonus) * ramp) * (1f + intensityClamped * 0.40f)
        val gatedBoost = if (gateOpen) onsetBoost else 0f

        val rawOutput = (dry * arousalGain * teaseFactor * surgeFactor
                * pulse * subResonance * chaosMod * breathingMod
                + gatedBoost).coerceIn(0f, 1f)

        // ---- Dual-motor spatial contrast ----
        val phaseOffsetHz = 0.3f + ramp * 1.7f
        motor2Phase = wrapPhase(motor2Phase + dt * phaseOffsetHz * tau)
        val phaseMod = 0.5f + 0.5f * sin(motor2Phase)
        val antiPhaseDepth = rawOutput.coerceIn(0f, 1f) * 0.85f
        val motor2Factor = lerp(1f, 0.15f + 0.85f * phaseMod, antiPhaseDepth)
        motor2Output = (rawOutput * motor2Factor).coerceIn(0f, 1f)

        // ---- Edge-and-deny: escalating across cycles ----
        // First deny is gentle and brief. Later denies are deeper and longer,
        // building frustration and making each return more devastating.
        if (rawOutput > 0.75f) {
            highOutputMs += dt * 1000f
        } else {
            highOutputMs = (highOutputMs - dt * 400f).coerceAtLeast(0f)
        }

        // Deny trigger: 6s initially, dropping to 3s as cycles mature
        val denyTriggerMs = 6000f - 3000f * cycleMaturity
        if (!denyActive && highOutputMs > denyTriggerMs) {
            denyActive = true
            denyStartMs = currentTimeMs
            // Duration escalates: 600ms initially → up to 3000ms in later cycles
            val baseDuration = 600f + 1800f * cycleMaturity
            val jitter = 0.5f + 0.5f * sin(currentTimeMs * 0.00137f)
            denyDurationMs = baseDuration + 400f * jitter
            highOutputMs = 0f
        }

        if (denyActive) {
            val denyElapsed = currentTimeMs - denyStartMs
            if (denyElapsed >= denyDurationMs) {
                denyActive = false
                // Post-deny surge: overshoot harder after deeper denies.
                val postDenyBoost = 0.30f + 0.25f * cycleMaturity
                onsetBoost = (onsetBoost + postDenyBoost).coerceAtMost(0.65f)
            } else {
                val denyT = denyElapsed / denyDurationMs
                // Deny depth escalates: 60% initially → 90% at maturity
                val denyDepthVal = 0.60f + 0.30f * cycleMaturity
                // Asymmetric envelope: cliff → hold → slow sensual return.
                val denyEnvelope = when {
                    denyT < 0.10f -> {
                        // Sharp cliff (50-100ms to floor)
                        denyDepthVal * smoothStep(denyT / 0.10f)
                    }
                    denyT < 0.75f -> {
                        // Hold at floor -- nerve endings reset, ache builds
                        denyDepthVal
                    }
                    else -> {
                        // Slow curved return (last 25%) -- deliberately agonizing
                        val returnT = (denyT - 0.75f) / 0.25f
                        denyDepthVal * (1f - smoothStep(returnT))
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
