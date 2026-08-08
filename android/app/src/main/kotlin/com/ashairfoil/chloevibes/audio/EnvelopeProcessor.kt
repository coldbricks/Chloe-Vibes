// ==========================================================================
// EnvelopeProcessor.kt -- Full ADSR envelope with configurable curves
// Ported from audio.rs EnvelopeProcessor
//
// Each stage has a configurable curve exponent:
//   1.0 = linear
//   < 1.0 = fast start, slow finish (logarithmic feel)
//   > 1.0 = slow start, fast finish (exponential feel)
//
// The enhanced sustain modulation uses multi-layer oscillation to prevent
// neural adaptation during sustained stimulation.
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.sin

// ---------------------------------------------------------------------------
// Envelope State
// ---------------------------------------------------------------------------

enum class EnvelopeState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release
}

// ---------------------------------------------------------------------------
// EnvelopeProcessor
// ---------------------------------------------------------------------------

/**
 * Full Attack-Decay-Sustain-Release envelope processor.
 * Transforms raw gate/trigger events into smooth, shaped output curves.
 */
class EnvelopeProcessor {

    companion object {
        /**
         * Sustain at or below this is a pluck/boom floor: after Decay the
         * envelope releases to rest instead of holding a continuous hum.
         * Mirrors Rust EnvelopeProcessor::PLUCK_SUSTAIN.
         */
        const val PLUCK_SUSTAIN: Float = 0.15f
    }

    var state: EnvelopeState = EnvelopeState.Idle
        private set

    /** Current envelope value (0.0 - 1.0). */
    var value: Float = 0f
        private set

    /** Value when current phase started. */
    private var phaseStartValue: Float = 0f

    /** Trigger magnitude (how hard the trigger was). */
    private var magnitude: Float = 0f

    /** Attack target — normally 1.0, higher with velocity overshoot. */
    private var attackTarget: Float = 1f

    /** Timestamp when current phase started (ms). */
    private var startTimeMs: Float = 0f

    /** Was the gate open last frame? */
    private var lastGateOpen: Boolean = false

    /** Minimum time between retriggers (ms). */
    private val minRetriggerMs: Float = 20f

    /** Time of last trigger (ms). */
    private var lastTriggerTimeMs: Float = 0f

    /** Velocity of last trigger (gate-edge → real-onset upgrade). */
    private var lastTriggerVelocity: Float = 0f

    /** Stochastic micro-pause: next pause timestamp (ms). */
    private var nextMicroPauseMs: Float = 0f

    /** End time of the active micro-pause (ms); 0 = not pausing. Time-based. */
    private var microPauseUntilMs: Float = 0f

    /**
     * True this frame when the envelope forced a silence-class zero
     * (micro-pause). Downstream slew / peak-hold must hard-snap to rest.
     */
    var silenceEvent: Boolean = false
        private set

    /** Last time updateMagnitude ran (ms) for time-based sustain smoothing. */
    private var lastMagUpdateMs: Float = 0f

    /**
     * After Decay completes: hold Sustain (organ/pad) or fall through Release
     * to true rest (pluck/boom). Mirrors Rust EnvelopeProcessor::enter_post_decay.
     */
    private fun enterPostDecay(sustainLevel: Float, currentTimeMs: Float) {
        value = sustainLevel
        if (sustainLevel <= PLUCK_SUSTAIN) {
            if (sustainLevel <= 0.001f) {
                value = 0f
                state = EnvelopeState.Idle
                magnitude = 0f
                // Boom floor at zero: silence-class so slew/peak-hold hard-snap.
                silenceEvent = true
            } else {
                state = EnvelopeState.Release
                startTimeMs = currentTimeMs
                phaseStartValue = sustainLevel
            }
        } else {
            state = EnvelopeState.Sustain
        }
    }

