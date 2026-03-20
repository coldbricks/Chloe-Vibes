// ==========================================================================
// BleDeviceManager.kt -- Android BLE device management
//
// Handles BLE scanning, connection, and device control for Lovense and
// other BLE vibrators. Uses Android's BluetoothLeScanner for discovery
// and BluetoothGatt for communication.
// ==========================================================================

package com.ashairfoil.chloevibes.device

import android.annotation.SuppressLint
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import java.util.*
import kotlin.math.roundToInt

// ---------------------------------------------------------------------------
// Device info
// ---------------------------------------------------------------------------

data class BleDeviceInfo(
    val name: String,
    val address: String,
    val rssi: Int = 0,
    val isLovense: Boolean = false
)

enum class ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Ready
}

// ---------------------------------------------------------------------------
// BleDeviceManager
// ---------------------------------------------------------------------------

@SuppressLint("MissingPermission")
class BleDeviceManager(private val context: Context) {

    private val bluetoothManager: BluetoothManager? =
        context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
    private val bluetoothAdapter: BluetoothAdapter? = bluetoothManager?.adapter
    private val scanner: BluetoothLeScanner? = bluetoothAdapter?.bluetoothLeScanner

    private var gatt: BluetoothGatt? = null
    private var writeCharacteristic: BluetoothGattCharacteristic? = null
    private var notifyCharacteristic: BluetoothGattCharacteristic? = null

    // State
    @Volatile var connectionState: ConnectionState = ConnectionState.Disconnected
        private set
    @Volatile var connectedDeviceName: String? = null
        private set
    @Volatile var batteryLevel: Int = -1
        private set

    // BLE write gating -- Lovense devices drop commands if sent too fast.
    // We wait for the onCharacteristicWrite callback before sending the next.
    @Volatile private var writeInFlight = false
    private var pendingCommand: String? = null
    private val writeLock = Object()
    private var lastWriteMs: Long = 0
    private val minWriteIntervalMs = 38L  // ~26Hz command rate
    private val writeTimeoutMs = 160L
    private var pendingDrainScheduled = false
    private var writeWatchdogScheduled = false

    // Output dithering -- temporal sub-step resolution.
    // Lovense has 21 intensity levels (0-20). By dithering between adjacent
    // levels across frames, motor inertia integrates the rapid switching into
    // smooth intermediate intensities, giving effective sub-step resolution.
    // This makes micro-pulse, chaos, and sustain modulation physically
    // expressible instead of being crushed to 5% steps.
    @Volatile private var ditherErrorMain: Float = 0f
    @Volatile private var ditherErrorMotor1: Float = 0f
    @Volatile private var ditherErrorMotor2: Float = 0f

    // Scan results
    private val discoveredDevices = mutableMapOf<String, BleDeviceInfo>()
    private var isScanning = false
    private val handler = Handler(Looper.getMainLooper())

    // Callbacks
    var onDeviceDiscovered: ((BleDeviceInfo) -> Unit)? = null
    var onConnectionStateChanged: ((ConnectionState) -> Unit)? = null
    var onBatteryUpdate: ((Int) -> Unit)? = null

    // Known Lovense service/characteristic UUID sets.
    // Older devices use Nordic UART; newer firmware uses Lovense-specific UUIDs.
    data class ServiceUuids(val service: UUID, val tx: UUID, val rx: UUID)

