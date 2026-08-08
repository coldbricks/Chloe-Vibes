package com.ashairfoil.chloevibes

import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/**
 * Pure dead-man unit tests for the primary audio→BLE path.
 * Patterned on [FunscriptVibrationSessionControllerTest.staleHeartbeatCannotExtendDeadman].
 */
class AudioPipelineWatchdogTest {
    @Test
    fun freshHeartbeatKeepsMotorsArmed() {
        val wd = AudioPipelineWatchdog()
        wd.arm(1_000L)
        assertTrue(wd.heartbeat(1_100L))
        assertFalse(wd.poll(1_100L))
        assertTrue(wd.heartbeat(2_500L))
        assertFalse(wd.poll(2_500L))
        assertTrue(wd.isArmed())
        assertFalse(wd.isTripped())
    }

    @Test
    fun staleHeartbeatTripsAtTwoSeconds() {
        val wd = AudioPipelineWatchdog()
        wd.arm(1_000L)
        assertTrue(wd.heartbeat(1_000L))

        // Just under timeout: still live.
        assertFalse(wd.poll(1_000L + AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS - 1L))

        // At timeout: trip and force zero.
        assertTrue(wd.poll(1_000L + AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS))
        assertTrue(wd.isTripped())
        // Sticky until re-arm.
        assertTrue(wd.poll(5_000L))
    }

    @Test
    fun staleHeartbeatCannotExtendDeadman() {
        val wd = AudioPipelineWatchdog()
        wd.arm(1_000L)
        assertTrue(wd.heartbeat(1_000L))

        // Trip the dead-man.
        assertTrue(wd.poll(1_000L + AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS))
        assertTrue(wd.isTripped())

        // A late / post-trip heartbeat must not clear or extend the lease.
        assertFalse(wd.heartbeat(10_000L))
        assertTrue(wd.poll(10_000L))
        assertTrue(wd.isTripped())
    }

    @Test
    fun clockRegressionFailsClosed() {
        val wd = AudioPipelineWatchdog()
        wd.arm(100L)
        assertTrue(wd.heartbeat(100L))
        assertFalse(wd.heartbeat(99L))
        assertTrue(wd.isTripped())
        assertTrue(wd.poll(100L))
    }

    @Test
    fun disarmClearsTripAndDisarmedPollIsQuiet() {
        val wd = AudioPipelineWatchdog()
        wd.arm(0L)
        assertTrue(wd.poll(AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS))
        wd.disarm()
        assertFalse(wd.isArmed())
        assertFalse(wd.isTripped())
        assertFalse(wd.poll(100_000L))
        assertFalse(wd.heartbeat(100_000L))
    }

    @Test
    fun acknowledgeTripStopsArmWithoutClearingStickyTrip() {
        val wd = AudioPipelineWatchdog()
        wd.arm(0L)
        assertTrue(wd.poll(AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS))
        wd.acknowledgeTrip()
        assertFalse(wd.isArmed())
        assertTrue(wd.isTripped())
        // poll remains true for UI sticky state until disarm/arm.
        assertTrue(wd.poll(9_000L))
    }

    @Test
    fun rearmAfterTripResumesNormalLease() {
        val wd = AudioPipelineWatchdog()
        wd.arm(0L)
        assertTrue(wd.poll(AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS))
        wd.arm(5_000L)
        assertTrue(wd.isArmed())
        assertFalse(wd.isTripped())
        assertTrue(wd.heartbeat(5_100L))
        assertFalse(wd.poll(5_100L))
    }

    @Test
    fun timeoutCeilingMatchesDesktopTwoSeconds() {
        assertTrue(AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS == 2_000L)
        assertThrows(IllegalArgumentException::class.java) {
            AudioPipelineWatchdog(2_001L)
        }
        assertThrows(IllegalArgumentException::class.java) {
            AudioPipelineWatchdog(0L)
        }
    }
}