    /** Trigger the envelope (gate just opened or strong onset detected). */
    fun trigger(magnitude: Float, currentTimeMs: Float, velocity: Float, attackMs: Float = 30f) {
        // Enforce minimum retrigger interval — allow real onset to upgrade a
        // gate-edge thump (velocity=1) that fired a few ms earlier.
        if (currentTimeMs - lastTriggerTimeMs < minRetriggerMs) {
            val upgrade = velocity > 1.05f &&
                velocity > lastTriggerVelocity + 0.08f &&
                state != EnvelopeState.Idle
            if (!upgrade) return
        }

        // Magnitude tracks body level; velocity > 1 lives in attackTarget
        // overshoot so process() headroom is not double-scaled then clipped.
        val velForMag = velocity.coerceAtMost(1.0f)
        val scaledMagnitude = magnitude * (0.5f + 0.5f * velForMag)
        this.magnitude = scaledMagnitude.coerceIn(0f, 1.0f)

        // Velocity overshoot: strong onsets briefly exceed normal peak.
        // A hard drum hit should momentarily push past the normal ceiling,
        // creating a visceral "punch" sensation.
        attackTarget = if (velocity > 1.0f) {
            (1.0f + 0.30f * (velocity - 1.0f)).coerceAtMost(1.28f)
        } else {
            1.0f
        }
        lastTriggerVelocity = velocity

        // For short attacks (< 50ms), skip directly to peak.
        // Motor spin-up (~20ms) provides the physical ramp — sending peak
        // immediately ensures the BLE command carries the full transient.
        if (attackMs < 50f) {
            state = EnvelopeState.Decay
            value = attackTarget
            phaseStartValue = attackTarget
            startTimeMs = currentTimeMs
        } else {
            state = EnvelopeState.Attack
            startTimeMs = currentTimeMs
            phaseStartValue = value.coerceAtLeast(0.4f)
        }

        // Abort an active micro-pause on retrigger, but keep the scheduled
        // next rest so pad/continuous material still gets anti-adaptation zeros.
        microPauseUntilMs = 0f
        silenceEvent = false
        lastTriggerTimeMs = currentTimeMs
    }

    /** Release the envelope (gate just closed). */
    fun release(currentTimeMs: Float) {
        if (state != EnvelopeState.Idle && state != EnvelopeState.Release) {
            state = EnvelopeState.Release
            startTimeMs = currentTimeMs
            phaseStartValue = value
        }
    }

    /**
     * Update the sustain magnitude (for dynamic modes where energy
     * changes while gate is held open). Time-based so frame rate cannot
     * make the hold smoother/faster than designed.
     */
    fun updateMagnitude(newMagnitude: Float, currentTimeMs: Float) {
        if (state == EnvelopeState.Sustain) {
            val dtS = if (lastMagUpdateMs > 0f) {
                ((currentTimeMs - lastMagUpdateMs) / 1000f).coerceIn(0f, 0.05f)
            } else {
                1f / 60f
            }
            lastMagUpdateMs = currentTimeMs
            val tau = if (newMagnitude > magnitude) 0.050f else 0.110f
            val alpha = (1f - kotlin.math.exp(-dtS / tau))
            magnitude = magnitude * (1f - alpha) + newMagnitude * alpha
        }
    }

