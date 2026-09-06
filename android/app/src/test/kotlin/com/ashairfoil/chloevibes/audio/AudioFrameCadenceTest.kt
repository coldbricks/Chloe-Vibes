package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class AudioFrameCadenceTest {
    @Test
    fun slowerCaptureIsAnalyzedOnceAndHeldBetweenCallbacks() {
        val cadence = AudioFrameCadence()
        cadence.received(1000)
        assertEquals(AudioFrameCadence.Frame.Fresh, cadence.poll(1000))
        assertEquals(AudioFrameCadence.Frame.Held, cadence.poll(1016))
        assertEquals(AudioFrameCadence.Frame.Held, cadence.poll(1032))
        // An identical-valued later callback is still a genuine new frame.
        cadence.received(1048)
        assertEquals(AudioFrameCadence.Frame.Fresh, cadence.poll(1048))
    }

    @Test
    fun staleInputBecomesMissingAndCannotKeepOutputAlive() {
        val cadence = AudioFrameCadence()
        cadence.received(1000)
        assertEquals(AudioFrameCadence.Frame.Fresh, cadence.poll(1000))
        assertEquals(AudioFrameCadence.Frame.Held, cadence.poll(1120))
        assertEquals(AudioFrameCadence.Frame.Missing, cadence.poll(1121))
        assertEquals(AudioFrameCadence.Frame.Missing, cadence.poll(9000))
        cadence.received(9001)
        assertEquals(AudioFrameCadence.Frame.Fresh, cadence.poll(9001))
    }

    @Test
    fun delayedFirstReadAndClockRegressionFailClosed() {
        val cadence = AudioFrameCadence()
        cadence.received(1000)
        assertEquals(AudioFrameCadence.Frame.Missing, cadence.poll(1121))
        cadence.received(2000)
        assertEquals(AudioFrameCadence.Frame.Missing, cadence.poll(1999))
    }

    @Test
    fun stopClearsHeldAndUnconsumedInput() {
        val cadence = AudioFrameCadence()
        cadence.received(1000)
        cadence.clear()
        assertEquals(AudioFrameCadence.Frame.Missing, cadence.poll(1001))
    }

    @Test
    fun secondaryMotorContinuesAndReceivesItsFinalZero() {
        val dispatch = AudioOutputDispatch()
        assertFalse(dispatch.shouldSend(0f, 0f))
        assertTrue(dispatch.shouldSend(0.8f, 0.6f))
        assertTrue(dispatch.shouldSend(0f, 0.4f))
        assertTrue(dispatch.shouldSend(0f, 0.2f))
        assertTrue(dispatch.shouldSend(0f, 0f))
        assertFalse(dispatch.shouldSend(0f, 0f))
    }

    @Test
    fun gainCannotExceedChosenOutputCeiling() {
        assertEquals(0.2f, mapOutput(1f, 0f, 0.2f, 4f, false))
        assertEquals(0f, mapOutput(1f, 0.1f, 0.2f, 4f, true))
        assertEquals(0f, mapOutput(0f, 0.1f, 0.2f, 4f, false))
        assertEquals(0.6f, mapOutput(1.2f, 0f, 1f, 0.5f, false))
    }

    @Test
    fun invalidOutputParametersDoNotReachMotors() {
        assertEquals(0f, mapOutput(Float.NaN, 0f, 1f, 1f, false))
        assertEquals(0f, mapOutput(1f, 0f, Float.POSITIVE_INFINITY, 1f, false))
        assertEquals(0f, mapOutput(1f, 0f, 1f, Float.NaN, false))
        assertEquals(0f, mapOutput(1f, 0f, 1f, -1f, false))
        assertEquals(0.2f, mapOutput(1f, 0.9f, 0.2f, 1f, false))
    }
}
