package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class AudioProcessingClockTest {
    @Test
    fun `short frame intervals retain precision after long device uptime`() {
        val origin = 40L * 24 * 60 * 60 * 1_000_000_000
        val clock = AudioProcessingClock(origin)
        assertEquals(12.345678f, clock.elapsedMs(origin + 12_345_678L), 0.00001f)
        var previous = 0f
        for (frame in 1..120) {
            val now = clock.elapsedMs(origin + frame * 16_666_667L)
            assertEquals(16.666667f, now - previous, 0.0002f)
            previous = now
        }
    }

    @Test
    fun `reset uses the processing epoch and runs once on the next frame`() {
        val origin = 30L * 24 * 60 * 60 * 1_000_000_000
        val clock = AudioProcessingClock(origin)
        val engine = ClimaxEngine()
        engine.reset(0f)
        val frame = origin + 40_000_000_000L
        val frameMs = clock.elapsedMs(frame)
        val previousPhase = engine.phaseProgress(frameMs, 60_000f)
        clock.requestClimaxReset()
        clock.requestClimaxReset()
        assertEquals(previousPhase, engine.phaseProgress(frameMs, 60_000f))
        assertTrue(clock.applyPendingClimaxReset(engine, frame))
        assertEquals(0f, engine.phaseProgress(frameMs, 60_000f))
        assertFalse(clock.applyPendingClimaxReset(engine, frame + 1_000_000_000L))
        assertEquals(1f / 60f, engine.phaseProgress(frameMs + 1000f, 60_000f), 0.00001f)
    }

    @Test
    fun `reset requested while capture is stopped survives until processing resumes`() {
        val origin = 123_000_000_000L
        val clock = AudioProcessingClock(origin)
        val engine = ClimaxEngine()
        engine.reset(clock.elapsedMs(origin + 10_000_000_000L))
        clock.requestClimaxReset()
        clock.onCaptureStartResult(false)
        // No processing frames occur during this five-minute capture pause.
        val resumedFrame = origin + 310_000_000_000L
        assertEquals(310_000f, clock.elapsedMs(resumedFrame))
        assertTrue(clock.applyPendingClimaxReset(engine, resumedFrame))
        assertEquals(0f, engine.phaseProgress(clock.elapsedMs(resumedFrame), 60_000f))
        assertEquals(0.5f,
            engine.phaseProgress(clock.elapsedMs(resumedFrame + 30_000_000_000L), 60_000f), 0.00001f)
        assertFalse(clock.applyPendingClimaxReset(engine, resumedFrame + 30_000_000_000L))
    }

    @Test
    fun `successful capture restart resets cycle but a failed start cannot request or consume a reset`() {
        val origin = 9_000_000_000L
        val clock = AudioProcessingClock(origin)
        val engine = ClimaxEngine()
        engine.reset(0f)
        val stoppedAt = origin + 20_000_000_000L
        clock.onCaptureStartResult(false)
        assertFalse(clock.applyPendingClimaxReset(engine, stoppedAt))
        assertEquals(1f / 3f, engine.phaseProgress(clock.elapsedMs(stoppedAt), 60_000f), 0.00001f)
        // A successful restart after a long pause queues a reset on this same epoch.
        val resumedFrame = origin + 3_020_000_000_000L
        clock.onCaptureStartResult(true)
        assertTrue(clock.applyPendingClimaxReset(engine, resumedFrame))
        assertEquals(0f, engine.phaseProgress(clock.elapsedMs(resumedFrame), 60_000f))
        assertEquals(1f / 60f,
            engine.phaseProgress(clock.elapsedMs(resumedFrame + 1_000_000_000L), 60_000f), 0.00001f)
        assertFalse(clock.applyPendingClimaxReset(engine, resumedFrame + 1_000_000_000L))
    }
}
