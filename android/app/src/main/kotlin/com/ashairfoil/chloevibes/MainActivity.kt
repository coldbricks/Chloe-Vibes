// ==========================================================================
// MainActivity.kt -- Main Activity
//
// Handles runtime permissions (BLE + audio), wires the signal processing
// pipeline to the UI and BLE device manager.
// ==========================================================================

package com.ashairfoil.chloevibes

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.*
import androidx.core.content.ContextCompat
import com.ashairfoil.chloevibes.audio.*
import com.ashairfoil.chloevibes.device.BleDeviceManager
import com.ashairfoil.chloevibes.device.ConnectionState
import com.ashairfoil.chloevibes.ui.ChloeVibesTheme
import com.ashairfoil.chloevibes.ui.MainScreen
import com.ashairfoil.chloevibes.ui.MainScreenState

class MainActivity : ComponentActivity() {

    private lateinit var audioCaptureManager: AudioCaptureManager
    private lateinit var bleDeviceManager: BleDeviceManager
    private val uiState = MainScreenState()
    private val handler = Handler(Looper.getMainLooper())
    private val discoveredDevices = mutableStateListOf<Pair<String, String>>()

    // UI update runnable (~30Hz)
    private val uiUpdateRunnable = object : Runnable {
        override fun run() {
            if (audioCaptureManager.isRunning) {
                val state = audioCaptureManager.state
                uiState.currentOutput = state.lastFinalOutput
                uiState.gateOpen = state.lastGateOpen
                uiState.envelopeState = state.lastEnvelopeState
                uiState.bandEnergies = state.lastSpectralData.bandEnergies.copyOf()
            }
            handler.postDelayed(this, 33) // ~30Hz UI updates
        }
    }

