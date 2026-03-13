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

    var state: EnvelopeState = EnvelopeState.Idle
        private set

    /** Current envelope value (0.0 - 1.0). */
    var value: Float = 0f
        private set

    /** Value when current phase started. */
    private var phaseStartValue: Float = 0f

    /** Trigger magnitude (how hard the trigger was). */
    private var magnitude: Float = 0f

    /** Timestamp when current phase started (ms). */
    private var startTimeMs: Float = 0f

    /** Was the gate open last frame? */
    private var lastGateOpen: Boolean = false

    /** Minimum time between retriggers (ms). */
    private val minRetriggerMs: Float = 35f

    /** Time of last trigger (ms). */
    private var lastTriggerTimeMs: Float = 0f

    /** Trigger the envelope (gate just opened or strong onset detected). */
    fun trigger(magnitude: Float, currentTimeMs: Float, velocity: Float) {
        // Enforce minimum retrigger interval
        if (currentTimeMs - lastTriggerTimeMs < minRetriggerMs) return

        val scaledMagnitude = magnitude * (0.5f + 0.5f * velocity)
        this.magnitude = scaledMagnitude.coerceIn(0f, 1.5f)
        state = EnvelopeState.Attack
        startTimeMs = currentTimeMs
        // Start from current value, but ensure a minimum floor so the trigger
        // frame produces non-zero output.  Without this, the first BLE command
        // after a trigger is Vibrate:0 because value=0 and elapsed=0.  In
        // foreground the next frame arrives in ~16ms so it's imperceptible, but
        // when backgrounded Android throttles the thread and the follow-up
        // command may be delayed hundreds of milliseconds — leaving the device
        // silent through the entire attack phase.
        phaseStartValue = value.coerceAtLeast(0.4f)
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
     * changes while gate is held open).
     */
    fun updateMagnitude(newMagnitude: Float) {
        if (state == EnvelopeState.Sustain) {
            // Asymmetric smoothing: fast rise (feel the hit), slower fall (natural decay)
            val alpha = if (newMagnitude > magnitude) 0.30f else 0.15f
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

        when (state) {
            EnvelopeState.Attack -> {
                if (attackMs <= 0.5f) {
                    // Instant attack
                    value = 1f
                    state = EnvelopeState.Decay
                    startTimeMs = currentTimeMs
                    phaseStartValue = 1f
                } else {
                    val progress = (elapsed / attackMs).coerceIn(0f, 1f)
                    val curved = applyCurve(progress, attackCurve)
                    value = phaseStartValue + (1f - phaseStartValue) * curved

                    if (progress >= 1f) {
                        value = 1f
                        state = EnvelopeState.Decay
                        startTimeMs = currentTimeMs
                        phaseStartValue = 1f
                    }
                }
            }

            EnvelopeState.Decay -> {
                if (decayMs <= 0.5f) {
                    value = sustainLevel
                    state = EnvelopeState.Sustain
                } else {
                    val progress = (elapsed / decayMs).coerceIn(0f, 1f)
                    val decayFactor = applyCurve(1f - progress, decayCurve)
                    value = sustainLevel + (phaseStartValue - sustainLevel) * decayFactor

                    if (progress >= 1f) {
                        value = sustainLevel
                        state = EnvelopeState.Sustain
                    }
                }
            }

            EnvelopeState.Sustain -> {
                // Multi-layer modulation to prevent neural adaptation.
                // Total variation +/-25-35% keeps nerve endings sensitized.
                //   Primary: ~1.2Hz, +/-20% (slow, deep oscillation)
                //   Secondary: ~0.3Hz, +/-12% (breathing rhythm)
                //   Perlin-style noise: +/-8% (irrational-ratio sines prevent pattern lock)
                val primary = 0.20f * sin(currentTimeMs * 0.0075f)    // ~1.2Hz
                val secondary = 0.12f * sin(currentTimeMs * 0.0019f)  // ~0.3Hz
                val noise = 0.08f * (
                        sin(currentTimeMs * 0.00317f) * 0.5f +
                        sin(currentTimeMs * 0.00713f) * 0.3f +
                        sin(currentTimeMs * 0.01137f) * 0.2f
                )
                val modulation = 1f + primary + secondary + noise
                value = sustainLevel * modulation
            }

            EnvelopeState.Release -> {
                if (releaseMs <= 0.5f) {
                    value = 0f
                    state = EnvelopeState.Idle
                    magnitude = 0f
                } else {
                    val progress = (elapsed / releaseMs).coerceIn(0f, 1f)
                    val releaseFactor = applyCurve(1f - progress, releaseCurve)
                    value = phaseStartValue * releaseFactor

                    if (value <= 0.001f || progress >= 1f) {
                        value = 0f
                        state = EnvelopeState.Idle
                        magnitude = 0f
                    }
                }
            }

            EnvelopeState.Idle -> {
                value = (value * 0.95f).coerceAtLeast(0f) // Gentle fade
                if (value < 0.001f) value = 0f
                magnitude = 0f
            }
        }

        // Apply magnitude scaling
        return (value * magnitude).coerceIn(0f, 1f)
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
        releaseCurve: Float
    ): Float {
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

        // Onset retrigger: retrigger on onsets above threshold during sustain
        val isOnsetTrigger = isOnset && onsetStrength > 1.05f &&
                gateOpen && state == EnvelopeState.Sustain

        // Trigger logic
        if (gateJustOpened || isOnsetTrigger) {
            val velocity = if (isOnsetTrigger) {
                onsetStrength.coerceAtMost(1.35f)
            } else {
                1f
            }
            trigger(mag.coerceAtLeast(0.03f), currentTimeMs, velocity)
        } else if (gateOpen && state == EnvelopeState.Idle) {
            // Gate open but envelope idle -- retrigger
            trigger(mag.coerceAtLeast(0.03f), currentTimeMs, 1f)
        } else if (gateJustClosed) {
            release(currentTimeMs)
        }

        // Update magnitude during sustain for dynamic/hybrid modes
        if (gateOpen && state == EnvelopeState.Sustain &&
            (triggerMode == TriggerMode.Dynamic || triggerMode == TriggerMode.Hybrid)
        ) {
            updateMagnitude(mag)
        }

        lastGateOpen = gateOpen

        // Process the envelope state machine
        return process(
            currentTimeMs,
            attackMs,
            decayMs,
            sustainLevel,
            releaseMs,
            attackCurve,
            decayCurve,
            releaseCurve
        )
    }

    fun reset() {
        state = EnvelopeState.Idle
        value = 0f
        magnitude = 0f
    }
}

/** Kotlin Float.pow extension for readability. */
private fun Float.pow(exp: Float): Float = Math.pow(this.toDouble(), exp.toDouble()).toFloat()