    companion object {
        private val KNOWN_SERVICES = listOf(
            // Nordic UART Service (older Lovense firmware)
            ServiceUuids(
                UUID.fromString("6e400001-b5a3-f393-e0a9-e50e24dcca9e"),
                UUID.fromString("6e400002-b5a3-f393-e0a9-e50e24dcca9e"),
                UUID.fromString("6e400003-b5a3-f393-e0a9-e50e24dcca9e")
            ),
            // Lovense-specific service (newer firmware, Domi 2 / Mission etc.)
            ServiceUuids(
                UUID.fromString("50300001-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("50300002-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("50300003-0023-4bd4-bbd5-a6920e4c5653")
            ),
            // Alternate Lovense service (some newer models)
            ServiceUuids(
                UUID.fromString("53300001-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("53300002-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("53300003-0023-4bd4-bbd5-a6920e4c5653")
            ),
            // Another variant seen on some Lovense devices
            ServiceUuids(
                UUID.fromString("57300001-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("57300002-0023-4bd4-bbd5-a6920e4c5653"),
                UUID.fromString("57300003-0023-4bd4-bbd5-a6920e4c5653")
            )
        )

        /** CCCD descriptor UUID for enabling notifications. */
        val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        private const val SCAN_TIMEOUT_MS = 15_000L
    }

    // -----------------------------------------------------------------------
    // Scanning
    // -----------------------------------------------------------------------

    /** Start scanning for BLE devices. Auto-stops after timeout. */
    fun startScan(): Boolean {
        if (scanner == null || isScanning) return false
        discoveredDevices.clear()

        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .build()

        // Scan without UUID filter — Lovense devices may advertise under any of
        // several service UUIDs depending on firmware version
        try {
            scanner.startScan(emptyList(), settings, scanCallback)
            isScanning = true
            handler.postDelayed({ stopScan() }, SCAN_TIMEOUT_MS)
            return true
        } catch (e: Exception) {
            return false
        }
    }

    /** Stop scanning. */
    fun stopScan() {
        if (!isScanning) return
        try {
            scanner?.stopScan(scanCallback)
        } catch (_: Exception) { }
        isScanning = false
    }

    fun getDiscoveredDevices(): List<BleDeviceInfo> = discoveredDevices.values.toList()

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val device = result.device
            // Device name can be null in early advertisements — try scanRecord
            // first (more reliable), then fall back to device.name, then skip
            // only if both are null AND we haven't seen this address before.
            val name = result.scanRecord?.deviceName
                ?: device.name
                ?: discoveredDevices[device.address]?.name
                ?: return
            val isLovense = name.startsWith("LVS-") || name.contains("Lovense", ignoreCase = true)

            val info = BleDeviceInfo(
                name = name,
                address = device.address,
                rssi = result.rssi,
                isLovense = isLovense
            )
            discoveredDevices[device.address] = info
            onDeviceDiscovered?.invoke(info)
        }

        override fun onScanFailed(errorCode: Int) {
            isScanning = false
        }
    }

    // -----------------------------------------------------------------------
    // Connection
    // -----------------------------------------------------------------------

    /** Connect to a device by address. */
    fun connect(address: String): Boolean {
        resetWriteState()
        val device = bluetoothAdapter?.getRemoteDevice(address) ?: return false
        connectionState = ConnectionState.Connecting
        onConnectionStateChanged?.invoke(connectionState)

        gatt = device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
        return gatt != null
    }

    /** Disconnect from the current device. */
    fun disconnect() {
        gatt?.disconnect()
        gatt?.close()
        gatt = null
        writeCharacteristic = null
        notifyCharacteristic = null
        resetWriteState()
        connectionState = ConnectionState.Disconnected
        connectedDeviceName = null
        batteryLevel = -1
        isDualMotor = false
        onConnectionStateChanged?.invoke(connectionState)
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    connectionState = ConnectionState.Connected
                    connectedDeviceName = gatt.device.name
                    // Detect dual-motor devices from name at connection time
                    gatt.device.name?.let { name ->
                        if (name.contains("Domi", ignoreCase = true) ||
                            name.contains("Edge", ignoreCase = true) ||
                            name.contains("Nora", ignoreCase = true)
                        ) {
                            isDualMotor = true
                        }
                    }
                    handler.post { onConnectionStateChanged?.invoke(connectionState) }
                    try { gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH) } catch (_: Exception) { }
                    gatt.discoverServices()
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    connectionState = ConnectionState.Disconnected
                    connectedDeviceName = null
                    writeCharacteristic = null
                    notifyCharacteristic = null
                    resetWriteState()
                    handler.post { onConnectionStateChanged?.invoke(connectionState) }
                }
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            if (status != BluetoothGatt.GATT_SUCCESS) return

            // Try each known Lovense UUID set
            for (uuids in KNOWN_SERVICES) {
                val service = gatt.getService(uuids.service) ?: continue
                val tx = service.getCharacteristic(uuids.tx) ?: continue
                val rx = service.getCharacteristic(uuids.rx) ?: continue
                writeCharacteristic = tx
                notifyCharacteristic = rx
                enableNotificationsAndFinish(gatt, rx)
                return
            }

