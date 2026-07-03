// ==========================================================================
// BleDeviceManager.kt -- Android BLE device management
//
// Handles BLE scanning, connection, and device control for Lovense and
// other BLE vibrators. Uses Android's BluetoothLeScanner for discovery
// and BluetoothGatt for communication.
// ==========================================================================

package com.ashairfoil.chloevibes.device

import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.UUID
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
    private val minWriteIntervalMs = 30L  // ~33Hz command rate
    private val writeTimeoutMs = 120L
    private var pendingDrainScheduled = false
    private var writeWatchdogScheduled = false
    private val peakHoldMs = 55L

    // Output dithering -- temporal sub-step resolution.
    // Lovense has 21 intensity levels (0-20). By dithering between adjacent
    // levels across frames, motor inertia integrates the rapid switching into
    // smooth intermediate intensities, giving effective sub-step resolution.
    // This makes micro-pulse, chaos, and sustain modulation physically
    // expressible instead of being crushed to 5% steps.
    @Volatile private var ditherErrorMain: Float = 0f
    @Volatile private var ditherErrorMotor1: Float = 0f
    @Volatile private var ditherErrorMotor2: Float = 0f
    @Volatile private var heldMainLevel: Int = 0
    @Volatile private var heldMotor1Level: Int = 0
    @Volatile private var heldMotor2Level: Int = 0
    @Volatile private var heldMainUntilMs: Long = 0L
    @Volatile private var heldMotor1UntilMs: Long = 0L
    @Volatile private var heldMotor2UntilMs: Long = 0L
    @Volatile private var lastRequestedMainLevel: Int = -1
    @Volatile private var lastRequestedMotor1Level: Int = -1
    @Volatile private var lastRequestedMotor2Level: Int = -1

    // Scan results
    private val discoveredDevices = mutableMapOf<String, BleDeviceInfo>()
    private var isScanning = false
    private val handler = Handler(Looper.getMainLooper())

    // Auto-reconnect -- re-establish the link after an unexpected drop
    // (RF dropout, peer supervision timeout). Cleared by an explicit
    // disconnect() so a user-requested disconnect stays disconnected.
    @Volatile private var lastConnectAddress: String? = null
    @Volatile private var userRequestedDisconnect = false
    @Volatile private var reconnectAttempts = 0
    private val reconnectRunnable = Runnable { attemptReconnect() }

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
        private const val MAX_RECONNECT_ATTEMPTS = 6
        private const val RECONNECT_BASE_DELAY_MS = 600L
        private const val RECONNECT_MAX_DELAY_MS = 8_000L

        // Lovense DeviceType identifiers (the code before the first ':' in the
        // "<code>:<fw>:<mac>" reply) with two INDEPENDENT vibration motors that
        // accept Vibrate1:/Vibrate2:. Codes can be multi-letter, so we match the
        // FULL code exactly -- e.g. "OC" (Osci 3) must not collide with "O" (Osci).
        //
        // Only "P" (Edge / Edge 2) is fixture-verified high-confidence, so it is
        // the only one enabled. The design is deliberately ASYMMETRIC: a false
        // "dual" makes a single-motor toy drop Vibrate2 and possibly go silent,
        // whereas leaving a real dual device on plain "Vibrate:" still drives all
        // its motors uniformly -- a safe, working fallback. So when unsure, stay
        // single. Single-motor models (Domi "W", Lush "S", Gush "ED", Nora, Max,
        // Hush "Z", ...) are intentionally NOT here.
        //
        // Medium-confidence dual candidates from protocol research, to enable only
        // after testing on real hardware: "J" (Dolce), "N" (Gemini), "OC" (Osci 3).
        // Flexer "EI" is dual-actuator but uses Mply:, not Vibrate1/2 -- do NOT add.
        private val DUAL_VIBRATE_IDENTIFIERS = setOf("P")
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
            // first (more reliable), then fall back to device.name, then to a
            // previously-seen name. Unlike before, we no longer drop unnamed
            // devices: a Lovense wand often advertises with a null name for the
            // first few packets, so skipping them hid the device entirely.
            val advertisedName = result.scanRecord?.deviceName
                ?: device.name
                ?: discoveredDevices[device.address]?.name
            val name = advertisedName ?: "Unknown BLE ${device.address.takeLast(5)}"
            val isLovense = name.startsWith("LVS-") || name.contains("Lovense", ignoreCase = true)

            Log.d(
                "ChloeVibes",
                "BLE seen: name=$advertisedName, address=${device.address}, rssi=${result.rssi}, uuids=${result.scanRecord?.serviceUuids}"
            )

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
            Log.w("ChloeVibes", "BLE scan failed with error code: $errorCode")
            isScanning = false
        }
    }

    // -----------------------------------------------------------------------
    // Connection
    // -----------------------------------------------------------------------

    /** Connect to a device by address. */
    fun connect(address: String): Boolean {
        handler.removeCallbacks(reconnectRunnable)
        userRequestedDisconnect = false
        reconnectAttempts = 0
        lastConnectAddress = address
        return connectInternal(address)
    }

    private fun connectInternal(address: String): Boolean {
        // Scanning while a connection attempt is in flight starves the link
        // layer on many stacks and is a common cause of GATT error 133. This
        // lives here (not in connect()) so auto-reconnect attempts get the
        // same guard — the user may have started a scan mid-backoff.
        stopScan()
        resetWriteState()
        closeGatt()
        val device = bluetoothAdapter?.getRemoteDevice(address) ?: return false
        Log.d("ChloeVibes", "BLE connecting to device: $address")
        connectionState = ConnectionState.Connecting
        onConnectionStateChanged?.invoke(connectionState)

        gatt = device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
        return gatt != null
    }

    /** Release the current GATT client interface, if any. */
    private fun closeGatt() {
        gatt?.let {
            try { it.disconnect() } catch (_: Exception) { }
            try { it.close() } catch (_: Exception) { }
        }
        gatt = null
        writeCharacteristic = null
        notifyCharacteristic = null
    }

    /** Disconnect from the current device. */
    fun disconnect() {
        Log.d("ChloeVibes", "BLE disconnecting from device: ${connectedDeviceName ?: "unknown"}")
        userRequestedDisconnect = true
        lastConnectAddress = null
        handler.removeCallbacks(reconnectRunnable)
        closeGatt()
        resetWriteState()
        connectionState = ConnectionState.Disconnected
        connectedDeviceName = null
        batteryLevel = -1
        isDualMotor = false
        onConnectionStateChanged?.invoke(connectionState)
    }

    /**
     * Reconnect after an unexpected link loss. Exponential backoff, capped;
     * gives up after MAX_RECONNECT_ATTEMPTS and reports Disconnected.
     */
    private fun scheduleReconnect() {
        reconnectAttempts += 1
        val shift = (reconnectAttempts - 1).coerceAtMost(4)
        val delay = (RECONNECT_BASE_DELAY_MS shl shift).coerceAtMost(RECONNECT_MAX_DELAY_MS)
        Log.d(
            "ChloeVibes",
            "BLE link lost; reconnect attempt $reconnectAttempts/$MAX_RECONNECT_ATTEMPTS in ${delay}ms"
        )
        connectionState = ConnectionState.Connecting
        handler.post { onConnectionStateChanged?.invoke(connectionState) }
        handler.removeCallbacks(reconnectRunnable)
        handler.postDelayed(reconnectRunnable, delay)
    }

    private fun attemptReconnect() {
        if (userRequestedDisconnect) return
        if (connectionState == ConnectionState.Connected || connectionState == ConnectionState.Ready) return
        val address = lastConnectAddress ?: return
        if (!connectInternal(address)) {
            // Synchronous failure (adapter off/null): no GATT callback will
            // ever fire, so drive the retry/give-up path from here or the
            // state machine is stuck showing Connecting forever.
            if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
                scheduleReconnect()
            } else {
                connectionState = ConnectionState.Disconnected
                handler.post { onConnectionStateChanged?.invoke(connectionState) }
            }
        }
    }

    /**
     * Whether a callback's gatt is the one this manager currently owns. A
     * superseded client's late callbacks (especially STATE_DISCONNECTED after
     * a re-connect) must not clobber the live connection's state. The
     * null-while-Connecting case covers the tiny window between
     * connectGatt() registering the callback and the field assignment.
     */
    private fun isCurrentGatt(g: BluetoothGatt): Boolean {
        val current = gatt
        return current === g ||
            (current == null && connectionState == ConnectionState.Connecting)
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            if (!isCurrentGatt(gatt)) {
                Log.d("ChloeVibes", "Ignoring callback from superseded GATT client (newState=$newState)")
                try { gatt.close() } catch (_: Exception) { }
                return
            }
            if (status != BluetoothGatt.GATT_SUCCESS) {
                Log.e("ChloeVibes", "GATT error: status=$status, newState=$newState")
            }
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    Log.d("ChloeVibes", "BLE connected to: ${gatt.device.name ?: gatt.device.address}")
                    connectionState = ConnectionState.Connected
                    connectedDeviceName = gatt.device.name
                    // Dual-motor capability is detected from the DeviceType
                    // response once services are ready (see parseLovenseResponse).
                    // Lovense advertises as "LVS-XXXX", so the model name is never
                    // in the BLE name -- the old name-substring check never fired.
                    handler.post { onConnectionStateChanged?.invoke(connectionState) }
                    try { gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH) } catch (_: Exception) { }
                    // Request a larger MTU first (the dual-motor
                    // "Vibrate1:..;Vibrate2:..;" command is 24 bytes, over the
                    // 20-byte default payload), THEN discover services from
                    // onMtuChanged. Issuing requestMtu() and discoverServices()
                    // back-to-back makes the second GATT op get dropped on many
                    // stacks, so onServicesDiscovered never fires and the peer
                    // drops the link after ~10s (GATT status 19). Fall back to
                    // discovering immediately only if the MTU request won't start.
                    val mtuRequested = try { gatt.requestMtu(185) } catch (_: Exception) { false }
                    if (!mtuRequested) {
                        gatt.discoverServices()
                    }
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    Log.d("ChloeVibes", "BLE disconnected (status=$status)")
                    // ALWAYS close the client interface here. Android caps the
                    // process at ~32 GATT clients; leaking one per dropped or
                    // failed connection eventually makes every connectGatt()
                    // fail until the app is killed ("works after a restart").
                    try { gatt.close() } catch (_: Exception) { }
                    if (this@BleDeviceManager.gatt === gatt) {
                        this@BleDeviceManager.gatt = null
                    }
                    connectedDeviceName = null
                    writeCharacteristic = null
                    notifyCharacteristic = null
                    resetWriteState()
                    if (!userRequestedDisconnect && lastConnectAddress != null &&
                        reconnectAttempts < MAX_RECONNECT_ATTEMPTS
                    ) {
                        scheduleReconnect()
                    } else {
                        connectionState = ConnectionState.Disconnected
                        handler.post { onConnectionStateChanged?.invoke(connectionState) }
                    }
                }
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            if (!isCurrentGatt(gatt)) return
            // MTU exchange finished (success or not) -- now it's safe to issue the
            // next GATT op. Discover services here so it isn't dropped.
            Log.d("ChloeVibes", "MTU changed: mtu=$mtu status=$status; discovering services")
            gatt.discoverServices()
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            if (!isCurrentGatt(gatt)) return
            if (status != BluetoothGatt.GATT_SUCCESS) {
                Log.w("ChloeVibes", "onServicesDiscovered failed: status=$status")
                // Drop the link instead of sitting half-connected; the
                // disconnect callback closes the client and retries.
                gatt.disconnect()
                return
            }
            Log.d("ChloeVibes", "Services discovered: ${gatt.services.size}")

            // Try each known Lovense UUID set
            for (uuids in KNOWN_SERVICES) {
                val service = gatt.getService(uuids.service) ?: continue
                val tx = service.getCharacteristic(uuids.tx) ?: continue
                val rx = service.getCharacteristic(uuids.rx) ?: continue
                writeCharacteristic = tx
                notifyCharacteristic = rx
                Log.d("ChloeVibes", "Matched Lovense service ${uuids.service} tx=${uuids.tx} props=${tx.properties}")
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
                    Log.d("ChloeVibes", "Fallback match: service ${service.uuid} tx=${txCandidate.uuid} props=${txCandidate.properties}")
                    enableNotificationsAndFinish(gatt, rxCandidate)
                    return
                }
            }

            // No compatible service/characteristic found -- incompatible
            // device, so do not auto-reconnect to it.
            Log.w("ChloeVibes", "No compatible GATT characteristics found for device")
            lastConnectAddress = null
            connectionState = ConnectionState.Disconnected
            handler.post { onConnectionStateChanged?.invoke(ConnectionState.Disconnected) }
            gatt.disconnect()
            gatt.close()
        }

        private fun enableNotificationsAndFinish(gatt: BluetoothGatt, rxChar: BluetoothGattCharacteristic) {
            gatt.setCharacteristicNotification(rxChar, true)
            val descriptor = rxChar.getDescriptor(CCCD_UUID)
            descriptor?.let {
                it.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                gatt.writeDescriptor(it)
            }
            connectionState = ConnectionState.Ready
            reconnectAttempts = 0
            Log.d("ChloeVibes", "Connection Ready; requesting DeviceType + battery")
            handler.post { onConnectionStateChanged?.invoke(connectionState) }
            // Ask the device what it is first -- the DeviceType reply drives
            // dual-motor detection (parseLovenseResponse) -- then poll battery.
            handler.postDelayed({ sendCommand(LovenseProtocol.deviceType()) }, 300)
            handler.postDelayed({ sendCommand(LovenseProtocol.battery()) }, 700)
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
                Log.d("ChloeVibes", "RX '$response'")
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
        val characteristic = writeCharacteristic
        if (characteristic == null) {
            if (!command.startsWith("Vibrate")) {
                Log.w("ChloeVibes", "sendCommand('$command') dropped: not ready (no characteristic)")
            }
            return false
        }
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
        // Use write-with-response only if the characteristic advertises it; many
        // Lovense TX characteristics are WRITE_NO_RESPONSE only, and writing with
        // the wrong type silently never reaches the device.
        characteristic.writeType =
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0) {
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            } else {
                BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
            }
        val started = g.writeCharacteristic(characteristic)
        if (!command.startsWith("Vibrate")) {
            Log.d("ChloeVibes", "TX '$command' started=$started wt=${characteristic.writeType}")
        }
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
        heldMainLevel = 0
        heldMotor1Level = 0
        heldMotor2Level = 0
        heldMainUntilMs = 0L
        heldMotor1UntilMs = 0L
        heldMotor2UntilMs = 0L
        lastRequestedMainLevel = -1
        lastRequestedMotor1Level = -1
        lastRequestedMotor2Level = -1
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
        val continuous = level.coerceIn(0f, 1f) * 20f
        val withError = continuous + ditherErrorMain
        val quantized = withError.roundToInt().coerceIn(0, 20)
        ditherErrorMain = withError - quantized.toFloat()
        val held = holdMainPeak(quantized)
        if (held == lastRequestedMainLevel) return
        lastRequestedMainLevel = held
        sendCommand(LovenseProtocol.vibrate(held))
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
            val cont1 = motor1.coerceIn(0f, 1f) * 20f
            val with1 = cont1 + ditherErrorMotor1
            val q1 = with1.roundToInt().coerceIn(0, 20)
            ditherErrorMotor1 = with1 - q1.toFloat()

            val cont2 = motor2.coerceIn(0f, 1f) * 20f
            val with2 = cont2 + ditherErrorMotor2
            val q2 = with2.roundToInt().coerceIn(0, 20)
            ditherErrorMotor2 = with2 - q2.toFloat()

            val held1 = holdMotor1Peak(q1)
            val held2 = holdMotor2Peak(q2)
            if (held1 == lastRequestedMotor1Level && held2 == lastRequestedMotor2Level) return
            lastRequestedMotor1Level = held1
            lastRequestedMotor2Level = held2
            sendCommand(LovenseProtocol.vibrate2(held1, held2))
        } else {
            setIntensity(motor1)
        }
    }

    /** Stop all motors immediately, bypassing peak hold and duplicate suppression. */
    fun stopMotors() {
        ditherErrorMain = 0f
        ditherErrorMotor1 = 0f
        ditherErrorMotor2 = 0f
        heldMainLevel = 0
        heldMotor1Level = 0
        heldMotor2Level = 0
        heldMainUntilMs = 0L
        heldMotor1UntilMs = 0L
        heldMotor2UntilMs = 0L
        lastRequestedMainLevel = -1
        lastRequestedMotor1Level = -1
        lastRequestedMotor2Level = -1
        if (connectionState == ConnectionState.Ready) {
            sendCommand(LovenseProtocol.stop())
        }
    }

    private fun holdMainPeak(level: Int): Int {
        val now = System.currentTimeMillis()
        if (level > heldMainLevel) {
            heldMainLevel = level
            heldMainUntilMs = now + peakHoldMs
        } else if (now > heldMainUntilMs) {
            heldMainLevel = level
        }
        return if (now <= heldMainUntilMs) heldMainLevel.coerceAtLeast(level) else level
    }

    private fun holdMotor1Peak(level: Int): Int {
        val now = System.currentTimeMillis()
        if (level > heldMotor1Level) {
            heldMotor1Level = level
            heldMotor1UntilMs = now + peakHoldMs
        } else if (now > heldMotor1UntilMs) {
            heldMotor1Level = level
        }
        return if (now <= heldMotor1UntilMs) heldMotor1Level.coerceAtLeast(level) else level
    }

    private fun holdMotor2Peak(level: Int): Int {
        val now = System.currentTimeMillis()
        if (level > heldMotor2Level) {
            heldMotor2Level = level
            heldMotor2UntilMs = now + peakHoldMs
        } else if (now > heldMotor2UntilMs) {
            heldMotor2Level = level
        }
        return if (now <= heldMotor2UntilMs) heldMotor2Level.coerceAtLeast(level) else level
    }

    /** Request battery level update. */
    fun requestBattery() {
        if (connectionState == ConnectionState.Ready) {
            sendCommand(LovenseProtocol.battery())
        }
    }

    private fun parseLovenseResponse(response: String) {
        val trimmed = response.trim().removeSuffix(";")

        // DeviceType reply: "<identifier>:<version>:<serial>", e.g. "P:02:0082..".
        // The leading identifier is the Lovense model code -- map it to motor
        // capability. Checked BEFORE battery parsing since it contains ':'.
        if (trimmed.contains(':')) {
            // The type code is 1-4 letters ("P", "OC", "ED", ...); reject other
            // colon-bearing replies so they can't be misread as a device type.
            val identifier = trimmed.substringBefore(':').trim().uppercase()
            if (identifier.matches(Regex("^[A-Z]{1,4}$"))) {
                applyDeviceType(identifier)
            }
            return
        }

        // Simple numeric battery response: "85" → 85%
        val numeric = trimmed.toIntOrNull()
        if (numeric != null) {
            if (numeric in 0..100) {
                batteryLevel = numeric
                handler.post { onBatteryUpdate?.invoke(numeric) }
            }
            return
        }

        // Some firmware reports battery as "Bxx" (no colon).
        if (trimmed.length >= 2 && trimmed[0].uppercaseChar() == 'B' && trimmed[1].isDigit()) {
            trimmed.substring(1).toIntOrNull()?.let { level ->
                if (level in 0..100) {
                    batteryLevel = level
                    handler.post { onBatteryUpdate?.invoke(level) }
                }
            }
        }
    }

    /**
     * Map a Lovense DeviceType identifier (the leading code in the
     * "<id>:<version>:<serial>" reply) to dual-motor capability. Only identifiers
     * known to accept independent Vibrate1:/Vibrate2: commands are treated as
     * dual; everything else stays single-motor. Safe by default -- a single-motor
     * device silently ignores Vibrate2, so we never guess "dual" for an unknown
     * model and end up sending commands it drops.
     */
    private fun applyDeviceType(identifier: String) {
        val dual = identifier in DUAL_VIBRATE_IDENTIFIERS
        isDualMotor = dual
        Log.d("ChloeVibes", "Lovense DeviceType id=$identifier -> dualMotor=$dual")
    }
}
