package com.ashairfoil.chloevibes

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class FunscriptVibrationSessionControllerTest {
    @Test
    fun hereSphereFallbackIsExplicitlyInvertedFromFunscriptPosition() {
        assertEquals(1f, HereSphereInversePositionScalarFallback.intensity(0), 0f)
        assertEquals(0.5f, HereSphereInversePositionScalarFallback.intensity(50_000), 0f)
        assertEquals(0f, HereSphereInversePositionScalarFallback.intensity(100_000), 0f)
        assertThrows(IllegalArgumentException::class.java) {
            HereSphereInversePositionScalarFallback.intensity(100_001)
        }
    }

    @Test
    fun rampInterpolatesAtTwentyHertzAndEmitsTerminalPosition() {
        val controller = adoptedController(nowMs = 1_000L)
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 100_000, 0, 1_000L, 1_000L)
        )

        assertPosition(controller.poll(1_000L), 100_000)
        assertPosition(controller.poll(1_500L), 50_000)
        assertPosition(controller.poll(2_000L), 0)
        assertNull(controller.poll(2_050L))
        assertEquals(50L, CompanionSessionController.RAMP_INTERVAL_MS)
    }

    @Test
    fun zeroDurationMoveJumpsDirectlyToTarget() {
        val controller = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 20_000, 75_000, 0L, 0L)
        )

        assertPosition(controller.poll(0L), 75_000)
        assertNull(controller.poll(50L))
    }

    @Test
    fun sparseLongMaxSegmentRemainsMediaClockAccurate() {
        val controller = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 100_000, 0, Long.MAX_VALUE, 0L)
        )

        assertTrue(controller.heartbeat(SESSION, GENERATION, Long.MAX_VALUE / 2L))
        assertPosition(controller.poll(Long.MAX_VALUE / 2L), 50_000)
    }

    @Test
    fun newGenerationFencesPriorRampAndRejectsStaleWork() {
        val controller = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 0, 100_000, 1_000L, 0L)
        )

        assertTrue(controller.adoptGenerationAndStop(SESSION, GENERATION + 1L, 10L))
        assertEquals(
            CompanionMoveResult.Stale,
            controller.move(SESSION, GENERATION, 0, 100_000, 100L, 10L)
        )
        assertFalse(controller.stop(SESSION, GENERATION, 10L))
        assertNull(controller.poll(10L))
    }

    @Test
    fun stopCancelsRampButAllowsResumeInCurrentGeneration() {
        val controller = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 100_000, 0, 1_000L, 0L)
        )
        assertTrue(controller.stop(SESSION, GENERATION, 250L))
        assertNull(controller.poll(250L))

        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 50_000, 0, 500L, 500L)
        )
        assertPosition(controller.poll(750L), 25_000)
    }

    @Test
    fun releaseClearsOwnershipAndRejectsLateWork() {
        val controller = adoptedController()

        assertTrue(controller.release(SESSION, GENERATION))
        assertEquals(
            CompanionMoveResult.Stale,
            controller.move(SESSION, GENERATION, 0, 100_000, 100L, 0L)
        )
        assertNull(controller.poll(0L))
    }

    @Test
    fun staleHeartbeatCannotExtendDeadman() {
        val controller = adoptedController(nowMs = 1_000L)

        assertFalse(controller.heartbeat(SESSION, GENERATION - 1L, 2_000L))
        val expired = controller.poll(
            1_000L + CompanionSessionController.DEFAULT_HEARTBEAT_TIMEOUT_MS
        )
        assertTrue(expired?.shouldRelease == true)
        assertEquals(SESSION, expired?.sessionId)
        assertNull(controller.poll(3_000L))
        assertTrue(CompanionSessionController.DEFAULT_HEARTBEAT_TIMEOUT_MS <= 2_000L)
    }

    @Test
    fun rampClockRegressionFailsClosed() {
        val controller = adoptedController(nowMs = 100L)
        assertEquals(
            CompanionMoveResult.Accepted,
            controller.move(SESSION, GENERATION, 0, 100_000, 1_000L, 100L)
        )

        val failedClosed = controller.poll(99L)
        assertTrue(failedClosed?.shouldRelease == true)
        assertNull(controller.poll(100L))
    }

    @Test
    fun invalidSessionPositionsDurationsAndTimeoutAreRejected() {
        val controller = CompanionSessionController()
        assertThrows(IllegalArgumentException::class.java) { controller.arm(0L, 0L) }
        controller.arm(SESSION, 0L)
        assertEquals(
            CompanionMoveResult.Stale,
            controller.move(SESSION, Long.MIN_VALUE, 0, 100_000, 1L, 0L)
        )

        assertEquals(
            CompanionMoveResult.Invalid,
            adoptedController().move(SESSION, GENERATION, -1, 0, 1L, 0L)
        )
        assertEquals(
            CompanionMoveResult.Invalid,
            adoptedController().move(SESSION, GENERATION, 0, 100_001, 1L, 0L)
        )
        assertEquals(
            CompanionMoveResult.Invalid,
            adoptedController().move(
                SESSION,
                GENERATION,
                0,
                100_000,
                CompanionSessionController.MAX_RAMP_DURATION_MS + 1L,
                0L
            )
        )
        assertThrows(IllegalArgumentException::class.java) {
            CompanionSessionController(2_001L)
        }
    }

    @Test
    fun malformedCurrentMoveFailsClosedButMalformedStaleMoveIsIgnored() {
        val current = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            current.move(SESSION, GENERATION, 100_000, 0, 1_000L, 0L)
        )
        assertEquals(
            CompanionMoveResult.Invalid,
            current.move(SESSION, GENERATION, 100_001, 0, 1_000L, 10L)
        )
        assertNull(current.poll(500L))

        val stale = adoptedController()
        assertEquals(
            CompanionMoveResult.Accepted,
            stale.move(SESSION, GENERATION, 100_000, 0, 1_000L, 0L)
        )
        assertEquals(
            CompanionMoveResult.Stale,
            stale.move(SESSION, GENERATION - 1L, 100_001, 0, 1_000L, 10L)
        )
        assertPosition(stale.poll(500L), 50_000)
    }

    private fun adoptedController(nowMs: Long = 0L): CompanionSessionController =
        CompanionSessionController().also {
            it.arm(SESSION, nowMs)
            assertTrue(it.adoptGenerationAndStop(SESSION, GENERATION, nowMs))
        }

    private fun assertPosition(output: CompanionOutput?, positionMilli: Int) {
        assertEquals(SESSION, output?.sessionId)
        assertEquals(positionMilli, output?.positionMilli)
        assertFalse(output?.shouldRelease ?: true)
    }

    companion object {
        private const val SESSION = 41L
        private const val GENERATION = 7L
    }
}