            // Fallback: scan ALL services for a writable + notifiable pair
            // (covers unknown firmware revisions)
            for (service in gatt.services) {
                var txCandidate: BluetoothGattCharacteristic? = null
                var rxCandidate: BluetoothGattCharacteristic? = null
                for (c in service.characteristics) {
                    val props = c.properties
                    if (props and BluetoothGattCharacteristic.PROPERTY_WRITE != 0 ||
                        props and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0
                    ) {
                        txCandidate = c
                    }
                    if (props and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0) {
                        rxCandidate = c
                    }
                }
                if (txCandidate != null && rxCandidate != null) {
                    writeCharacteristic = txCandidate
                    notifyCharacteristic = rxCandidate
                    enableNotificationsAndFinish(gatt, rxCandidate)
                    return
                }
            }
        }

        private fun enableNotificationsAndFinish(gatt: BluetoothGatt, rxChar: BluetoothGattCharacteristic) {
            gatt.setCharacteristicNotification(rxChar, true)
            val descriptor = rxChar.getDescriptor(CCCD_UUID)
            descriptor?.let {
                it.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                gatt.writeDescriptor(it)
            }
            connectionState = ConnectionState.Ready
            handler.post { onConnectionStateChanged?.invoke(connectionState) }
            handler.postDelayed({ sendCommand("Battery;") }, 500)
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int
        ) {
            synchronized(writeLock) {
                writeWatchdogScheduled = false
            }
            // Previous write completed — flush any queued command
            flushPendingWrite()
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic
        ) {
            if (characteristic.uuid == notifyCharacteristic?.uuid) {
                val response = characteristic.getStringValue(0) ?: return
                parseLovenseResponse(response)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------

    /**
     * Send a raw string command to the connected device.
     * Respects BLE write gating — only one write in flight at a time.
     */
    fun sendCommand(command: String): Boolean {
        val characteristic = writeCharacteristic ?: return false
        val g = gatt ?: return false

        var shouldWrite = false
        var queueBecauseInFlight = false
        var pendingDelayMs = 0L

        synchronized(writeLock) {
            val now = System.currentTimeMillis()
            // Auto-clear writeInFlight if the BLE callback hasn't arrived
            // within timeout.  When the app is backgrounded, Android may delay
            // onCharacteristicWrite callbacks, leaving writeInFlight stuck and
            // blocking all subsequent vibration commands.
            if (writeInFlight && (now - lastWriteMs) > writeTimeoutMs) {
                writeInFlight = false
            }

            if (writeInFlight) {
                pendingCommand = command
                queueBecauseInFlight = true
            } else {
                val sinceLast = now - lastWriteMs
                if (sinceLast < minWriteIntervalMs) {
                    pendingCommand = command
                    pendingDelayMs = minWriteIntervalMs - sinceLast
                } else {
                    writeInFlight = true
                    lastWriteMs = now
                    shouldWrite = true
                }
            }
        }

        if (queueBecauseInFlight) {
            scheduleWriteWatchdog()
            return false
        }
        if (!shouldWrite) {
            schedulePendingDrain(pendingDelayMs)
            return false
        }

        characteristic.value = command.toByteArray(Charsets.US_ASCII)
        characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        val started = g.writeCharacteristic(characteristic)
        if (!started) {
            synchronized(writeLock) {
                writeInFlight = false
                pendingCommand = command
            }
            schedulePendingDrain(minWriteIntervalMs)
        } else {
            scheduleWriteWatchdog()
        }
        return started
    }

    /** Flush pending command after a write completes. */
    private fun flushPendingWrite() {
        val cmd: String?
        synchronized(writeLock) {
            writeInFlight = false
            cmd = pendingCommand
            pendingCommand = null
        }
        cmd?.let { sendCommand(it) }
    }

    /** Schedules pending command flush once min write interval has elapsed. */
    private fun schedulePendingDrain(delayMs: Long) {
        val shouldSchedule = synchronized(writeLock) {
            if (pendingDrainScheduled) {
                false
            } else {
                pendingDrainScheduled = true
                true
            }
        }
        if (!shouldSchedule) return

        handler.postDelayed({
            synchronized(writeLock) {
                pendingDrainScheduled = false
            }
            flushPendingWrite()
        }, delayMs.coerceAtLeast(1L))
    }

    /** Watchdog for missing onCharacteristicWrite callbacks. */
    private fun scheduleWriteWatchdog() {
        val shouldSchedule = synchronized(writeLock) {
            if (writeWatchdogScheduled) {
                false
            } else {
                writeWatchdogScheduled = true
                true
            }
        }
        if (!shouldSchedule) return

        handler.postDelayed({
            var shouldFlush = false
            synchronized(writeLock) {
                writeWatchdogScheduled = false
                val now = System.currentTimeMillis()
                if (writeInFlight && (now - lastWriteMs) > writeTimeoutMs) {
                    writeInFlight = false
                    shouldFlush = pendingCommand != null
                }
            }
            if (shouldFlush) {
                flushPendingWrite()
            }
        }, writeTimeoutMs)
    }

    private fun resetWriteState() {
        synchronized(writeLock) {
            writeInFlight = false
            pendingCommand = null
            lastWriteMs = 0L
            pendingDrainScheduled = false
            writeWatchdogScheduled = false
        }
        ditherErrorMain = 0f
        ditherErrorMotor1 = 0f
        ditherErrorMotor2 = 0f
    }

    /** Whether connected device supports dual motors (Domi 2, Nora, etc). */
    @Volatile var isDualMotor: Boolean = false
        private set

    /**
     * Set vibration intensity (0.0 - 1.0).
     * Maps to Lovense protocol: Vibrate:X; where X is 0-20.
     * Uses first-order noise-shaped dithering for sub-step resolution.
     */
    fun setIntensity(level: Float) {
        if (connectionState != ConnectionState.Ready) return
        val continuous = level * 20f
        val withError = continuous + ditherErrorMain
        val quantized = withError.roundToInt().coerceIn(0, 20)
        ditherErrorMain = withError - quantized.toFloat()
        sendCommand(LovenseProtocol.vibrate(quantized))
    }

    /**
     * Set dual-motor intensity with independent motor levels.
     * Motor 1 and motor 2 receive separate intensity values,
     * creating spatial movement when they differ.
     * Falls back to single-motor command for non-dual devices.
     *
     * @param motor1 primary motor intensity (0.0 - 1.0)
     * @param motor2 secondary motor intensity (0.0 - 1.0)
     */
    fun setDualIntensity(motor1: Float, motor2: Float) {
        if (connectionState != ConnectionState.Ready) return
        if (isDualMotor) {
            val cont1 = motor1 * 20f
            val with1 = cont1 + ditherErrorMotor1
            val q1 = with1.roundToInt().coerceIn(0, 20)
            ditherErrorMotor1 = with1 - q1.toFloat()

            val cont2 = motor2 * 20f
            val with2 = cont2 + ditherErrorMotor2
            val q2 = with2.roundToInt().coerceIn(0, 20)
            ditherErrorMotor2 = with2 - q2.toFloat()

            sendCommand(LovenseProtocol.vibrate2(q1, q2))
        } else {
            setIntensity(motor1)
        }
    }

    /** Request battery level update. */
    fun requestBattery() {
        if (connectionState == ConnectionState.Ready) {
            sendCommand(LovenseProtocol.battery())
        }
    }

    private fun parseLovenseResponse(response: String) {
        val trimmed = response.trim().removeSuffix(";")

        // Simple numeric response: "85;" → battery 85%
        trimmed.toIntOrNull()?.let { level ->
            if (level in 0..100) {
                batteryLevel = level
                handler.post { onBatteryUpdate?.invoke(level) }
                return
            }
        }

        // Newer Lovense firmware: battery response as "Bxx;" where xx is 1-100,
        // or device type "Axx:yy:zzzzzz;" etc.  Also "OK;" is just an ACK.
        if (trimmed.length >= 2 && trimmed[0].uppercaseChar() == 'B') {
            trimmed.substring(1).toIntOrNull()?.let { level ->
                if (level in 0..100) {
                    batteryLevel = level
                    handler.post { onBatteryUpdate?.invoke(level) }
                }
            }
        }

        // Device type response -- detect dual-motor devices.
        // Domi 2 responds with device type starting with "W" (wand)
        // Nora responds with "A", Edge with "P" -- all dual motor.
        val upperTrimmed = trimmed.uppercase()
        if (upperTrimmed.startsWith("W") || upperTrimmed.startsWith("P")) {
            isDualMotor = true
        }
        // Also detect from device name at connection time
        connectedDeviceName?.let { name ->
            if (name.contains("Domi", ignoreCase = true) ||
                name.contains("Edge", ignoreCase = true) ||
                name.contains("Nora", ignoreCase = true)
            ) {
                isDualMotor = true
            }
        }
    }
}