    // Permission launcher
    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        val audioGranted = permissions[Manifest.permission.RECORD_AUDIO] == true
        if (audioGranted) {
            startCapture()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        audioCaptureManager = AudioCaptureManager(this)
        bleDeviceManager = BleDeviceManager(this)

        // Wire BLE device output to haptic device
        audioCaptureManager.onOutputUpdate = { output ->
            bleDeviceManager.setIntensity(output)
        }

        // Wire BLE callbacks
        bleDeviceManager.onDeviceDiscovered = { device ->
            handler.post {
                val entry = Pair(device.name, device.address)
                if (discoveredDevices.none { it.second == device.address }) {
                    discoveredDevices.add(entry)
                }
            }
        }
        bleDeviceManager.onConnectionStateChanged = { state ->
            handler.post {
                uiState.connectionState = state
                uiState.connectedDeviceName = bleDeviceManager.connectedDeviceName
            }
        }
        bleDeviceManager.onBatteryUpdate = { level ->
            handler.post { uiState.batteryLevel = level }
        }

        // Apply default preset
        val defaultPreset = findPreset("Ride Intensity")
        if (defaultPreset != null) {
            uiState.applyPreset(defaultPreset)
            audioCaptureManager.applyPreset(defaultPreset)
        }

        // Start UI update loop
        handler.post(uiUpdateRunnable)

        setContent {
            ChloeVibesTheme {
                MainScreen(
                    state = uiState,
                    onPresetSelected = { preset ->
                        uiState.applyPreset(preset)
                        audioCaptureManager.applyPreset(preset)
                        syncParamsToCapture()
                    },
                    onStartCapture = { requestPermissionsAndStart() },
                    onStopCapture = { stopCapture() },
                    onScanDevices = { scanForDevices() },
                    onConnectDevice = { address -> bleDeviceManager.connect(address) },
                    onDisconnectDevice = { bleDeviceManager.disconnect() },
                    discoveredDevices = discoveredDevices,
                    onParameterChanged = { syncParamsToCapture() }
                )
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        handler.removeCallbacks(uiUpdateRunnable)
        audioCaptureManager.stop()
        bleDeviceManager.disconnect()
    }

    // -----------------------------------------------------------------------
    // Audio capture
    // -----------------------------------------------------------------------

    private fun requestPermissionsAndStart() {
        val needed = mutableListOf<String>()

        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) {
            needed.add(Manifest.permission.RECORD_AUDIO)
        }

        if (needed.isEmpty()) {
            startCapture()
        } else {
            permissionLauncher.launch(needed.toTypedArray())
        }
    }

    private fun startCapture() {
        syncParamsToCapture()
        val started = audioCaptureManager.start(AudioSourceMode.SystemAudio)
        if (!started) {
            // Fall back to microphone
            audioCaptureManager.start(AudioSourceMode.Microphone)
        }
        uiState.isCapturing = audioCaptureManager.isRunning
    }

    private fun stopCapture() {
        audioCaptureManager.stop()
        uiState.isCapturing = false
        uiState.currentOutput = 0f
        uiState.gateOpen = false
        uiState.envelopeState = EnvelopeState.Idle
        uiState.bandEnergies = FloatArray(NUM_BANDS)
    }

    /** Push all UI parameter values into the AudioCaptureManager. */
    private fun syncParamsToCapture() {
        audioCaptureManager.apply {
            mainVolume = uiState.mainVolume
            frequencyMode = uiState.frequencyMode
            targetFrequency = uiState.targetFrequency
            gateThreshold = uiState.gateThreshold
            autoGateAmount = uiState.autoGateAmount
            gateSmoothing = uiState.gateSmoothing
            triggerMode = uiState.triggerMode
            binaryLevel = uiState.binaryLevel
            hybridBlend = uiState.hybridBlend
            attackMs = uiState.attackMs
            decayMs = uiState.decayMs
            sustainLevel = uiState.sustainLevel
            releaseMs = uiState.releaseMs
            attackCurve = uiState.attackCurve
            decayCurve = uiState.decayCurve
            releaseCurve = uiState.releaseCurve
            minVibe = uiState.minVibe
            maxVibe = uiState.maxVibe
            outputGain = uiState.outputGain
            climaxEnabled = uiState.climaxEnabled
            climaxIntensity = uiState.climaxIntensity
            climaxBuildUpMs = uiState.climaxBuildUpMs
            climaxTeaseRatio = uiState.climaxTeaseRatio
            climaxTeaseDrop = uiState.climaxTeaseDrop
            climaxSurgeBoost = uiState.climaxSurgeBoost
            climaxPulseDepth = uiState.climaxPulseDepth
            climaxPattern = uiState.climaxPattern
        }
    }

    // -----------------------------------------------------------------------
    // BLE scanning
    // -----------------------------------------------------------------------

    private fun scanForDevices() {
        val needed = mutableListOf<String>()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Android 12+: need BLUETOOTH_SCAN and BLUETOOTH_CONNECT
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.BLUETOOTH_SCAN)
                != PackageManager.PERMISSION_GRANTED
            ) {
                needed.add(Manifest.permission.BLUETOOTH_SCAN)
            }
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.BLUETOOTH_CONNECT)
                != PackageManager.PERMISSION_GRANTED
            ) {
                needed.add(Manifest.permission.BLUETOOTH_CONNECT)
            }
        } else {
            // Pre-Android 12: BLE scanning requires ACCESS_FINE_LOCATION
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION)
                != PackageManager.PERMISSION_GRANTED
            ) {
                needed.add(Manifest.permission.ACCESS_FINE_LOCATION)
            }
        }

        if (needed.isEmpty()) {
            discoveredDevices.clear()
            bleDeviceManager.startScan()
        } else {
            bleScanPermissionLauncher.launch(needed.toTypedArray())
        }
    }

    private val bleScanPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        val allGranted = permissions.values.all { it }
        if (allGranted) {
            discoveredDevices.clear()
            bleDeviceManager.startScan()
        }
    }
}
