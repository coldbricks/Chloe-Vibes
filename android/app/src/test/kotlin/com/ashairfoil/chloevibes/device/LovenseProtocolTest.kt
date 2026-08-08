package com.ashairfoil.chloevibes.device

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

/**
 * Motor-feel honesty: rest floors / gammas lockstep with desktop MotorKind,
 * stop detection, and DeviceType / advertised-name mapping.
 */
class LovenseProtocolTest {

    @Test
    fun wandRestFloorHigherThanLegacyHangFloor() {
        // Domi hang fix used 0.018; true-zero rest prefers a higher floor.
        assertTrue(LovenseProtocol.MotorFeel.Wand.restFloor >= 0.024f)
        assertTrue(LovenseProtocol.MotorFeel.Wand.feelGamma >= 1.6f)
    }

    @Test
    fun deviceTypeMapsNoraToDualBody() {
        assertEquals(
            LovenseProtocol.MotorFeel.DualBody,
            LovenseProtocol.MotorFeel.fromDeviceTypeId("A")
        )
        assertEquals(
            LovenseProtocol.MotorFeel.DualBody,
            LovenseProtocol.MotorFeel.fromDeviceTypeId("C")
        )
        assertEquals(
            LovenseProtocol.MotorFeel.Wand,
            LovenseProtocol.MotorFeel.fromDeviceTypeId("W")
        )
        assertEquals(
            LovenseProtocol.MotorFeel.Compact,
            LovenseProtocol.MotorFeel.fromDeviceTypeId("S")
        )
    }

    @Test
    fun advertisedNameProvisionalFeel() {
        assertEquals(
            LovenseProtocol.MotorFeel.Wand,
            LovenseProtocol.MotorFeel.fromAdvertisedName("LVS-Domi38")
        )
        assertEquals(
            LovenseProtocol.MotorFeel.DualBody,
            LovenseProtocol.MotorFeel.fromAdvertisedName("LVS-Edge2")
        )
        assertEquals(
            LovenseProtocol.MotorFeel.Compact,
            LovenseProtocol.MotorFeel.fromAdvertisedName("LVS-Lush3")
        )
    }

    @Test
    fun stopCommandDetection() {
        assertTrue(LovenseProtocol.isStopCommand(LovenseProtocol.stop()))
        assertTrue(LovenseProtocol.isStopCommand(LovenseProtocol.vibrate(0)))
        assertTrue(LovenseProtocol.isStopCommand(LovenseProtocol.vibrate2(0, 0)))
        assertFalse(LovenseProtocol.isStopCommand(LovenseProtocol.vibrate(5)))
        assertFalse(LovenseProtocol.isStopCommand(LovenseProtocol.vibrate2(3, 0)))
    }

    @Test
    fun restFloorOrderingCompactAboveWand() {
        // Compact insertables need a slightly higher floor than wands.
        assertTrue(
            LovenseProtocol.MotorFeel.Compact.restFloor
                >= LovenseProtocol.MotorFeel.Wand.restFloor
        )
    }
}
