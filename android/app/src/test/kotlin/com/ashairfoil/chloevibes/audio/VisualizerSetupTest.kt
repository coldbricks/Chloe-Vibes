package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test

class VisualizerSetupTest {
    private class Target(
        val failedStage: String? = null,
        val thrownStage: String? = null,
        val throwDuringCleanup: Boolean = false,
    ) : VisualizerSetupTarget {
        val calls = mutableListOf<String>()
        var published = false
        private fun stage(name: String): Int {
            calls += name
            if (name == thrownStage) throw IllegalStateException("fixture")
            return if (name == failedStage) -5 else 0
        }
        override fun configureCaptureSize() = stage("capture size")
        override fun registerCaptureListener() = stage("capture listener")
        override fun enableCapture(): Int {
            published = true
            return stage("enable")
        }
        override fun detachOwner() {
            calls += "detach"
            published = false
            if (throwDuringCleanup) throw IllegalStateException("detach fixture")
        }
        override fun disableCapture() {
            calls += "disable"
            if (throwDuringCleanup) throw IllegalStateException("disable fixture")
        }
        override fun release() { calls += "release" }
    }

    @Test fun allThreeAcceptedStagesTransferOwnershipWithoutReleasing() {
        val target = Target()
        val result = initializeVisualizer(0) { target }
        assertTrue(result.started)
        assertNull(result.failureMessage)
        assertTrue(target.published)
        assertEquals(listOf("capture size", "capture listener", "enable"), target.calls)
    }

    @Test fun nonThrowingPlatformErrorsStopSetupAndReleaseEveryAllocatedSession() {
        val stages = listOf("capture size", "capture listener", "enable")
        for ((index, stage) in stages.withIndex()) {
            val target = Target(failedStage = stage)
            val result = initializeVisualizer(0) { target }
            assertFalse(result.started, stage)
            assertEquals("System audio setup failed at $stage (status -5)", result.failureMessage)
            assertEquals(stages.take(index + 1) + listOf("detach", "disable", "release"), target.calls)
            assertFalse(target.published)
        }
    }

    @Test fun thrownPlatformFailuresKeepTheirStageAndCauseAndStillRelease() {
        for (stage in listOf("capture size", "capture listener", "enable")) {
            val target = Target(thrownStage = stage)
            val result = initializeVisualizer(0) { target }
            assertFalse(result.started)
            assertEquals("System audio setup failed at $stage (IllegalStateException)", result.failureMessage)
            assertEquals("fixture", result.cause?.message)
            assertEquals(listOf("detach", "disable", "release"), target.calls.takeLast(3))
            assertFalse(target.published)
        }
    }

    @Test fun failedConstructorReportsCreateWithoutPretendingThereIsAResourceToRelease() {
        val result = initializeVisualizer(0) { throw UnsupportedOperationException("unavailable") }
        assertFalse(result.started)
        assertEquals("System audio setup failed at create (UnsupportedOperationException)", result.failureMessage)
        assertEquals("unavailable", result.cause?.message)
    }

    @Test fun cleanupExceptionsDoNotMaskTheSetupStatusOrSkipRelease() {
        val target = Target(failedStage = "enable", throwDuringCleanup = true)
        val result = initializeVisualizer(0) { target }
        assertFalse(result.started)
        assertEquals("System audio setup failed at enable (status -5)", result.failureMessage)
        assertEquals(listOf("detach", "disable", "release"), target.calls.takeLast(3))
    }

    @Test fun releasedFailureDoesNotPreventASeparateSuccessfulRetry() {
        val first = Target(failedStage = "capture listener")
        val second = Target()
        assertFalse(initializeVisualizer(0) { first }.started)
        assertTrue(initializeVisualizer(0) { second }.started)
        assertEquals(1, first.calls.count { it == "release" })
        assertEquals(0, second.calls.count { it == "release" })
    }
}
