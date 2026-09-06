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
