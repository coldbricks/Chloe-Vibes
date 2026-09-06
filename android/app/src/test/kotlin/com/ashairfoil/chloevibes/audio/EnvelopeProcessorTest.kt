package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class EnvelopeProcessorTest {
    @Test
    fun `first trigger starts immediately and reset rearms gate`() {
        val envelope = EnvelopeProcessor()
        assertTrue(drive(envelope, 0f) > 0f)
        assertTrue(envelope.triggeredAt(0f))
        envelope.reset()
        assertTrue(drive(envelope, 0f) > 0f)
        assertEquals(EnvelopeState.Decay, envelope.state)
    }

    @Test
    fun `sparse processing carries time through attack decay and release`() {
        val dense = EnvelopeProcessor()
        val sparse = EnvelopeProcessor()
        dense.trigger(1f, 100f, 1f, 80f)
        sparse.trigger(1f, 100f, 1f, 80f)
        for (now in 100..345 step 5) process(dense, now.toFloat())
        val denseOutput = process(dense, 350f)
        val sparseOutput = process(sparse, 350f)
        assertEquals(0.05f, denseOutput, 0.000001f)
        assertEquals(denseOutput, sparseOutput, 0.000001f)
        assertEquals(EnvelopeState.Release, sparse.state)
        assertEquals(0f, process(sparse, 400f), 0f)
        assertTrue(sparse.silenceEvent)
    }

    @Test
    fun `retrigger acknowledgment follows actual cooldown acceptance`() {
        val envelope = EnvelopeProcessor()
        envelope.trigger(1f, 100f, 1.2f, 20f)
        assertTrue(envelope.triggeredAt(100f))
        envelope.trigger(1f, 110f, 1.2f, 20f)
        assertFalse(envelope.triggeredAt(110f))
        envelope.trigger(1f, 130f, 1.2f, 20f)
        assertTrue(envelope.triggeredAt(130f))
    }

    @Test
    fun `invalid envelope frame stops and next valid trigger recovers`() {
        val envelope = EnvelopeProcessor()
        envelope.trigger(1f, 100f, 1f, 20f)
        assertEquals(0f, process(envelope, Float.NaN), 0f)
        assertTrue(envelope.silenceEvent)
        assertTrue(drive(envelope, 200f) > 0f)
    }

    @Test
    fun `played prefire cannot attack again when matching real onset reopens gate`() {
        val beat = lockedDetector()
        val envelope = EnvelopeProcessor()
        val strength = requireNotNull(beat.takePrefire(2950f))
        val predictedPeak = driveOnset(envelope, 2950f, true, true, strength, false)
        assertTrue(envelope.triggeredAt(2950f))
        beat.confirmPrefire(2950f)
        driveOnset(envelope, 2970f, false, false, 0f, false)
        assertEquals(EnvelopeState.Release, envelope.state)

        val (real, realStrength) = beat.process(10f, 3000f)
        val played = real && beat.isPrefiredOnset(3000f)
        assertTrue(played)
        val actual = driveOnset(envelope, 3000f, true, real && !played, realStrength, played)
        assertFalse(envelope.triggeredAt(3000f), "gate reopening must not duplicate a played beat")
        assertTrue(actual < predictedPeak)
        assertEquals(EnvelopeState.Release, envelope.state)
        driveOnset(envelope, 3010f, true, false, 0f, false)
        assertFalse(envelope.triggeredAt(3010f), "suppressed gate edge must still be consumed")

        val (separate, separateStrength) = beat.process(10f, 3060f)
        val separatePlayed = separate && beat.isPrefiredOnset(3060f)
        assertTrue(separate && !separatePlayed)
        driveOnset(envelope, 3060f, true, separate, separateStrength, separatePlayed)
        assertTrue(envelope.triggeredAt(3060f))
        driveOnset(envelope, 3070f, false, false, 0f, true)
        assertEquals(EnvelopeState.Release, envelope.state)
        assertEquals(0f, driveOnset(envelope, 3500f, false, false, 0f, false), 0f)
        assertTrue(envelope.silenceEvent)
    }

    @Test
    fun `closed gate or cooldown rejected prefire leaves real attack playable`() {
        for (gateWasOpen in listOf(false, true)) {
            val beat = lockedDetector()
            val envelope = EnvelopeProcessor()
            if (gateWasOpen) driveOnset(envelope, 2945f, true, true, 1.35f, false)
            val strength = requireNotNull(beat.takePrefire(2950f))
            driveOnset(envelope, 2950f, gateWasOpen, true, strength, false)
            assertFalse(envelope.triggeredAt(2950f))
            val (real, realStrength) = beat.process(10f, 3000f)
            val played = real && beat.isPrefiredOnset(3000f)
            assertTrue(real && !played)
            driveOnset(envelope, 3000f, true, real, realStrength, played)
            assertTrue(envelope.triggeredAt(3000f))
        }
    }

    private fun lockedDetector() = BeatDetector().also { beat ->
        for (time in listOf(1000f, 1500f, 2000f, 2500f)) {
            assertTrue(beat.process(10f, time).first)
        }
    }

    private fun driveOnset(
        envelope: EnvelopeProcessor,
        time: Float,
        gate: Boolean,
        onset: Boolean,
        strength: Float,
        alreadyPlayed: Boolean
    ) = envelope.drive(
        gateOpen = gate, energy = if (gate) 0.8f else 0f,
        isOnset = onset, onsetStrength = strength, currentTimeMs = time,
        triggerMode = TriggerMode.Hybrid, threshold = 0.14f,
        thresholdKnee = 0.15f, dynamicCurve = 1.2f, binaryLevel = 0.82f,
        hybridBlend = 0.58f, attackMs = 20f, decayMs = 375f,
        sustainLevel = 0.08f, releaseMs = 240f, attackCurve = 0.7f,
        decayCurve = 1.8f, releaseCurve = 1.3f, spectralCentroid = 100f,
        onsetAlreadyPlayed = alreadyPlayed
    )

    private fun process(envelope: EnvelopeProcessor, time: Float) =
        envelope.process(time, 80f, 120f, 0.1f, 100f, 1f, 1f, 1f)

    private fun drive(envelope: EnvelopeProcessor, time: Float) = envelope.drive(
        gateOpen = true, energy = 1f, isOnset = false, onsetStrength = 1f,
        currentTimeMs = time, triggerMode = TriggerMode.Binary,
        threshold = 0.1f, thresholdKnee = 0f, dynamicCurve = 1f,
        binaryLevel = 1f, hybridBlend = 0f, attackMs = 20f, decayMs = 120f,
        sustainLevel = 0.1f, releaseMs = 100f, attackCurve = 1f,
        decayCurve = 1f, releaseCurve = 1f, spectralCentroid = 100f
    )
}
