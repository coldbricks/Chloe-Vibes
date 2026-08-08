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
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.mutableStateListOf
import androidx.core.content.ContextCompat
import com.ashairfoil.chloevibes.audio.AudioCaptureManager
import com.ashairfoil.chloevibes.audio.EnvelopeState
import com.ashairfoil.chloevibes.audio.NUM_BANDS
import com.ashairfoil.chloevibes.audio.findPreset
import com.ashairfoil.chloevibes.device.BleDeviceInfo
import com.ashairfoil.chloevibes.device.BleDeviceManager
import com.ashairfoil.chloevibes.ui.ChloeVibesTheme
import com.ashairfoil.chloevibes.ui.MainScreen
import com.ashairfoil.chloevibes.ui.MainScreenState

class MainActivity : ComponentActivity() {

    private lateinit var chloeVibesApplication: ChloeVibesApplication
    private lateinit var audioCaptureManager: AudioCaptureManager
    private lateinit var bleDeviceManager: BleDeviceManager
    private val uiState = MainScreenState()
    private val handler = Handler(Looper.getMainLooper())
    private val discoveredDevices = mutableStateListOf<BleDeviceInfo>()

    // UI update runnable (~30Hz)
    private val uiUpdateRunnable = object : Runnable {
        override fun run() {
            uiState.isCapturing = audioCaptureManager.isRunning
            if (audioCaptureManager.isRunning) {
                val state = audioCaptureManager.state
                uiState.currentOutput = state.lastFinalOutput
                uiState.gateOpen = state.lastGateOpen
                uiState.envelopeState = state.lastEnvelopeState
                uiState.bandEnergies = state.lastSpectralData.bandEnergies.copyOf()
                uiState.climaxPhase = state.lastClimaxPhase
            } else {
                uiState.currentOutput = 0f
                uiState.climaxPhase = 0f
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

        chloeVibesApplication = application as ChloeVibesApplication
        audioCaptureManager = chloeVibesApplication.audioCaptureManager
        bleDeviceManager = chloeVibesApplication.bleDeviceManager
        uiState.connectionState = bleDeviceManager.connectionState
        uiState.connectedDeviceName = bleDeviceManager.connectedDeviceName
        uiState.batteryLevel = bleDeviceManager.batteryLevel
        discoveredDevices.addAll(bleDeviceManager.getDiscoveredDevices())

        // Wire BLE callbacks
        bleDeviceManager.onDeviceDiscovered = { device ->
            handler.post {
                // Replace-or-add so late-arriving names and fresher RSSI
                // readings update the entry instead of being dropped.
                val index = discoveredDevices.indexOfFirst { it.address == device.address }
                if (index >= 0) {
                    discoveredDevices[index] = device
                } else {
                    discoveredDevices.add(device)
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
        // Dead-man / emergency-stop surface (Application → UI).
        chloeVibesApplication.onSafetyStateChanged = { safety ->
            handler.post {
                uiState.watchdogTripped = safety.watchdogTripped
                uiState.safetyMessage = safety.message
                if (safety.motorsForcedZero) {
                    uiState.isCapturing = audioCaptureManager.isRunning
                    uiState.currentOutput = 0f
                    uiState.gateOpen = false
                    uiState.envelopeState = EnvelopeState.Idle
                    uiState.bandEnergies = FloatArray(NUM_BANDS)
                }
            }
        }
        // Restore sticky banner if Activity was recreated mid-trip.
        chloeVibesApplication.currentSafetyUiState().let { safety ->
            uiState.watchdogTripped = safety.watchdogTripped
            uiState.safetyMessage = safety.message
        }

        // Apply default preset
        val defaultPreset = findPreset("Bass Drum")
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
                    onStopAll = { emergencyStopAll() },
                    onScanDevices = { scanForDevices() },
                    onConnectDevice = { address ->
                        chloeVibesApplication.armConnectedDeviceForCompanion()
                        bleDeviceManager.connect(address)
                    },
                    onDisconnectDevice = {
                        chloeVibesApplication.disarmCompanion()
                        bleDeviceManager.disconnect()
                    },
                    onClimaxReset = {
                        audioCaptureManager.state.climaxEngine.reset(
                            System.currentTimeMillis().toFloat()
                        )
                        uiState.climaxPhase = 0f
                    },
                    discoveredDevices = discoveredDevices,
                    onParameterChanged = { syncParamsToCapture() }
                )
            }
        }
    }

    override fun onDestroy() {
        handler.removeCallbacks(uiUpdateRunnable)
        bleDeviceManager.onDeviceDiscovered = null
        bleDeviceManager.onConnectionStateChanged = null
        bleDeviceManager.onBatteryUpdate = null
        chloeVibesApplication.onSafetyStateChanged = null
        if (isFinishing) chloeVibesApplication.disconnectIfNoCompanionSession()
        super.onDestroy()
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
        uiState.isCapturing = chloeVibesApplication.startAudioCapture()
        if (uiState.isCapturing) {
            // Successful re-arm clears a prior dead-man banner.
            uiState.watchdogTripped = false
            uiState.safetyMessage = null
        } else {
            val safety = chloeVibesApplication.currentSafetyUiState()
            uiState.watchdogTripped = safety.watchdogTripped
            uiState.safetyMessage = safety.message
        }
        applyKeepScreenOn(uiState.isCapturing)
    }

    private fun stopCapture() {
        chloeVibesApplication.stopAudioCapture()
        uiState.isCapturing = false
        uiState.currentOutput = 0f
        uiState.gateOpen = false
        uiState.envelopeState = EnvelopeState.Idle
        uiState.bandEnergies = FloatArray(NUM_BANDS)
        uiState.climaxPhase = 0f
        applyKeepScreenOn(false)
    }

    /** Sticky safety rail: zero motors + stop audio capture if it owns output. */
    private fun emergencyStopAll() {
        chloeVibesApplication.emergencyStopAll()
        uiState.isCapturing = false
        uiState.currentOutput = 0f
        uiState.gateOpen = false
        uiState.envelopeState = EnvelopeState.Idle
        uiState.bandEnergies = FloatArray(NUM_BANDS)
        uiState.climaxPhase = 0f
        val safety = chloeVibesApplication.currentSafetyUiState()
        uiState.watchdogTripped = safety.watchdogTripped
        uiState.safetyMessage = safety.message
        applyKeepScreenOn(false)
    }

    private fun applyKeepScreenOn(keepOn: Boolean) {
        if (keepOn) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
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
            thresholdKnee = uiState.thresholdKnee
            triggerMode = uiState.triggerMode
            binaryLevel = uiState.binaryLevel
            hybridBlend = uiState.hybridBlend
            dynamicCurve = uiState.dynamicCurve
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
            // Android 12+: Nearby Devices permissions only. The manifest
            // asserts neverForLocation on BLUETOOTH_SCAN, so no Location
            // permission (and no GPS) is needed to scan.
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
