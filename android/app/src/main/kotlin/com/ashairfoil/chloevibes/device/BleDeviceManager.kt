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
    private val minWriteIntervalMs = 50L  // 20Hz max command rate

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
            scanner.startScan(scanCallback)
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
        connectionState = ConnectionState.Disconnected
        connectedDeviceName = null
        batteryLevel = -1
        onConnectionStateChanged?.invoke(connectionState)
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    connectionState = ConnectionState.Connected
                    connectedDeviceName = gatt.device.name
                    handler.post { onConnectionStateChanged?.invoke(connectionState) }
                    gatt.discoverServices()
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    connectionState = ConnectionState.Disconnected
                    connectedDeviceName = null
                    writeCharacteristic = null
                    notifyCharacteristic = null
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

        synchronized(writeLock) {
            val now = System.currentTimeMillis()
            // Auto-clear writeInFlight if the BLE callback hasn't arrived
            // within 200ms.  When the app is backgrounded, Android may delay
            // onCharacteristicWrite callbacks, leaving writeInFlight stuck and
            // blocking all subsequent vibration commands.
            if (writeInFlight && (now - lastWriteMs) > 200) {
                writeInFlight = false
            }
            if (writeInFlight || (now - lastWriteMs) < minWriteIntervalMs) {
                // Queue this as the next command (overwrites any stale pending)
                pendingCommand = command
                return false
            }
            writeInFlight = true
            lastWriteMs = now
        }

        characteristic.value = command.toByteArray(Charsets.US_ASCII)
        characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        return g.writeCharacteristic(characteristic)
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

    /**
     * Set vibration intensity (0.0 - 1.0).
     * Maps to Lovense protocol: Vibrate:X; where X is 0-20
     */
    fun setIntensity(level: Float) {
        if (connectionState != ConnectionState.Ready) return
        val lovenseLevel = (level * 20f).toInt().coerceIn(0, 20)
        sendCommand(LovenseProtocol.vibrate(lovenseLevel))
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
    }
}
