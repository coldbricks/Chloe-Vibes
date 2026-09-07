package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test

class SilentSystemAudioRecoveryTest {
    @Test fun continuingSilentCallbacksRecoverAtTheOld240FrameThreshold() {
        val recovery = SilentSystemAudioRecovery()
        repeat(239) { assertFalse(recovery.observe(it * 50L, true, false)) }
        assertTrue(recovery.observe(239 * 50L, true, false))
        assertEquals(1, recovery.attempts)
    }

    @Test fun heldFramesDoNotPretendToBeRepeatedSilentCallbacks() {
        val recovery = SilentSystemAudioRecovery(silentFramesBeforeRetry = 2)
        assertFalse(recovery.observe(0, true, false))
        repeat(500) { assertFalse(recovery.observe(it.toLong(), false, false)) }
        assertTrue(recovery.observe(1000, true, false))
    }

    @Test fun intentionalSilenceCannotCauseUnlimitedRestarts() {
        val recovery = SilentSystemAudioRecovery(silentFramesBeforeRetry = 1)
        assertTrue(recovery.observe(0, true, false))
        assertFalse(recovery.observe(4999, true, false))
        assertTrue(recovery.observe(5000, true, false))
        assertTrue(recovery.observe(10000, true, false))
        assertTrue(recovery.exhausted)
        repeat(500) { assertFalse(recovery.observe(20000 + it * 1000L, true, false)) }
        assertEquals(3, recovery.attempts)
    }

    @Test fun actualSignalRestoresTheRetryBudgetForTheNextCaptureFailure() {
        val recovery = SilentSystemAudioRecovery(silentFramesBeforeRetry = 1, maxRetries = 1)
        assertTrue(recovery.observe(0, true, false))
        assertFalse(recovery.observe(5000, true, false))
        assertFalse(recovery.observe(6000, true, true))
        assertEquals(0, recovery.attempts)
        assertTrue(recovery.observe(7000, true, false))
    }

    @Test fun manualRestartResetsAnExhaustedRun() {
        val recovery = SilentSystemAudioRecovery(silentFramesBeforeRetry = 1, maxRetries = 1)
        assertTrue(recovery.observe(0, true, false))
        recovery.reset()
        assertTrue(recovery.observe(1, true, false))
    }
}
