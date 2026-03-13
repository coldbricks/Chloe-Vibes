// ==========================================================================
// LovenseProtocol.kt -- Lovense BLE command protocol
//
// Lovense devices advertise as "LVS-XXXX". Communication is via the
// Nordic UART Service (NUS):
//   Service:  6e400001-b5a3-f393-e0a9-e50e24dcca9e
//   TX (write): 6e400002-b5a3-f393-e0a9-e50e24dcca9e
//   RX (notify): 6e400003-b5a3-f393-e0a9-e50e24dcca9e
//
// Commands are ASCII strings terminated with semicolons.
// ==========================================================================

package com.ashairfoil.chloevibes.device

/**
 * Lovense BLE command builder.
 * All commands are ASCII strings ending with ';'
 */
object LovenseProtocol {

    /**
     * Set vibration intensity.
     * @param level 0-20 (0 = off, 20 = maximum)
     */
    fun vibrate(level: Int): String {
        val clamped = level.coerceIn(0, 20)
        return "Vibrate:$clamped;"
    }

    /**
     * Set vibration for devices with two motors (e.g., Nora).
     * @param level1 motor 1 intensity (0-20)
     * @param level2 motor 2 intensity (0-20)
     */
    fun vibrate2(level1: Int, level2: Int): String {
        val c1 = level1.coerceIn(0, 20)
        val c2 = level2.coerceIn(0, 20)
        return "Vibrate1:$c1;Vibrate2:$c2;"
    }

    /**
     * Set rotation speed (for supported devices like Nora).
     * @param level 0-20
     */
    fun rotate(level: Int): String {
        val clamped = level.coerceIn(0, 20)
        return "Rotate:$clamped;"
    }

    /** Request battery level. Response is "X;" where X is 0-100. */
    fun battery(): String = "Battery;"

    /** Request device type/model. */
    fun deviceType(): String = "DeviceType;"

    /** Request device info (firmware version, etc.). */
    fun deviceInfo(): String = "DeviceInfo;"

    /** Start a preset pattern. @param pattern 1-10 */
    fun preset(pattern: Int): String {
        val clamped = pattern.coerceIn(1, 10)
        return "Preset:$clamped;"
    }

    /** Power off the device. */
    fun powerOff(): String = "PowerOff;"

    /** Stop all motors. */
    fun stop(): String = "Vibrate:0;"
}
