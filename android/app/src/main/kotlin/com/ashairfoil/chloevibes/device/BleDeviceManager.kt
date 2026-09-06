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
import android.os.SystemClock
import android.util.Log
import java.util.UUID
import kotlin.math.pow
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

    // Written on BLE binder callback threads, read from the audio processing
    // thread in sendCommand() — volatile for cross-thread visibility.
    @Volatile private var gatt: BluetoothGatt? = null
    @Volatile private var writeCharacteristic: BluetoothGattCharacteristic? = null
    @Volatile private var notifyCharacteristic: BluetoothGattCharacteristic? = null

    // State
    @Volatile var connectionState: ConnectionState = ConnectionState.Disconnected
        private set
    @Volatile var connectedDeviceName: String? = null
        private set
    @Volatile var batteryLevel: Int = -1
        private set

    // BLE write gating -- Lovense devices drop commands if sent too fast.
    // We wait for the onCharacteristicWrite callback before sending the next.
    // Stops/true-zeros bypass the interval so rest troughs are not delayed
    // behind a stale peak packet still in flight.
    @Volatile private var writeInFlight = false
    private val commandQueue = BleCommandQueue()
    private var inFlightCommand: String? = null
    private var writeEpoch = 0L
    private var writeTicket = 0L
    private var writeFailures = 0
    private val writeLock = Object()
    private var lastWriteMs: Long = 0
    private val minWriteIntervalMs = 28L  // ~36Hz steady-state; stops may go faster
    private val stopWriteIntervalMs = 12L // allow rest to win over peak backlog
    private val writeTimeoutMs = 500L
    private var pendingDrainScheduled = false
    private var pendingDrainAtMs = 0L
    private var pendingDrainTicket = 0L
    // Peak hold only preserves punch transients — long holds filled Domi
    // rest troughs. WAVE-002: ~20–25 ms; large down-steps bypass entirely.
    private val peakHoldMs = 22L
    private val peakHoldDropBypassSteps = 3

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
        synchronized(writeLock) {
            handler.removeCallbacks(reconnectRunnable)
            userRequestedDisconnect = false
            reconnectAttempts = 0
            lastConnectAddress = address
            return connectInternal(address)
        }
    }

    private fun connectInternal(address: String): Boolean {
        synchronized(writeLock) {
            // Scanning while a connection attempt is in flight starves the link
            // layer on many stacks and is a common cause of GATT error 133. This
            // lives here (not in connect()) so auto-reconnect attempts get the
            // same guard — the user may have started a scan mid-backoff.
            stopScan()
            resetWriteState()
            // Fresh session: dual + feel defaults until DeviceType confirms.
            // Stale rest/gamma from a prior toy must not leak across reconnect.
            isDualMotor = false
            applyMotorFeel(LovenseProtocol.MotorFeel.Generic)
            closeGatt()
            val device = try { bluetoothAdapter?.getRemoteDevice(address) } catch (_: Exception) { null }
                ?: return false
            Log.d("ChloeVibes", "BLE connecting to device: $address")
            connectionState = ConnectionState.Connecting
            onConnectionStateChanged?.invoke(connectionState)
            // Provisional curves from advertised name so the first hits after
            // Ready feel right before DeviceType reply lands.
            val provisionalName = device.name
                ?: discoveredDevices[address]?.name
            if (provisionalName != null) {
                applyMotorFeel(LovenseProtocol.MotorFeel.fromAdvertisedName(provisionalName))
                Log.d("ChloeVibes", "Provisional motor feel from name='$provisionalName' rest=$motorRestFloor gamma=$motorFeelGamma")
            }

            gatt = try {
                device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
            } catch (e: Exception) {
                Log.w("ChloeVibes", "BLE connection could not start", e)
                null
            }
            if (gatt == null) {
                connectionState = ConnectionState.Disconnected
                onConnectionStateChanged?.invoke(connectionState)
            }
            return gatt != null
        }
    }

    /** Release the current GATT client interface, if any. */
    private fun closeGatt() {
        val oldGatt = synchronized(writeLock) {
            val old = gatt
            gatt = null
            writeCharacteristic = null
            notifyCharacteristic = null
            old
        }
        oldGatt?.let {
            try { it.disconnect() } catch (_: Exception) { }
            try { it.close() } catch (_: Exception) { }
        }
    }

    /** Retire an unresponsive client even if Android never reports disconnect. */
    private fun recoverGatt(client: BluetoothGatt, reason: String) {
        synchronized(writeLock) {
            if (!isCurrentGatt(client)) return
            Log.w("ChloeVibes", reason)
            closeGatt()
            resetWriteState()
            connectedDeviceName = null
            batteryLevel = -1
            if (!userRequestedDisconnect && lastConnectAddress != null &&
                reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
                scheduleReconnect()
            } else {
                connectionState = ConnectionState.Disconnected
                handler.post { onConnectionStateChanged?.invoke(ConnectionState.Disconnected) }
            }
        }
    }

    /** Disconnect from the current device. */
    fun disconnect() {
        synchronized(writeLock) {
            Log.d("ChloeVibes", "BLE disconnecting from device: ${connectedDeviceName ?: "unknown"}")
            userRequestedDisconnect = true
            lastConnectAddress = null
            handler.removeCallbacks(reconnectRunnable)
            val client = gatt
            if (client != null && connectionState == ConnectionState.Ready) {
                // Keep the link long enough to submit a zero behind an in-flight
                // write. Positive commands are fenced once disconnect is asked.
                commandQueue.clear()
                stopMotors()
                handler.postDelayed({ finishDisconnect(client) }, writeTimeoutMs)
            } else {
                finishDisconnect(client)
            }
        }
    }

    private fun finishDisconnect(client: BluetoothGatt?) {
        synchronized(writeLock) {
            if (!userRequestedDisconnect || gatt !== client) return
            closeGatt()
            resetWriteState()
            connectionState = ConnectionState.Disconnected
            connectedDeviceName = null
            batteryLevel = -1
            isDualMotor = false
            applyMotorFeel(LovenseProtocol.MotorFeel.Generic)
            handler.post { onConnectionStateChanged?.invoke(ConnectionState.Disconnected) }
        }
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
        synchronized(writeLock) {
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
    }

    /**
     * Whether a callback's gatt is the one this manager currently owns. A
     * superseded client's late callbacks (especially STATE_DISCONNECTED after
     * a re-connect) must not clobber the live connection's state. The
     * A reconnect backoff has no current client; late callbacks must not adopt it.
     */
    private fun isCurrentGatt(g: BluetoothGatt): Boolean {
        val current = gatt
        return current === g
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            synchronized(writeLock) {
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
                        // Refresh provisional feel once the stack exposes the name
                        // (often null at connectInternal time).
                        gatt.device.name?.let { name ->
                            applyMotorFeel(LovenseProtocol.MotorFeel.fromAdvertisedName(name))
                        }
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
                        handler.postDelayed({
                            if (isCurrentGatt(gatt) && connectionState != ConnectionState.Ready) {
                                recoverGatt(gatt, "BLE setup timed out; reconnecting")
                            }
                        }, 5_000L)
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
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            synchronized(writeLock) {
                if (!isCurrentGatt(gatt)) return
                // MTU exchange finished (success or not) -- now it's safe to issue the
                // next GATT op. Discover services here so it isn't dropped.
                Log.d("ChloeVibes", "MTU changed: mtu=$mtu status=$status; discovering services")
                gatt.discoverServices()
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            synchronized(writeLock) {
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
        }

        @Suppress("DEPRECATION") // API 26-32 compatibility; submission is serialized.
        private fun enableNotificationsAndFinish(gatt: BluetoothGatt, rxChar: BluetoothGattCharacteristic) {
            if (!gatt.setCharacteristicNotification(rxChar, true)) {
                gatt.disconnect()
                return
            }
            val descriptor = rxChar.getDescriptor(CCCD_UUID)
            if (descriptor == null) {
                gatt.disconnect()
                return
            }
            descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            if (!gatt.writeDescriptor(descriptor)) gatt.disconnect()
        }

        override fun onDescriptorWrite(gatt: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            synchronized(writeLock) {
                if (!isCurrentGatt(gatt) || descriptor.uuid != CCCD_UUID) return
                if (status != BluetoothGatt.GATT_SUCCESS) {
                    gatt.disconnect()
                    return
                }
                connectionState = ConnectionState.Ready
                reconnectAttempts = 0
                Log.d("ChloeVibes", "Connection Ready; requesting DeviceType + battery")
                handler.post { onConnectionStateChanged?.invoke(connectionState) }
                // Ask the device what it is first -- the DeviceType reply drives
                // dual-motor detection (parseLovenseResponse) -- then poll battery.
                stopMotors()
                sendCommand(LovenseProtocol.deviceType())
                sendCommand(LovenseProtocol.battery())
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int
        ) {
            if (!isCurrentGatt(gatt) || characteristic !== writeCharacteristic) return
            synchronized(writeLock) {
                if (!isCurrentGatt(gatt) || characteristic !== writeCharacteristic) return
                val completed = inFlightCommand
                if (userRequestedDisconnect && status == BluetoothGatt.GATT_SUCCESS &&
                    completed != null && LovenseProtocol.isStopCommand(completed)) {
                    finishDisconnect(gatt)
                    return
                }
                if (status != BluetoothGatt.GATT_SUCCESS && completed != null) {
                    commandQueue.retry(completed)
                    writeFailures++
                } else if (status == BluetoothGatt.GATT_SUCCESS) {
                    writeFailures = 0
                }
                writeInFlight = false
                inFlightCommand = null
                if (writeFailures >= 3) {
                    connectionState = ConnectionState.Connected
                    handler.post { recoverGatt(gatt, "Repeated BLE write failures; reconnecting") }
                }
            }
            // Previous write completed — flush any queued command
            flushPendingWrite()
        }

        @Suppress("DEPRECATION") // Legacy callback required on API 26-32.
        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic
        ) {
            synchronized(writeLock) {
                if (isCurrentGatt(gatt) && characteristic.uuid == notifyCharacteristic?.uuid) {
                    val response = characteristic.getStringValue(0) ?: return
                    Log.d("ChloeVibes", "RX '$response'")
                    parseLovenseResponse(response)
                }
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
        synchronized(writeLock) {
            if (connectionState != ConnectionState.Ready || gatt == null || writeCharacteristic == null) return false
            if (userRequestedDisconnect && !LovenseProtocol.isStopCommand(command)) return false
            commandQueue.offer(command)
        }
        return flushPendingWrite()
    }

    /** One lock owns the pending queue, characteristic value and GATT submission. */
    @Suppress("DEPRECATION") // API 26-32 compatibility; characteristic mutation is locked.
    private fun flushPendingWrite(): Boolean = synchronized(writeLock) {
        if (writeInFlight || connectionState != ConnectionState.Ready) return@synchronized false
        val g = gatt ?: return@synchronized false
        val characteristic = writeCharacteristic ?: return@synchronized false
        val next = commandQueue.peek() ?: return@synchronized false
        val intervalMs = if (LovenseProtocol.isStopCommand(next)) stopWriteIntervalMs else minWriteIntervalMs
        val now = SystemClock.elapsedRealtime()
        val waitMs = intervalMs - (now - lastWriteMs)
        if (waitMs > 0) {
            schedulePendingDrain(waitMs)
            return@synchronized false
        }
        val command = commandQueue.take() ?: return@synchronized false
        writeInFlight = true
        inFlightCommand = command
        lastWriteMs = now
        writeTicket++
        val ticket = writeTicket
        val epoch = writeEpoch
        characteristic.value = command.toByteArray(Charsets.US_ASCII)
        characteristic.writeType =
            if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0)
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            else BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        val started = try {
            g.writeCharacteristic(characteristic)
        } catch (e: Exception) {
            Log.w("ChloeVibes", "BLE command submission failed", e)
            false
        }
        if (!started) {
            writeInFlight = false
            inFlightCommand = null
            commandQueue.retry(command)
            writeFailures++
            if (writeFailures >= 3) {
                connectionState = ConnectionState.Connected
                handler.post { recoverGatt(g, "Repeated BLE write submission failures; reconnecting") }
            } else {
                schedulePendingDrain(intervalMs)
            }
        } else {
            // A timed-out response makes subsequent callbacks ambiguous. Drop
            // this client instead of letting its late callback clear a new write.
            handler.postDelayed({
                val timedOut = synchronized(writeLock) {
                    if (epoch != writeEpoch || ticket != writeTicket || !writeInFlight || gatt !== g) {
                        false
                    } else {
                        connectionState = ConnectionState.Connected
                        true
                    }
                }
                if (timedOut) {
                    recoverGatt(g, "BLE write callback timed out; reconnecting")
                }
            }, writeTimeoutMs)
        }
        started
    }

    private fun schedulePendingDrain(delayMs: Long) {
        val delay = delayMs.coerceAtLeast(1L)
        val scheduled = synchronized(writeLock) {
            val deadline = SystemClock.elapsedRealtime() + delay
            if (pendingDrainScheduled && pendingDrainAtMs <= deadline) return
            pendingDrainScheduled = true
            pendingDrainAtMs = deadline
            pendingDrainTicket++
            Pair(writeEpoch, pendingDrainTicket)
        }
        handler.postDelayed({
            val current = synchronized(writeLock) {
                if (scheduled.first != writeEpoch || scheduled.second != pendingDrainTicket) false else {
                    pendingDrainScheduled = false
                    true
                }
            }
            if (current) flushPendingWrite()
        }, delay)
    }

    private fun resetWriteState() {
        synchronized(writeLock) {
            writeInFlight = false
            commandQueue.clear()
            inFlightCommand = null
            writeFailures = 0
            writeEpoch++
            lastWriteMs = 0L
            pendingDrainScheduled = false
            pendingDrainAtMs = 0L
            pendingDrainTicket++
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

    /** Whether connected device supports dual motors (Edge, etc). */
    @Volatile var isDualMotor: Boolean = false
        private set

    // Device-class feel: linear 0–1 → ERM is not equal body sensation.
    // Wands (Domi) crush softs and hang above rest; compact toys need a
    // higher rest floor. Goal: music on genitals — punch + true quiet.
    // Defaults match LovenseProtocol.MotorFeel.Generic (gui.rs lockstep).
    @Volatile private var motorRestFloor: Float = LovenseProtocol.MotorFeel.Generic.restFloor
    @Volatile private var motorFeelGamma: Float = LovenseProtocol.MotorFeel.Generic.feelGamma

    private fun applyMotorFeel(feel: LovenseProtocol.MotorFeel) {
        motorRestFloor = feel.restFloor
        motorFeelGamma = feel.feelGamma
    }

    /**
     * Map pipeline level through device rest-floor + power curve before
     * Domi-style 0–20 quantization. True-zero below rest; gamma keeps softs
     * quiet so residual energy cannot re-arm a Domi hum.
     */
    private fun shapeForMotor(level: Float): Float {
        val x = level.coerceIn(0f, 1f)
        val rest = motorRestFloor
        if (x <= rest) return 0f
        val n = ((x - rest) / (1f - rest).coerceAtLeast(1e-6f)).coerceIn(0f, 1f)
        return n.toDouble().pow(motorFeelGamma.toDouble()).toFloat()
    }

    /**
     * Set vibration intensity (0.0 - 1.0).
     * Maps to Lovense protocol: Vibrate:X; where X is 0-20.
     * Uses first-order noise-shaped dithering for sub-step resolution.
     */
    fun setIntensity(level: Float) {
        if (connectionState != ConnectionState.Ready) return
        val continuous = shapeForMotor(level) * 20f
        // Below half a Domi step: hard zero. Noise-shaped dither error from
        // prior frames can round a near-silent target up to level 1 and leave
        // the motor humming after the envelope has fully rested.
        val quantized = if (continuous < 0.5f) {
            ditherErrorMain = 0f
            0
        } else {
            val withError = continuous + ditherErrorMain
            val q = withError.roundToInt().coerceIn(0, 20)
            ditherErrorMain = withError - q.toFloat()
            q
        }
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
            val cont1 = shapeForMotor(motor1) * 20f
            val q1 = if (cont1 < 0.5f) {
                ditherErrorMotor1 = 0f
                0
            } else {
                val with1 = cont1 + ditherErrorMotor1
                val q = with1.roundToInt().coerceIn(0, 20)
                ditherErrorMotor1 = with1 - q.toFloat()
                q
            }

            val cont2 = shapeForMotor(motor2) * 20f
            val q2 = if (cont2 < 0.5f) {
                ditherErrorMotor2 = 0f
                0
            } else {
                val with2 = cont2 + ditherErrorMotor2
                val q = with2.roundToInt().coerceIn(0, 20)
                ditherErrorMotor2 = with2 - q.toFloat()
                q
            }

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

    /**
     * Stop all motors immediately, bypassing peak hold and duplicate suppression.
     * Stop commands replace any pending vibrate so a safety path cannot be
     * overwritten by a stale intensity still sitting in the write queue.
     */
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
        val stopCmd = LovenseProtocol.stop()
        // Coalesce the queue to stop before attempting the live write so an
        // in-flight vibrate completion cannot flush a later intensity first.
        synchronized(writeLock) {
            commandQueue.offer(stopCmd)
        }
        if (connectionState == ConnectionState.Ready) {
            sendCommand(stopCmd)
        }
    }

    /**
     * Brief peak hold for punch, with honesty rules:
     * - level 0 always passes (true-zero rest / silence-class)
     * - large downward steps (≥ peakHoldDropBypassSteps) bypass hold so
     *   boom troughs are not filled by a 55 ms residual peak
     */
    private fun holdPeak(level: Int, heldLevel: Int, heldUntilMs: Long): Triple<Int, Int, Long> {
        if (level <= 0) {
            return Triple(0, 0, 0L)
        }
        val now = SystemClock.elapsedRealtime()
        // Large drop toward rest: do not keep the old peak alive.
        if (heldLevel - level >= peakHoldDropBypassSteps) {
            return Triple(level, level, 0L)
        }
        var newHeld = heldLevel
        var newUntil = heldUntilMs
        if (level > heldLevel) {
            newHeld = level
            newUntil = now + peakHoldMs
        } else if (now > heldUntilMs) {
            newHeld = level
            newUntil = 0L
        }
        val out = if (now <= newUntil) newHeld.coerceAtLeast(level) else level
        return Triple(out, newHeld, newUntil)
    }

    private fun holdMainPeak(level: Int): Int {
        val (out, held, until) = holdPeak(level, heldMainLevel, heldMainUntilMs)
        heldMainLevel = held
        heldMainUntilMs = until
        return out
    }

    private fun holdMotor1Peak(level: Int): Int {
        val (out, held, until) = holdPeak(level, heldMotor1Level, heldMotor1UntilMs)
        heldMotor1Level = held
        heldMotor1UntilMs = until
        return out
    }

    private fun holdMotor2Peak(level: Int): Int {
        val (out, held, until) = holdPeak(level, heldMotor2Level, heldMotor2UntilMs)
        heldMotor2Level = held
        heldMotor2UntilMs = until
        return out
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
        // Feel profiles by Lovense DeviceType code — lockstep with desktop
        // MotorKind / LovenseProtocol.MotorFeel (Nora A/C = DualBody).
        val feel = LovenseProtocol.MotorFeel.fromDeviceTypeId(identifier)
        applyMotorFeel(feel)
        Log.d(
            "ChloeVibes",
            "Lovense DeviceType id=$identifier dualMotor=$dual feel=$feel rest=$motorRestFloor gamma=$motorFeelGamma"
        )
    }
}