    /**
     * Process one frame of the envelope. Returns output value (0.0 - 1.0).
     */
    fun process(
        currentTimeMs: Float,
        attackMs: Float,
        decayMs: Float,
        sustainLevel: Float,
        releaseMs: Float,
        attackCurve: Float,
        decayCurve: Float,
        releaseCurve: Float
    ): Float {
        val elapsed = currentTimeMs - startTimeMs
        silenceEvent = false

        when (state) {
            EnvelopeState.Attack -> {
                if (attackMs <= 0.5f) {
                    // Instant attack
                    value = attackTarget
                    state = EnvelopeState.Decay
                    startTimeMs = currentTimeMs
                    phaseStartValue = attackTarget
                } else {
                    val progress = (elapsed / attackMs).coerceIn(0f, 1f)
                    val curved = applyCurve(progress, attackCurve)
                    value = phaseStartValue + (attackTarget - phaseStartValue) * curved

                    if (progress >= 1f) {
                        value = attackTarget
                        state = EnvelopeState.Decay
                        startTimeMs = currentTimeMs
                        phaseStartValue = attackTarget
                    }
                }
            }

            EnvelopeState.Decay -> {
                if (decayMs <= 0.5f) {
                    enterPostDecay(sustainLevel, currentTimeMs)
                } else {
                    val progress = (elapsed / decayMs).coerceIn(0f, 1f)
                    val decayFactor = applyCurve(1f - progress, decayCurve)
                    value = sustainLevel + (phaseStartValue - sustainLevel) * decayFactor

                    if (progress >= 1f) {
                        enterPostDecay(sustainLevel, currentTimeMs)
                    }
                }
            }

            EnvelopeState.Sustain -> {
                // Stochastic micro-pauses: true zero for 90–120 ms (time-based).
                // silenceEvent flags the output stage to bypass slew/peak-hold.
                if (microPauseUntilMs > 0f && currentTimeMs < microPauseUntilMs) {
                    value = 0f
                    silenceEvent = true
                } else if (nextMicroPauseMs > 0f && currentTimeMs >= nextMicroPauseMs) {
                    // 90–120 ms pause — floor raised so rest completes on hardware
                    val pseudoRand = ((currentTimeMs * 7.13f).toInt() and 0xFFFF).toFloat() / 65535f
                    val pauseMs = 90f + pseudoRand * 30f
                    microPauseUntilMs = currentTimeMs + pauseMs
                    val gapRand = ((currentTimeMs * 13.37f).toInt() and 0xFFFF).toFloat() / 65535f
                    nextMicroPauseMs = currentTimeMs + 2000f + gapRand * 6000f
                    value = 0f
                    silenceEvent = true
                } else {
                    microPauseUntilMs = 0f
                    // First rest sooner (0.8–2.5 s) so pads get a zero early
                    if (nextMicroPauseMs <= 0f) {
                        val pseudoRand = ((currentTimeMs * 13.37f).toInt() and 0xFFFF).toFloat() / 65535f
                        nextMicroPauseMs = currentTimeMs + 800f + pseudoRand * 1700f
                    }

                    // Multi-layer modulation to prevent neural adaptation.
                    // 5 layers with irrational frequency ratios ensure the
                    // combined waveform never exactly repeats, keeping nerve
                    // endings from filtering out the stimulus.
                    val primary = 0.22f * sin(currentTimeMs * 0.0075f)     // ~1.2Hz
                    val secondary = 0.14f * sin(currentTimeMs * 0.0019f)   // ~0.3Hz
                    val tertiary = 0.10f * sin(currentTimeMs * 0.01696f)   // ~2.7Hz
                    val crossFreq = 0.08f * sin(currentTimeMs * 0.001068f) // ~0.17Hz
                    val noise = 0.10f * (
                            sin(currentTimeMs * 0.00317f) * 0.30f +
                            sin(currentTimeMs * 0.00713f) * 0.25f +
                            sin(currentTimeMs * 0.01137f) * 0.20f +
                            sin(currentTimeMs * 0.02173f) * 0.15f +
                            sin(currentTimeMs * 0.00491f) * 0.10f
                    )
                    val modulation = 1f + primary + secondary + tertiary + crossFreq + noise
                    // Clamp to [0,1] to mirror Rust (audio.rs: sustain_level * modulation
                    // is .clamp(0.0, 1.0)). Without this the engines diverge for any
                    // sustain_level above ~0.61, where modulation (peak ~1.64) pushes the
                    // stored value past 1.0 on Kotlin only.
                    value = (sustainLevel * modulation).coerceIn(0f, 1f)
                }
            }

            EnvelopeState.Release -> {
                if (releaseMs <= 0.5f) {
                    value = 0f
                    state = EnvelopeState.Idle
                    magnitude = 0f
                    silenceEvent = true
                } else {
                    val progress = (elapsed / releaseMs).coerceIn(0f, 1f)
                    val releaseFactor = applyCurve(1f - progress, releaseCurve)
                    value = phaseStartValue * releaseFactor

                    if (value <= 0.001f || progress >= 1f) {
                        value = 0f
                        state = EnvelopeState.Idle
                        magnitude = 0f
                        // Pluck/boom true-rest: hard-snap past slew so troughs empty.
                        silenceEvent = true
                    }
                }
            }

            EnvelopeState.Idle -> {
                value = (value * 0.95f).coerceAtLeast(0f) // Gentle fade
                if (value < 0.001f) {
                    value = 0f
                    silenceEvent = true
                }
                magnitude = 0f
            }
        }

        // Magnitude scales body level; attackTarget overshoot may exceed 1.0.
        // Cap at 1.20 so Auto-Lock binary headroom becomes real punch.
        return (value * magnitude).coerceIn(0f, 1.20f)
    }

