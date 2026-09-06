package com.ashairfoil.chloevibes.device

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test

class BleCommandQueueTest {
    @Test
    fun nextKickCannotEraseQueuedRest() {
        val queue = BleCommandQueue()
        queue.offer(LovenseProtocol.vibrate(15))
        queue.offer(LovenseProtocol.stop())
        queue.offer(LovenseProtocol.vibrate(18))
        assertEquals(LovenseProtocol.stop(), queue.take())
        assertEquals(LovenseProtocol.vibrate(18), queue.take())
        assertNull(queue.take())
    }

    @Test
    fun deviceDiscoveryCannotBeOverwrittenByAudio() {
        val queue = BleCommandQueue()
        queue.offer(LovenseProtocol.deviceType())
        queue.offer(LovenseProtocol.battery())
        queue.offer(LovenseProtocol.vibrate(4))
        queue.offer(LovenseProtocol.vibrate(14))
        assertEquals(LovenseProtocol.deviceType(), queue.take())
        assertEquals(LovenseProtocol.battery(), queue.take())
        assertEquals(LovenseProtocol.vibrate(14), queue.take())
    }

    @Test
    fun failedOldIntensityDoesNotReplaceNewerIntent() {
        val queue = BleCommandQueue()
        queue.offer(LovenseProtocol.vibrate(4))
        queue.retry(LovenseProtocol.vibrate(18))
        assertEquals(LovenseProtocol.vibrate(4), queue.take())
        queue.offer(LovenseProtocol.stop())
        queue.retry(LovenseProtocol.vibrate(18))
        assertEquals(LovenseProtocol.stop(), queue.take())
        assertNull(queue.take())
    }

    @Test
    fun failedStopRetriesAheadOfNewIntensity() {
        val queue = BleCommandQueue()
        queue.offer(LovenseProtocol.vibrate(12))
        queue.retry(LovenseProtocol.stop())
        assertEquals(LovenseProtocol.stop(), queue.take())
        assertEquals(LovenseProtocol.vibrate(12), queue.take())
    }

    @Test
    fun reconnectDropsOldSessionCommandsAndDeduplicatesDiscovery() {
        val queue = BleCommandQueue()
        queue.offer(LovenseProtocol.deviceType())
        queue.offer(LovenseProtocol.deviceType())
        assertEquals(LovenseProtocol.deviceType(), queue.take())
        assertNull(queue.take())
        queue.offer(LovenseProtocol.vibrate(20))
        queue.offer(LovenseProtocol.stop())
        queue.clear()
        assertNull(queue.peek())
    }
}
