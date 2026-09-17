package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test
import kotlin.math.abs

class BeatDetectorTest {
    @Test
    fun `manual tempo requires real audio expires in silence and returns to auto`() {
        val detector = BeatDetector()
        detector.setManualTempo(125f)
        assertNull(detector.takePrefire(950f))
        assertTrue(detector.process(10f, 1000f).first)
        assertEquals(480f, detector.tempoIntervalMs)
        assertEquals(1480f, detector.predictedNextOnsetMs)
        detector.setManualTempo(125f)
        assertNotNull(detector.takePrefire(1430f))
        assertNull(detector.takePrefire(1430f))
        detector.advanceTime(2440f)
        assertNull(detector.takePrefire(2440f))
        assertEquals(0f, detector.tempoConfidence)
        assertTrue(detector.process(10f, 2500f).first)
        assertEquals(480f, detector.tempoIntervalMs)
        detector.setManualTempo(null)
        assertEquals(0f, detector.predictedNextOnsetMs)
        for (now in listOf(3000f, 3500f, 4000f, 4500f)) assertTrue(detector.process(10f, now).first)
        assertEquals(500f, detector.tempoIntervalMs)
    }

    @Test
    fun `four taps reject bounce and jitter then restart after a pause`() {
        val tap = TapTempo()
        for (time in listOf(1000.0, 1480.0, 1500.0, 1965.0)) tap.tap(time)
        assertEquals(3, tap.count)
        assertNull(tap.bpm)
        tap.tap(2440.0)
        assertEquals(125f, tap.bpm)
        tap.tap(Double.NaN)
        assertEquals(125f, tap.bpm)
        tap.tap(6000.0)
        assertEquals(1, tap.count)
        assertNull(tap.bpm)
        for (time in listOf(6600.0, 7200.0, 7800.0)) tap.tap(time)
        assertEquals(100f, tap.bpm)
        tap.reset()
        assertEquals(0, tap.count)
        assertNull(tap.bpm)
    }
    @Test
    fun `confidence depends on elapsed time not polling frequency`() {
        val dense = lockedDetector()
        val sparse = lockedDetector()
        for (now in 2510..3490 step 10) dense.process(0f, now.toFloat())
        dense.process(0f, 3500f)
        sparse.process(0f, 3500f)
        assertEquals(sparse.tempoConfidence, dense.tempoConfidence, 0.000001f)
        val before = dense.tempoConfidence
        repeat(20) { dense.advanceTime(3500f) }
        assertEquals(before, dense.tempoConfidence, 0f)
    }

    @Test
    fun `nonfinite flux cannot poison subsequent real onsets`() {
        val detector = BeatDetector()
        assertEquals(false to 0f, detector.process(Float.NaN, 100f))
        assertEquals(false to 0f, detector.process(Float.POSITIVE_INFINITY, 150f))
        assertTrue(detector.process(10f, 200f).first)
    }

    @Test
    fun `confirmed prefire matches one real onset and leaves separate flam intact`() {
        val detector = lockedDetector()
        assertNotNull(detector.takePrefire(2950f))
        detector.confirmPrefire(2950f)
        assertTrue(detector.process(10f, 2960f).first)
        assertTrue(detector.isPrefiredOnset(2960f))
        assertTrue(detector.process(10f, 3020f).first)
        assertFalse(detector.isPrefiredOnset(3020f))
    }

    @Test
    fun `unplayed prediction leaves real onset intact`() {
        val detector = lockedDetector()
        assertNotNull(detector.takePrefire(2950f))
        assertTrue(detector.process(10f, 3000f).first)
        assertFalse(detector.isPrefiredOnset(3000f))
    }

    @Test
    fun `advancing stale time requires relearning a fresh tempo`() {
        val detector = lockedDetector()
        for (now in 2505..4000 step 5) detector.advanceTime(now.toFloat())
        assertEquals(0f, detector.tempoConfidence, 0f)
        assertTrue(detector.process(10f, 4500f).first)
        assertEquals(0f, detector.predictedNextOnsetMs, 0f)
    }

    @Test
    fun `quiet process frames cannot resurrect a consumed prediction`() {
        val detector = lockedDetector()
        assertEquals(3000f, detector.predictedNextOnsetMs, .001f)
        var prefires = 0
        for (time in 2940..2995 step 5) {
            assertFalse(detector.process(0f, time.toFloat()).first)
            detector.takePrefire(time.toFloat())?.let { strength ->
                prefires++
                assertTrue(strength in 1.15f..1.35f)
            }
            if (time >= 2950) assertEquals(3500f, detector.predictedNextOnsetMs, .001f)
        }
        assertEquals(1, prefires)
        assertFalse(detector.prefireOk(2995f))
    }

    @Test
    fun `later real beats still receive one predictive onset each`() {
        val detector = lockedDetector()
        for (onset in 3000..5500 step 500) {
            assertFalse(detector.process(0f, onset - 50f).first)
            assertNotNull(detector.takePrefire(onset - 50f))
            assertFalse(detector.process(0f, onset - 25f).first)
            assertNull(detector.takePrefire(onset - 25f))
            assertTrue(detector.process(10f, onset.toFloat()).first)
            assertEquals(onset + 500f, detector.predictedNextOnsetMs, .001f)
        }
    }