    /**
     * Drive the envelope from gate state and onset detection.
     * Main entry point called each frame.
     */
    fun drive(
        gateOpen: Boolean,
        energy: Float,
        isOnset: Boolean,
        onsetStrength: Float,
        currentTimeMs: Float,
        triggerMode: TriggerMode,
        threshold: Float,
        thresholdKnee: Float,
        dynamicCurve: Float,
        binaryLevel: Float,
        hybridBlend: Float,
        attackMs: Float,
        decayMs: Float,
        sustainLevel: Float,
        releaseMs: Float,
        attackCurve: Float,
        decayCurve: Float,
        releaseCurve: Float,
        spectralCentroid: Float = 1000f
    ): Float {
        // Frequency-dependent envelope shaping: bass = deep sustained
        // pressure, treble = sharp surface tingling. Spectral centroid
        // tells us whether the current sound is bass-heavy or bright.
        val centroidNorm = ((spectralCentroid - 100f) / 4000f).coerceIn(0f, 1f)
        // Bass: hold longer (continuous pressure). Treble: release faster (tap).
        val adjSustainLevel = sustainLevel * (1f - 0.25f * centroidNorm)
        val adjReleaseMs = releaseMs * (1f + 0.4f * (1f - centroidNorm))

        // Calculate dynamic component
        val dynamicComponent = run {
            val knee = thresholdKnee.coerceIn(0f, 0.45f)
            val start = (threshold - knee).coerceIn(0f, 1f)
            val span = (1f - start).coerceAtLeast(0.01f)
            val normalized = ((energy - start) / span).coerceIn(0f, 1f)
            normalized.pow(dynamicCurve.coerceIn(0.35f, 2.5f))
        }

        // Calculate trigger magnitude based on mode
        val mag = when (triggerMode) {
            TriggerMode.Dynamic -> dynamicComponent
            TriggerMode.Binary -> if (gateOpen) binaryLevel else 0f
            TriggerMode.Hybrid -> {
                dynamicComponent * (1f - hybridBlend) +
                        if (gateOpen) binaryLevel * hybridBlend else 0f
            }
        }

        // Gate edge detection
        val gateJustOpened = gateOpen && !lastGateOpen
        val gateJustClosed = !gateOpen && lastGateOpen

        // Onset retrigger in any post-attack state (and Idle). Pluck envelopes
        // rest in Idle while residual energy holds the gate open — without
        // Idle/Release onset arming, later kicks are dropped.
        val isOnsetTrigger = isOnset && onsetStrength > 1.05f &&
                gateOpen && state != EnvelopeState.Attack

        // Continuous (pad/organ) re-arm only — use centroid-adjusted sustain
        // (same threshold enterPostDecay uses) so near-boundary pads do not
        // Idle-retrigger stutter. Never for boom/pluck paths (Domi hang fix).
        val continuousHold = adjSustainLevel > PLUCK_SUSTAIN &&
                (triggerMode == TriggerMode.Dynamic ||
                    (triggerMode == TriggerMode.Hybrid && hybridBlend < 0.45f))

        // Trigger logic
        if (gateJustOpened || isOnsetTrigger) {
            val velocity = if (isOnsetTrigger) {
                onsetStrength.coerceAtMost(1.35f)
            } else {
                1f
            }
            trigger(mag.coerceAtLeast(0.03f), currentTimeMs, velocity, attackMs)
        } else if (gateOpen && state == EnvelopeState.Idle && continuousHold && mag > 0.05f) {
            // Pad/organ: re-enter while gate held. Require real magnitude.
            trigger(mag, currentTimeMs, 1f, attackMs)
        } else if (gateJustClosed) {
            release(currentTimeMs)
        }

        // Update magnitude during sustain for dynamic/hybrid modes
        if (gateOpen && state == EnvelopeState.Sustain &&
            (triggerMode == TriggerMode.Dynamic || triggerMode == TriggerMode.Hybrid)
        ) {
            updateMagnitude(mag, currentTimeMs)
        }

        lastGateOpen = gateOpen

        // Process the envelope state machine (using frequency-adjusted params)
        return process(
            currentTimeMs,
            attackMs,
            decayMs,
            adjSustainLevel,
            adjReleaseMs,
            attackCurve,
            decayCurve,
            releaseCurve
        )
    }

    fun reset() {
        state = EnvelopeState.Idle
        value = 0f
        magnitude = 0f
        attackTarget = 1f
        lastTriggerTimeMs = 0f
        lastTriggerVelocity = 0f
        microPauseUntilMs = 0f
        nextMicroPauseMs = 0f
        silenceEvent = false
        lastMagUpdateMs = 0f
    }
}

/** Kotlin Float.pow extension for readability. */
private fun Float.pow(exp: Float): Float = Math.pow(this.toDouble(), exp.toDouble()).toFloat()