    @Test
    fun `real onset can retime the next beat after a prefire`() {
        val detector = lockedDetector()
        assertNotNull(detector.takePrefire(2950f))
        assertEquals(3500f, detector.predictedNextOnsetMs, .001f)
        assertTrue(detector.process(10f, 2980f).first)
        val retimed = detector.predictedNextOnsetMs
        assertTrue(retimed > 3400f && retimed < 3500f)
        assertFalse(detector.process(0f, retimed - 50f).first)
        assertNotNull(detector.takePrefire(retimed - 50f))
        assertFalse(detector.process(0f, retimed - 25f).first)
        assertNull(detector.takePrefire(retimed - 25f))
    }

    @Test
    fun `stale lock clears and must learn four fresh onsets before predicting again`() {
        val detector = lockedDetector()
        assertNotNull(detector.takePrefire(2950f))
        assertFalse(detector.process(0f, 10000f).first)
        assertEquals(0f, detector.tempoConfidence, 0f)
        assertEquals(0f, detector.tempoIntervalMs, 0f)
        assertEquals(0f, detector.predictedNextOnsetMs, 0f)
        assertNull(detector.takePrefire(10000f))
        for (time in intArrayOf(10050, 10550, 11050)) {
            assertTrue(detector.process(10f, time.toFloat()).first)
            assertEquals(0f, detector.predictedNextOnsetMs, 0f)
        }
        assertTrue(detector.process(10f, 11550f).first)
        assertEquals(12050f, detector.predictedNextOnsetMs, .001f)
        assertFalse(detector.process(0f, 12000f).first)
        assertNotNull(detector.takePrefire(12000f))
    }

    @Test
    fun `fresh detector has an independent prediction epoch`() {
        val old = lockedDetector()
        assertNotNull(old.takePrefire(2950f))
        val fresh = BeatDetector()
        assertNull(fresh.takePrefire(0f))
        assertEquals(0f, fresh.predictedNextOnsetMs, 0f)
        for (time in intArrayOf(100, 600, 1100, 1600)) {
            assertTrue(fresh.process(10f, time.toFloat()).first)
        }
        assertEquals(2100f, fresh.predictedNextOnsetMs, .001f)
        assertFalse(fresh.process(0f, 2050f).first)
        assertNotNull(fresh.takePrefire(2050f))
        assertFalse(fresh.process(0f, 2060f).first)
        assertNull(fresh.takePrefire(2060f))
        assertEquals(3500f, old.predictedNextOnsetMs, .001f)
    }

    @Test
    fun `syncopated and subdivided rhythms lock with perceptual beat and prefire`() {
        val detector = BeatDetector()
        // Warm up the 43-frame flux history with baseline noise
        for (i in 0 until 50) {
            detector.process(0.02f, i * 20f)
        }
        detector.clearTempoLock()

        val pattern = listOf(
            1000f to 10f, // kick
            1250f to 4f,  // hat (8th note)
            1500f to 10f, // kick/snare
            1750f to 4f,  // hat
            2000f to 10f, // kick
            2250f to 4f,  // hat
            2500f to 10f, // kick/snare
            2750f to 4f,  // hat
            3000f to 10f, // kick
            3250f to 4f,  // hat
            3500f to 10f  // kick/snare
        )

        var t = 1000f
        val dt = 25f
        var patternIdx = 0
        var onsetsDetected = 0

        while (t <= 3600f) {
            var flux = 0.02f
            if (patternIdx < pattern.size && abs(t - pattern[patternIdx].first) < 1f) {
                flux = pattern[patternIdx].second
                patternIdx++
            }
            val (isOnset, _) = detector.process(flux, t)
            if (isOnset) {
                onsetsDetected++
            }
            t += dt
        }

        assertEquals(pattern.size, patternIdx, "all pattern items should be fed")
        assertTrue(onsetsDetected >= 8, "most pattern onsets should be detected, got $onsetsDetected")

        // Perceptual folding should recognize the 500ms beat grid:
        // Confidence must be high (>= 0.70) instead of being crippled to 0.0 by raw CV.
        assertTrue(
            detector.tempoConfidence >= 0.70f,
            "expected high tempo confidence on syncopated music, got ${detector.tempoConfidence}"
        )
        assertEquals(500f, detector.tempoIntervalMs, 10f)

        // Next quarter note is at 4000.0.
        // At 3955.0 (45ms before downbeat), predictive prefire MUST engage!
        assertTrue(
            detector.prefireOk(3955f),
            "prefireOk should be true 45ms before next beat, next=${detector.predictedNextOnsetMs}"
        )
        val prefire = detector.takePrefire(3955f)
        assertNotNull(prefire, "prefire should trigger for quarter-note beat")
    }

    private fun lockedDetector() = BeatDetector().also { detector ->
        for (time in intArrayOf(1000, 1500, 2000, 2500)) {
            assertTrue(detector.process(10f, time.toFloat()).first)
        }
        assertEquals(500f, detector.tempoIntervalMs, .001f)
        assertTrue(detector.tempoConfidence > .6f)
    }
}

