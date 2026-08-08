// ==========================================================================
// MainScreen.kt -- Main Compose screen
//
// Provides: preset selector, ADSR sliders, output visualization,
// device connection status, climax engine controls.
// Signal chain: Audio In -> Freq Filter -> Gate -> Trigger -> ADSR -> Output -> Device
// ==========================================================================

package com.ashairfoil.chloevibes.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BluetoothDisabled
import androidx.compose.material.icons.filled.BluetoothSearching
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke as DrawStroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.ashairfoil.chloevibes.audio.BAND_NAMES
import com.ashairfoil.chloevibes.audio.ClimaxPattern
import com.ashairfoil.chloevibes.audio.EnvelopeState
import com.ashairfoil.chloevibes.audio.FrequencyMode
import com.ashairfoil.chloevibes.audio.NUM_BANDS
import com.ashairfoil.chloevibes.audio.Preset
import com.ashairfoil.chloevibes.audio.PresetCategory
import com.ashairfoil.chloevibes.audio.TriggerMode
import com.ashairfoil.chloevibes.audio.applyCurve
import com.ashairfoil.chloevibes.audio.presetsInCategory
import com.ashairfoil.chloevibes.device.BleDeviceInfo
import com.ashairfoil.chloevibes.device.ConnectionState
import com.ashairfoil.chloevibes.device.LovenseProtocol
import kotlin.math.exp
import kotlin.math.ln
import kotlin.math.pow

// ---------------------------------------------------------------------------
// ViewModel state
// ---------------------------------------------------------------------------

class MainScreenState {
    // Preset
    var selectedPresetName by mutableStateOf("Bass Drum")
    var selectedCategory by mutableStateOf(PresetCategory.Init)

    // Input — bass-drum boom defaults
    var mainVolume by mutableFloatStateOf(1.90f)
    var frequencyMode by mutableStateOf(FrequencyMode.LowPass)
    var targetFrequency by mutableFloatStateOf(120f)

    // Gate
    var gateThreshold by mutableFloatStateOf(0.14f)
    var autoGateAmount by mutableFloatStateOf(0f)
    var gateSmoothing by mutableFloatStateOf(0.06f)
    var thresholdKnee by mutableFloatStateOf(0.15f)

    // Trigger
    var triggerMode by mutableStateOf(TriggerMode.Hybrid)
    var binaryLevel by mutableFloatStateOf(0.82f)
    var hybridBlend by mutableFloatStateOf(0.58f)
    var dynamicCurve by mutableFloatStateOf(1.2f)

    // ADSR — instant peak → long exp decay → near-zero floor (~125 BPM)
    var attackMs by mutableFloatStateOf(20f)
    var decayMs by mutableFloatStateOf(375f)
    var sustainLevel by mutableFloatStateOf(0.08f)
    var releaseMs by mutableFloatStateOf(240f)
    var attackCurve by mutableFloatStateOf(0.7f)
    var decayCurve by mutableFloatStateOf(1.8f)
    var releaseCurve by mutableFloatStateOf(1.3f)

    // Output
    var minVibe by mutableFloatStateOf(0f)
    var maxVibe by mutableFloatStateOf(1f)
    var outputGain by mutableFloatStateOf(1f)

    // Climax
    var climaxEnabled by mutableStateOf(false)
    var climaxIntensity by mutableFloatStateOf(0.7f)
    var climaxBuildUpMs by mutableFloatStateOf(90_000f)
    var climaxTeaseRatio by mutableFloatStateOf(0.18f)
    var climaxTeaseDrop by mutableFloatStateOf(0.35f)
    var climaxSurgeBoost by mutableFloatStateOf(0.5f)
    var climaxPulseDepth by mutableFloatStateOf(0.18f)
    var climaxPattern by mutableStateOf(ClimaxPattern.Wave)
    /** Live cycle progress [0, 1) from the engine (UI only). */
    var climaxPhase by mutableFloatStateOf(0f)

    // Live readouts
    var currentOutput by mutableFloatStateOf(0f)
    var gateOpen by mutableStateOf(false)
    var envelopeState by mutableStateOf(EnvelopeState.Idle)
    var bandEnergies by mutableStateOf(FloatArray(NUM_BANDS))

    // Device
    var connectionState by mutableStateOf(ConnectionState.Disconnected)
    var connectedDeviceName by mutableStateOf<String?>(null)
    var batteryLevel by mutableIntStateOf(-1)
    var isCapturing by mutableStateOf(false)

    // Safety (audio-path dead-man + emergency stop)
    var watchdogTripped by mutableStateOf(false)
    var safetyMessage by mutableStateOf<String?>(null)

    fun applyPreset(preset: Preset) {
        selectedPresetName = preset.name
        mainVolume = preset.mainVolume
        frequencyMode = preset.frequencyMode
        targetFrequency = preset.targetFrequency
        gateThreshold = preset.gateThreshold
        autoGateAmount = preset.autoGateAmount
        gateSmoothing = preset.gateSmoothing
        thresholdKnee = preset.thresholdKnee
        triggerMode = preset.triggerMode
        binaryLevel = preset.binaryLevel
        hybridBlend = preset.hybridBlend
        dynamicCurve = preset.dynamicCurve
        attackMs = preset.attackMs
        decayMs = preset.decayMs
        sustainLevel = preset.sustainLevel
        releaseMs = preset.releaseMs
        attackCurve = preset.attackCurve
        decayCurve = preset.decayCurve
        releaseCurve = preset.releaseCurve
        minVibe = preset.minVibe
        maxVibe = preset.maxVibe
        climaxEnabled = preset.climaxEnabled
        climaxIntensity = preset.climaxIntensity
        climaxBuildUpMs = preset.climaxBuildUpMs
        climaxTeaseRatio = preset.climaxTeaseRatio
        climaxTeaseDrop = preset.climaxTeaseDrop
        climaxSurgeBoost = preset.climaxSurgeBoost
        climaxPulseDepth = preset.climaxPulseDepth
        climaxPattern = preset.climaxPattern
    }
}

// ---------------------------------------------------------------------------
// Main Screen
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainScreen(
    state: MainScreenState,
    onPresetSelected: (Preset) -> Unit,
    onStartCapture: () -> Unit,
    onStopCapture: () -> Unit,
    onStopAll: () -> Unit,
    onScanDevices: () -> Unit,
    onConnectDevice: (String) -> Unit,
    onDisconnectDevice: () -> Unit,
    onClimaxReset: () -> Unit,
    discoveredDevices: List<BleDeviceInfo>,
    onParameterChanged: () -> Unit
) {
    val scrollState = rememberScrollState()
    var showClimaxOffConfirm by remember { mutableStateOf(false) }
    var expertOpen by remember { mutableStateOf(false) }
    var climaxFineOpen by remember { mutableStateOf(false) }

    fun setClimaxEnabled(enabled: Boolean) {
        if (!enabled && state.climaxEnabled) {
            // Guard: accidental mid-edge disable is hard to undo under arousal.
            showClimaxOffConfirm = true
            return
        }
        state.climaxEnabled = enabled
        onParameterChanged()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ChloeColors.Background)
    ) {
        Column(
            modifier = Modifier
                .weight(1f)
                .verticalScroll(scrollState)
                .padding(horizontal = 16.dp, vertical = 12.dp)
        ) {
            // Header
            Text(
                text = "CHLOE VIBES",
                color = ChloeColors.Purple,
                fontSize = 24.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 4.sp
            )
            Text(
                text = "Spectral Haptics Engine",
                color = ChloeColors.OnSurfaceDim,
                fontSize = 12.sp,
                letterSpacing = 2.sp
            )

            Spacer(modifier = Modifier.height(12.dp))

            // Output meter + device status
            OutputMeter(
                output = state.currentOutput,
                gateOpen = state.gateOpen,
                envelopeState = state.envelopeState,
                connectionState = state.connectionState,
                deviceName = state.connectedDeviceName,
                batteryLevel = state.batteryLevel,
                isCapturing = state.isCapturing,
                watchdogTripped = state.watchdogTripped
            )

            // Sticky safety banner when dead-man trips or emergency stop reports.
            val safetyMsg = state.safetyMessage
            if (state.watchdogTripped || safetyMsg != null) {
                Spacer(modifier = Modifier.height(8.dp))
                SafetyBanner(
                    tripped = state.watchdogTripped,
                    message = safetyMsg ?: "Motors forced to zero"
                )
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Controls row: Start/Stop, Scan, Connect
            ControlsRow(
                isCapturing = state.isCapturing,
                connectionState = state.connectionState,
                onStartCapture = onStartCapture,
                onStopCapture = onStopCapture,
                onScanDevices = onScanDevices,
                onDisconnectDevice = onDisconnectDevice,
                discoveredDevices = discoveredDevices,
                onConnectDevice = onConnectDevice
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Spectrum visualizer
            SpectrumVisualizer(bandEnergies = state.bandEnergies)

            Spacer(modifier = Modifier.height(16.dp))

            // Preset path (Android stand-in for desktop FIND BOOM + presets)
            SectionHeader("PRESETS")
            Text(
                "Pick a starting point — big cards, one tap. FIND BOOM is desktop-only.",
                color = ChloeColors.OnSurfaceDim,
                fontSize = 11.sp,
                modifier = Modifier.padding(bottom = 6.dp)
            )
            PresetSelector(
                selectedCategory = state.selectedCategory,
                selectedPresetName = state.selectedPresetName,
                onCategorySelected = { state.selectedCategory = it },
                onPresetSelected = onPresetSelected
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Promoted essentials under arousal: Volume + Ceiling
            SectionHeader("INTENSITY")
            LabeledSlider(
                "Volume",
                state.mainVolume,
                0f,
                10f,
                "%.2f",
                ChloeColors.Teal,
                taper = 3f,
                large = true
            ) {
                state.mainVolume = it; state.selectedPresetName = "Custom"; onParameterChanged()
            }
            CeilingControl(
                maxVibe = state.maxVibe,
                minVibe = state.minVibe,
                onCeilingChange = { ceiling ->
                    state.maxVibe = ceiling
                    if (state.minVibe > state.maxVibe) state.minVibe = state.maxVibe
                    state.selectedPresetName = "Custom"
                    onParameterChanged()
                }
            )

            Spacer(modifier = Modifier.height(16.dp))

            // CLIMAX — primary path; fine knobs collapsed
            SectionHeader("CLIMAX")
            ClimaxArmRow(
                enabled = state.climaxEnabled,
                phase = state.climaxPhase,
                onRequestEnable = { setClimaxEnabled(true) },
                onRequestDisable = { setClimaxEnabled(false) },
                onReset = onClimaxReset
            )

            if (state.climaxEnabled) {
                Spacer(modifier = Modifier.height(8.dp))
                ClimaxPatternSelector(state.climaxPattern) {
                    state.climaxPattern = it; onParameterChanged()
                }
                LabeledSlider(
                    "Intensity",
                    state.climaxIntensity,
                    0f,
                    1f,
                    "%.2f",
                    ChloeColors.Pink,
                    large = true
                ) {
                    state.climaxIntensity = it; onParameterChanged()
                }
                // Build cycle is stored as ms; UI is true seconds (8–240 s).
                LabeledSlider(
                    "Build Cycle",
                    state.climaxBuildUpMs / 1000f,
                    8f,
                    240f,
                    "%.0f s",
                    ChloeColors.Pink,
                    large = true
                ) {
                    state.climaxBuildUpMs = it * 1000f; onParameterChanged()
                }

                Text(
                    if (climaxFineOpen) "Hide fine tune ▴" else "Fine tune (tease / surge) ▾",
                    color = ChloeColors.OnSurfaceDim,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier
                        .padding(top = 4.dp, bottom = 4.dp)
                        .clickable { climaxFineOpen = !climaxFineOpen }
                        .padding(vertical = 8.dp)
                )
                if (climaxFineOpen) {
                    LabeledSlider("Tease Ratio", state.climaxTeaseRatio, 0.05f, 0.5f, "%.2f", ChloeColors.Pink) {
                        state.climaxTeaseRatio = it; onParameterChanged()
                    }
                    LabeledSlider("Tease Drop", state.climaxTeaseDrop, 0f, 0.9f, "%.2f") {
                        state.climaxTeaseDrop = it; onParameterChanged()
                    }
                    LabeledSlider("Surge Boost", state.climaxSurgeBoost, 0f, 1.2f, "%.2f") {
                        state.climaxSurgeBoost = it; onParameterChanged()
                    }
                    LabeledSlider("Pulse Depth", state.climaxPulseDepth, 0f, 0.45f, "%.2f") {
                        state.climaxPulseDepth = it; onParameterChanged()
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Expert path collapsed by default — less clutter mid-session
            Text(
                if (expertOpen) "Hide expert knobs ▴" else "Expert knobs (gate / ADSR / gain) ▾",
                color = ChloeColors.OnSurfaceDim,
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
                letterSpacing = 0.5.sp,
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { expertOpen = !expertOpen }
                    .padding(vertical = 10.dp)
            )

            if (expertOpen) {
                // INPUT section
                SectionHeader("INPUT")
                FrequencyModeSelector(state.frequencyMode) {
                    state.frequencyMode = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                if (state.frequencyMode != FrequencyMode.Full) {
                    LabeledSlider(
                        "Target Freq",
                        state.targetFrequency,
                        20f,
                        16000f,
                        "%.0f Hz",
                        logarithmic = true
                    ) {
                        state.targetFrequency = it; state.selectedPresetName = "Custom"; onParameterChanged()
                    }
                }

                Spacer(modifier = Modifier.height(12.dp))

                // GATE section
                SectionHeader("GATE", trailingContent = {
                    GateIndicator(state.gateOpen)
                })
                LabeledSlider("Threshold", state.gateThreshold, 0f, 1f, "%.2f") {
                    state.gateThreshold = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Auto-Sense", state.autoGateAmount, 0f, 1f, "%.2f") {
                    state.autoGateAmount = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Smooth", state.gateSmoothing, 0f, 1f, "%.2f") {
                    state.gateSmoothing = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Knee", state.thresholdKnee, 0f, 0.35f, "%.2f") {
                    state.thresholdKnee = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }

                Spacer(modifier = Modifier.height(12.dp))

                // TRIGGER section
                SectionHeader("TRIGGER")
                TriggerModeSelector(state.triggerMode) {
                    state.triggerMode = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Dynamic Curve", state.dynamicCurve, 0.4f, 2.4f, "%.2f") {
                    state.dynamicCurve = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                if (state.triggerMode == TriggerMode.Binary || state.triggerMode == TriggerMode.Hybrid) {
                    LabeledSlider("Binary Level", state.binaryLevel, 0f, 1f, "%.2f") {
                        state.binaryLevel = it; state.selectedPresetName = "Custom"; onParameterChanged()
                    }
                }
                if (state.triggerMode == TriggerMode.Hybrid) {
                    LabeledSlider("Hybrid Blend", state.hybridBlend, 0f, 1f, "%.2f") {
                        state.hybridBlend = it; state.selectedPresetName = "Custom"; onParameterChanged()
                    }
                }

                Spacer(modifier = Modifier.height(12.dp))

                // ENVELOPE section (color-coded ADSR)
                SectionHeader("ENVELOPE")
                LabeledSlider(
                    "Attack", state.attackMs, 0.5f, 500f, "%.0f ms", ChloeColors.Attack,
                    logarithmic = true
                ) {
                    state.attackMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider(
                    "Decay", state.decayMs, 1f, 1000f, "%.0f ms", ChloeColors.Decay,
                    logarithmic = true
                ) {
                    state.decayMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Sustain", state.sustainLevel, 0f, 1f, "%.2f", ChloeColors.Sustain) {
                    state.sustainLevel = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider(
                    "Release", state.releaseMs, 1f, 2000f, "%.0f ms", ChloeColors.Release,
                    logarithmic = true
                ) {
                    state.releaseMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }

                Spacer(modifier = Modifier.height(10.dp))

                EnvelopeScopeView(
                    attackMs = state.attackMs,
                    decayMs = state.decayMs,
                    sustainLevel = state.sustainLevel,
                    releaseMs = state.releaseMs,
                    attackCurve = state.attackCurve,
                    decayCurve = state.decayCurve,
                    releaseCurve = state.releaseCurve
                )

                Spacer(modifier = Modifier.height(8.dp))
                Text("Curves", color = ChloeColors.OnSurfaceDim, fontSize = 11.sp, letterSpacing = 1.sp)
                LabeledSlider("A Curve", state.attackCurve, 0.1f, 3f, "%.2f", ChloeColors.Attack) {
                    state.attackCurve = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("D Curve", state.decayCurve, 0.1f, 3f, "%.2f", ChloeColors.Decay) {
                    state.decayCurve = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("R Curve", state.releaseCurve, 0.1f, 3f, "%.2f", ChloeColors.Release) {
                    state.releaseCurve = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }

                Spacer(modifier = Modifier.height(12.dp))

                // OUTPUT section (gain + floor; ceiling lives in INTENSITY)
                SectionHeader("OUTPUT")
                LabeledSlider("Gain", state.outputGain, 0f, 20f, "%.1f", ChloeColors.Pink, taper = 3f) {
                    state.outputGain = it; state.selectedPresetName = "Custom"; onParameterChanged()
                }
                LabeledSlider("Floor", state.minVibe, 0f, 1f, "%.2f") { floor ->
                    state.minVibe = floor.coerceAtMost(state.maxVibe)
                    state.selectedPresetName = "Custom"
                    onParameterChanged()
                }
            }

            Spacer(modifier = Modifier.height(16.dp))
        }

        // Sticky bottom safety rail — always reachable without scrolling
        SafetyRail(
            output = state.currentOutput,
            isCapturing = state.isCapturing,
            climaxEnabled = state.climaxEnabled,
            climaxPhase = state.climaxPhase,
            onStopAll = onStopAll,
            onClimaxToggleRequest = { wantOn -> setClimaxEnabled(wantOn) }
        )
    }

    if (showClimaxOffConfirm) {
        AlertDialog(
            onDismissRequest = { showClimaxOffConfirm = false },
            title = {
                Text("Turn off climax?", color = ChloeColors.OnSurface, fontWeight = FontWeight.Bold)
            },
            text = {
                Text(
                    "This resets the edge cycle mid-build. Audio-reactive intensity continues; only the climax layer stops.",
                    color = ChloeColors.OnSurfaceDim,
                    fontSize = 14.sp
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    state.climaxEnabled = false
                    onParameterChanged()
                    showClimaxOffConfirm = false
                }) {
                    Text("Turn off", color = ChloeColors.Error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showClimaxOffConfirm = false }) {
                    Text("Keep on", color = ChloeColors.Pink)
                }
            },
            containerColor = ChloeColors.Surface
        )
    }
}

// ---------------------------------------------------------------------------
// Output Meter
// ---------------------------------------------------------------------------

@Composable
private fun OutputMeter(
    output: Float,
    gateOpen: Boolean,
    envelopeState: EnvelopeState,
    connectionState: ConnectionState,
    deviceName: String?,
    batteryLevel: Int,
    isCapturing: Boolean,
    watchdogTripped: Boolean = false
) {
    val outputColor by animateColorAsState(
        targetValue = when {
            output > 0.8f -> ChloeColors.Pink
            output > 0.4f -> ChloeColors.Purple
            output > 0.01f -> ChloeColors.Teal
            else -> ChloeColors.SurfaceVariant
        },
        animationSpec = tween(150),
        label = "outputColor"
    )

    Card(
        colors = CardDefaults.cardColors(containerColor = ChloeColors.Surface),
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            // Output bar
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("OUTPUT", color = ChloeColors.OnSurfaceDim, fontSize = 11.sp, letterSpacing = 2.sp)
                Spacer(modifier = Modifier.width(12.dp))
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(24.dp)
                        .clip(RoundedCornerShape(4.dp))
                        .background(ChloeColors.SurfaceVariant)
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxHeight()
                            .fillMaxWidth(output.coerceIn(0f, 1f))
                            .background(
                                Brush.horizontalGradient(
                                    listOf(ChloeColors.Teal, ChloeColors.Purple, ChloeColors.Pink)
                                )
                            )
                    )
                }
                Spacer(modifier = Modifier.width(12.dp))
                Text(
                    "%.0f%%".format(output * 100f),
                    color = outputColor,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Bold
                )
            }

            Spacer(modifier = Modifier.height(8.dp))

            // Status row
            Row(
                horizontalArrangement = Arrangement.SpaceBetween,
                modifier = Modifier.fillMaxWidth()
            ) {
                // Envelope state
                val phaseLabel = when (envelopeState) {
                    EnvelopeState.Idle -> "IDLE"
                    EnvelopeState.Attack -> "ATK"
                    EnvelopeState.Decay -> "DEC"
                    EnvelopeState.Sustain -> "SUS"
                    EnvelopeState.Release -> "REL"
                }
                val phaseColor = when (envelopeState) {
                    EnvelopeState.Attack -> ChloeColors.Attack
                    EnvelopeState.Decay -> ChloeColors.Decay
                    EnvelopeState.Sustain -> ChloeColors.Sustain
                    EnvelopeState.Release -> ChloeColors.Release
                    EnvelopeState.Idle -> ChloeColors.OnSurfaceDim
                }
                StatusChip(phaseLabel, phaseColor)

                // Gate status
                StatusChip(
                    if (gateOpen) "OPEN" else "CLOSED",
                    if (gateOpen) ChloeColors.GateOpen else ChloeColors.GateClosed
                )

                // Capture status — larger for arousal glanceability
                StatusChip(
                    when {
                        watchdogTripped -> "WATCHDOG"
                        isCapturing -> "LIVE"
                        else -> "STOPPED"
                    },
                    when {
                        watchdogTripped -> ChloeColors.Error
                        isCapturing -> ChloeColors.Teal
                        else -> ChloeColors.OnSurfaceDim
                    },
                    large = true
                )

                // Device status
                val deviceLabel = when (connectionState) {
                    ConnectionState.Disconnected -> "NO DEVICE"
                    ConnectionState.Connecting -> "LINKING..."
                    ConnectionState.Connected -> deviceName ?: "CONNECTED"
                    ConnectionState.Ready -> deviceName ?: "READY"
                }
                val deviceColor = when (connectionState) {
                    ConnectionState.Ready -> ChloeColors.Connected
                    ConnectionState.Connected -> ChloeColors.Teal
                    ConnectionState.Connecting -> ChloeColors.Amber
                    ConnectionState.Disconnected -> ChloeColors.Disconnected
                }
                StatusChip(deviceLabel, deviceColor)
            }

            // Battery
            if (batteryLevel >= 0) {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    "Battery: $batteryLevel%",
                    color = if (batteryLevel < 20) ChloeColors.Error else ChloeColors.OnSurfaceDim,
                    fontSize = 11.sp
                )
            }
        }
    }
}

@Composable
private fun StatusChip(label: String, color: Color, large: Boolean = false) {
    Text(
        text = label,
        color = color,
        fontSize = if (large) 13.sp else 10.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 1.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis
    )
}

@Composable
private fun SafetyBanner(tripped: Boolean, message: String) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (tripped) {
                ChloeColors.Error.copy(alpha = 0.18f)
            } else {
                ChloeColors.SurfaceVariant
            }
        ),
        shape = RoundedCornerShape(8.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        Text(
            text = message,
            color = if (tripped) ChloeColors.Error else ChloeColors.OnSurface,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 10.dp)
        )
    }
}

/**
 * Sticky bottom safety rail: red Stop all is always reachable without scrolling.
 * Climax arm shortcut stays here so edge control is one thumb away.
 */
@Composable
private fun SafetyRail(
    output: Float,
    isCapturing: Boolean,
    climaxEnabled: Boolean,
    climaxPhase: Float,
    onStopAll: () -> Unit,
    onClimaxToggleRequest: (Boolean) -> Unit
) {
    val liveColor by animateColorAsState(
        targetValue = if (isCapturing) ChloeColors.Teal else ChloeColors.OnSurfaceDim,
        animationSpec = tween(150),
        label = "railLiveColor"
    )
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(ChloeColors.Surface)
            .border(1.dp, ChloeColors.SurfaceBright)
            .padding(horizontal = 12.dp, vertical = 10.dp)
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxWidth()
        ) {
            Button(
                onClick = onStopAll,
                colors = ButtonDefaults.buttonColors(
                    containerColor = ChloeColors.Error,
                    contentColor = Color.White
                ),
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier
                    .weight(1.15f)
                    .heightIn(min = 56.dp)
                    .semantics { contentDescription = "Stop all motors and capture" }
            ) {
                Icon(Icons.Default.Stop, contentDescription = null, modifier = Modifier.size(22.dp))
                Spacer(modifier = Modifier.width(6.dp))
                Text("STOP ALL", fontSize = 15.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.sp)
            }

            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
                modifier = Modifier
                    .weight(0.7f)
                    .defaultMinSize(minHeight = 56.dp)
            ) {
                Text(
                    if (isCapturing) "LIVE" else "STOPPED",
                    color = liveColor,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp
                )
                Text(
                    "%.0f%%".format(output.coerceIn(0f, 1f) * 100f),
                    color = ChloeColors.OnSurface,
                    fontSize = 18.sp,
                    fontWeight = FontWeight.Bold
                )
            }

            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
                modifier = Modifier
                    .weight(0.9f)
                    .clip(RoundedCornerShape(12.dp))
                    .background(
                        if (climaxEnabled) ChloeColors.PinkDark.copy(alpha = 0.45f)
                        else ChloeColors.SurfaceVariant
                    )
                    .border(
                        width = if (climaxEnabled) 2.dp else 1.dp,
                        color = if (climaxEnabled) ChloeColors.Pink else ChloeColors.SurfaceBright,
                        shape = RoundedCornerShape(12.dp)
                    )
                    .clickable { onClimaxToggleRequest(!climaxEnabled) }
                    .padding(horizontal = 8.dp, vertical = 8.dp)
                    .defaultMinSize(minHeight = 56.dp)
            ) {
                Text(
                    if (climaxEnabled) "CLIMAX ON" else "CLIMAX OFF",
                    color = if (climaxEnabled) ChloeColors.PinkLight else ChloeColors.OnSurfaceDim,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 0.5.sp,
                    maxLines = 1
                )
                Text(
                    if (climaxEnabled) {
                        "CYCLE %.0f%%".format(climaxPhase.coerceIn(0f, 1f) * 100f)
                    } else {
                        "tap to arm"
                    },
                    color = if (climaxEnabled) Color.White else ChloeColors.OnSurfaceDim,
                    fontSize = if (climaxEnabled) 13.sp else 11.sp,
                    fontWeight = if (climaxEnabled) FontWeight.SemiBold else FontWeight.Normal
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ceiling control — large target + quick 50/75/100 presets
// ---------------------------------------------------------------------------

@Composable
private fun CeilingControl(
    maxVibe: Float,
    minVibe: Float,
    onCeilingChange: (Float) -> Unit
) {
    // Slider works in percent space so the readout is arousal-proof ("75" not "0.75").
    val ceilingPct = (maxVibe.coerceIn(0f, 1f) * 100f)
    Column(modifier = Modifier.fillMaxWidth()) {
        LabeledSlider(
            label = "Ceiling",
            value = ceilingPct,
            min = 0f,
            max = 100f,
            format = "%.0f%%",
            accentColor = ChloeColors.Amber,
            large = true
        ) { pct ->
            onCeilingChange((pct / 100f).coerceIn(0f, 1f))
        }

        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 2.dp, bottom = 2.dp)
        ) {
            listOf(0.50f to "50%", 0.75f to "75%", 1.0f to "100%").forEach { (level, label) ->
                val selected = kotlin.math.abs(maxVibe - level) < 0.02f
                FilterChip(
                    selected = selected,
                    onClick = { onCeilingChange(level.coerceAtLeast(minVibe)) },
                    label = {
                        Text(
                            label,
                            fontSize = 13.sp,
                            fontWeight = FontWeight.Bold,
                            modifier = Modifier.padding(horizontal = 4.dp, vertical = 2.dp)
                        )
                    },
                    modifier = Modifier
                        .weight(1f)
                        .heightIn(min = 44.dp),
                    colors = FilterChipDefaults.filterChipColors(
                        selectedContainerColor = ChloeColors.Amber,
                        selectedLabelColor = Color.Black,
                        containerColor = ChloeColors.SurfaceVariant,
                        labelColor = ChloeColors.OnSurface
                    )
                )
            }
        }
        if (minVibe > 0.001f) {
            Text(
                "Floor %.0f%% · troughs stay raised".format(minVibe * 100f),
                color = ChloeColors.OnSurfaceDim,
                fontSize = 11.sp
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Climax arm row — big active state + cycle % + reset
// ---------------------------------------------------------------------------

@Composable
private fun ClimaxArmRow(
    enabled: Boolean,
    phase: Float,
    onRequestEnable: () -> Unit,
    onRequestDisable: () -> Unit,
    onReset: () -> Unit
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(
                if (enabled) ChloeColors.PinkDark.copy(alpha = 0.35f) else ChloeColors.SurfaceVariant
            )
            .border(
                width = if (enabled) 2.dp else 1.dp,
                color = if (enabled) ChloeColors.Pink else ChloeColors.SurfaceBright,
                shape = RoundedCornerShape(12.dp)
            )
            .padding(horizontal = 12.dp, vertical = 10.dp)
    ) {
        Switch(
            checked = enabled,
            onCheckedChange = { want ->
                if (want) onRequestEnable() else onRequestDisable()
            },
            colors = SwitchDefaults.colors(
                checkedTrackColor = ChloeColors.Pink,
                checkedThumbColor = Color.White,
                uncheckedTrackColor = ChloeColors.SurfaceBright,
                uncheckedThumbColor = ChloeColors.OnSurfaceDim
            ),
            modifier = Modifier.semantics {
                contentDescription = if (enabled) "Climax on" else "Climax off"
            }
        )
        Spacer(modifier = Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                if (enabled) "ACTIVE" else "OFF",
                color = if (enabled) ChloeColors.PinkLight else ChloeColors.OnSurfaceDim,
                fontSize = 16.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 1.sp
            )
            Text(
                if (enabled) {
                    "Cycle %.0f%% · disable asks first".format(phase.coerceIn(0f, 1f) * 100f)
                } else {
                    "Arm for edge build / tease / surge"
                },
                color = ChloeColors.OnSurfaceDim,
                fontSize = 11.sp
            )
        }
        if (enabled) {
            TextButton(
                onClick = onReset,
                modifier = Modifier
                    .heightIn(min = 48.dp)
                    .padding(start = 4.dp)
            ) {
                Text("RESET", color = ChloeColors.Pink, fontWeight = FontWeight.Bold, fontSize = 13.sp)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Controls Row
// ---------------------------------------------------------------------------

@Composable
private fun ControlsRow(
    isCapturing: Boolean,
    connectionState: ConnectionState,
    onStartCapture: () -> Unit,
    onStopCapture: () -> Unit,
    onScanDevices: () -> Unit,
    onDisconnectDevice: () -> Unit,
    discoveredDevices: List<BleDeviceInfo>,
    onConnectDevice: (String) -> Unit
) {
    var showDeviceDialog by remember { mutableStateOf(false) }

    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        // Start/Stop capture
        Button(
            onClick = if (isCapturing) onStopCapture else onStartCapture,
            colors = ButtonDefaults.buttonColors(
                containerColor = if (isCapturing) ChloeColors.SurfaceVariant else ChloeColors.Teal,
                contentColor = if (isCapturing) ChloeColors.OnSurface else Color.Black
            ),
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier
                .weight(1f)
                .heightIn(min = 52.dp)
        ) {
            Icon(
                if (isCapturing) Icons.Default.Stop else Icons.Default.PlayArrow,
                contentDescription = null,
                modifier = Modifier.size(22.dp)
            )
            Spacer(modifier = Modifier.width(6.dp))
            Text(
                if (isCapturing) "Stop capture" else "Start",
                fontSize = 14.sp,
                fontWeight = FontWeight.Bold
            )
        }

        // Scan / Connect
        if (connectionState == ConnectionState.Ready || connectionState == ConnectionState.Connected) {
            Button(
                onClick = onDisconnectDevice,
                colors = ButtonDefaults.buttonColors(containerColor = ChloeColors.SurfaceVariant),
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 52.dp)
            ) {
                Icon(Icons.Default.BluetoothDisabled, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(modifier = Modifier.width(6.dp))
                Text("Disconnect", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            }
        } else {
            Button(
                onClick = {
                    onScanDevices()
                    showDeviceDialog = true
                },
                colors = ButtonDefaults.buttonColors(containerColor = ChloeColors.Purple),
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 52.dp)
            ) {
                Icon(Icons.Default.BluetoothSearching, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(modifier = Modifier.width(6.dp))
                Text("Scan", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }

    // Device picker dialog
    if (showDeviceDialog) {
        AlertDialog(
            onDismissRequest = { showDeviceDialog = false },
            title = { Text("Select Device") },
            text = {
                Column(modifier = Modifier.verticalScroll(rememberScrollState())) {
                    if (discoveredDevices.isEmpty()) {
                        Text("Scanning for devices...", color = ChloeColors.OnSurfaceDim)
                    }
                    // Stable keys only: sorting by live RSSI reordered rows
                    // under the user's finger mid-scan (tap lands on the
                    // wrong toy). RSSI is still shown as text per row.
                    val sorted = discoveredDevices.sortedWith(
                        compareByDescending<BleDeviceInfo> { it.isLovense }
                            .thenBy { it.name }
                            .thenBy { it.address }
                    )
                    sorted.forEach { device ->
                        val hint = LovenseProtocol.modelHint(device.name)
                        val title = when {
                            hint != null -> "Lovense $hint"
                            device.isLovense -> "Lovense"
                            else -> device.name
                        }
                        TextButton(
                            onClick = {
                                onConnectDevice(device.address)
                                showDeviceDialog = false
                            },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Column(modifier = Modifier.fillMaxWidth()) {
                                Text(
                                    title,
                                    color = if (device.isLovense) ChloeColors.Teal else ChloeColors.OnSurface,
                                    fontWeight = if (device.isLovense) FontWeight.SemiBold else FontWeight.Normal
                                )
                                Text(
                                    "${device.name} · ${device.rssi} dBm",
                                    color = ChloeColors.OnSurfaceDim,
                                    fontSize = 11.sp
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { showDeviceDialog = false }) {
                    Text("Cancel")
                }
            },
            containerColor = ChloeColors.Surface
        )
    }
}

// ---------------------------------------------------------------------------
// Spectrum Visualizer
// ---------------------------------------------------------------------------

@Composable
private fun SpectrumVisualizer(bandEnergies: FloatArray) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.Bottom,
        modifier = Modifier
            .fillMaxWidth()
            .height(48.dp)
            .clip(RoundedCornerShape(4.dp))
            .background(ChloeColors.Surface)
            .padding(horizontal = 8.dp, vertical = 4.dp)
    ) {
        for (i in 0 until NUM_BANDS.coerceAtMost(bandEnergies.size)) {
            val energy = bandEnergies[i].coerceIn(0f, 1f)
            val barColor = when {
                energy > 0.7f -> ChloeColors.Pink
                energy > 0.3f -> ChloeColors.Purple
                else -> ChloeColors.Teal
            }
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier.weight(1f)
            ) {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .height((36.dp.value * energy).dp.coerceAtLeast(2.dp))
                        .clip(RoundedCornerShape(topStart = 2.dp, topEnd = 2.dp))
                        .background(barColor)
                )
                Text(
                    BAND_NAMES.getOrElse(i) { "" },
                    fontSize = 7.sp,
                    color = ChloeColors.OnSurfaceDim,
                    textAlign = TextAlign.Center,
                    maxLines = 1
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Preset Selector
// ---------------------------------------------------------------------------

@Composable
private fun PresetSelector(
    selectedCategory: PresetCategory,
    selectedPresetName: String,
    onCategorySelected: (PresetCategory) -> Unit,
    onPresetSelected: (Preset) -> Unit
) {
    // Category tabs — taller chips for fat-finger selection
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(PresetCategory.all()) { category ->
            val isSelected = category == selectedCategory
            FilterChip(
                selected = isSelected,
                onClick = { onCategorySelected(category) },
                label = {
                    Text(
                        category.label,
                        fontSize = 13.sp,
                        fontWeight = if (isSelected) FontWeight.Bold else FontWeight.Medium,
                        letterSpacing = 1.sp,
                        modifier = Modifier.padding(vertical = 2.dp)
                    )
                },
                modifier = Modifier.heightIn(min = 44.dp),
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Purple,
                    selectedLabelColor = Color.White,
                    containerColor = ChloeColors.SurfaceVariant,
                    labelColor = ChloeColors.OnSurfaceDim
                )
            )
        }
    }

    Spacer(modifier = Modifier.height(10.dp))

    // Presets in category — large cards, strong selected border
    val presets = presetsInCategory(selectedCategory)
    LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        items(presets) { preset ->
            val isSelected = preset.name == selectedPresetName
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = if (isSelected) ChloeColors.PurpleDark else ChloeColors.SurfaceVariant
                ),
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier
                    .width(168.dp)
                    .heightIn(min = 88.dp)
                    .then(
                        if (isSelected) {
                            Modifier.border(2.dp, ChloeColors.PurpleLight, RoundedCornerShape(12.dp))
                        } else {
                            Modifier
                        }
                    )
                    .clickable { onPresetSelected(preset) }
            ) {
                Column(
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                    verticalArrangement = Arrangement.Center
                ) {
                    Text(
                        preset.name,
                        color = if (isSelected) Color.White else ChloeColors.OnSurface,
                        fontSize = 15.sp,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        preset.description,
                        color = if (isSelected) ChloeColors.PurpleLight else ChloeColors.OnSurfaceDim,
                        fontSize = 11.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis
                    )
                    if (isSelected) {
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            "ACTIVE",
                            color = ChloeColors.Teal,
                            fontSize = 10.sp,
                            fontWeight = FontWeight.Bold,
                            letterSpacing = 1.sp
                        )
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mode selectors
// ---------------------------------------------------------------------------

@Composable
private fun FrequencyModeSelector(current: FrequencyMode, onChange: (FrequencyMode) -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        modifier = Modifier.padding(vertical = 4.dp)
    ) {
        FrequencyMode.entries.forEach { mode ->
            val label = when (mode) {
                FrequencyMode.Full -> "Full"
                FrequencyMode.LowPass -> "LP"
                FrequencyMode.HighPass -> "HP"
                FrequencyMode.BandPass -> "BP"
            }
            FilterChip(
                selected = mode == current,
                onClick = { onChange(mode) },
                label = { Text(label, fontSize = 11.sp) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Teal,
                    selectedLabelColor = Color.Black
                )
            )
        }
    }
}

@Composable
private fun TriggerModeSelector(current: TriggerMode, onChange: (TriggerMode) -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        modifier = Modifier.padding(vertical = 4.dp)
    ) {
        TriggerMode.entries.forEach { mode ->
            FilterChip(
                selected = mode == current,
                onClick = { onChange(mode) },
                label = { Text(mode.name, fontSize = 11.sp) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Purple,
                    selectedLabelColor = Color.White
                )
            )
        }
    }
}

@Composable
private fun ClimaxPatternSelector(current: ClimaxPattern, onChange: (ClimaxPattern) -> Unit) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier
            .padding(vertical = 4.dp)
            .fillMaxWidth()
    ) {
        ClimaxPattern.entries.forEach { pattern ->
            val selected = pattern == current
            FilterChip(
                selected = selected,
                onClick = { onChange(pattern) },
                label = {
                    Text(
                        pattern.name,
                        fontSize = 13.sp,
                        fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                        modifier = Modifier.padding(horizontal = 2.dp, vertical = 2.dp)
                    )
                },
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp),
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Pink,
                    selectedLabelColor = Color.White,
                    containerColor = ChloeColors.SurfaceVariant,
                    labelColor = ChloeColors.OnSurfaceDim
                )
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Gate indicator
// ---------------------------------------------------------------------------

@Composable
private fun GateIndicator(open: Boolean) {
    val color by animateColorAsState(
        targetValue = if (open) ChloeColors.GateOpen else ChloeColors.GateClosed,
        animationSpec = tween(100),
        label = "gateColor"
    )
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(
            modifier = Modifier
                .size(8.dp)
                .clip(CircleShape)
                .background(color)
        )
        Spacer(modifier = Modifier.width(4.dp))
        Text(
            if (open) "OPEN" else "CLOSED",
            color = color,
            fontSize = 10.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = 1.sp
        )
    }
}

// ---------------------------------------------------------------------------
// Section header
// ---------------------------------------------------------------------------

@Composable
private fun SectionHeader(title: String, trailingContent: @Composable (() -> Unit)? = null) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 6.dp)
    ) {
        Text(
            title,
            color = ChloeColors.OnSurface,
            fontSize = 13.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = 2.sp
        )
        trailingContent?.invoke()
    }
}

// ---------------------------------------------------------------------------
// Labeled slider (with tap-to-edit and optional logarithmic scale)
// ---------------------------------------------------------------------------

@Composable
private fun LabeledSlider(
    label: String,
    value: Float,
    min: Float,
    max: Float,
    format: String,
    accentColor: Color = ChloeColors.OnSurface,
    logarithmic: Boolean = false,
    taper: Float = 1f,
    large: Boolean = false,
    onValueChange: (Float) -> Unit
) {
    var showDialog by remember { mutableStateOf(false) }

    // For logarithmic sliders, map actual value ↔ 0..1 slider position.
    // taper > 1 is a power taper (works with min = 0, unlike log): the low
    // end of the range gets most of the physical travel, so parameters whose
    // useful values sit far below max are not crammed into the first 15%.
    val sliderValue = if (logarithmic) {
        val minLog = ln(min)
        val maxLog = ln(max)
        ((ln(value.coerceIn(min, max)) - minLog) / (maxLog - minLog)).coerceIn(0f, 1f)
    } else if (taper != 1f) {
        val span = max - min
        if (span <= 0f) 0f
        else ((value.coerceIn(min, max) - min) / span).pow(1f / taper).coerceIn(0f, 1f)
    } else {
        value
    }
    val sliderRange = if (logarithmic || taper != 1f) 0f..1f else min..max
    val labelWidth = if (large) 88.dp else 80.dp
    val valueWidth = if (large) 64.dp else 56.dp

    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = if (large) 6.dp else 2.dp)
            .heightIn(min = if (large) 48.dp else 36.dp)
    ) {
        Text(
            label,
            color = accentColor,
            fontSize = if (large) 14.sp else 12.sp,
            fontWeight = if (large) FontWeight.SemiBold else FontWeight.Normal,
            modifier = Modifier.width(labelWidth)
        )
        Slider(
            value = sliderValue,
            onValueChange = { pos ->
                if (logarithmic) {
                    val minLog = ln(min)
                    val maxLog = ln(max)
                    onValueChange(exp(minLog + pos * (maxLog - minLog)))
                } else if (taper != 1f) {
                    onValueChange(min + (max - min) * pos.pow(taper))
                } else {
                    onValueChange(pos)
                }
            },
            valueRange = sliderRange,
            modifier = Modifier
                .weight(1f)
                .heightIn(min = if (large) 40.dp else 24.dp),
            colors = SliderDefaults.colors(
                thumbColor = accentColor,
                activeTrackColor = accentColor,
                inactiveTrackColor = ChloeColors.SurfaceVariant
            )
        )
        Text(
            format.format(value),
            color = ChloeColors.OnSurfaceDim,
            fontSize = if (large) 13.sp else 11.sp,
            fontWeight = if (large) FontWeight.SemiBold else FontWeight.Normal,
            modifier = Modifier
                .width(valueWidth)
                .clip(RoundedCornerShape(4.dp))
                .clickable { showDialog = true }
                .background(ChloeColors.SurfaceVariant.copy(alpha = 0.4f))
                .padding(horizontal = 4.dp, vertical = if (large) 6.dp else 2.dp),
            textAlign = TextAlign.End
        )
    }

    if (showDialog) {
        ManualEntryDialog(
            label = label,
            currentValue = value,
            min = min,
            max = max,
            format = format,
            onDismiss = { showDialog = false },
            onConfirm = { onValueChange(it); showDialog = false }
        )
    }
}

// ---------------------------------------------------------------------------
// Manual entry dialog
// ---------------------------------------------------------------------------

@Composable
private fun ManualEntryDialog(
    label: String,
    currentValue: Float,
    min: Float,
    max: Float,
    format: String,
    onDismiss: () -> Unit,
    onConfirm: (Float) -> Unit
) {
    // Strip unit suffix from format for the initial text value
    val numericStr = format.format(currentValue).replace(Regex("[^0-9.\\-]"), "").trim()
    var textValue by remember { mutableStateOf(numericStr) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(label, color = ChloeColors.OnSurface, fontWeight = FontWeight.Bold)
        },
        text = {
            Column {
                Text(
                    "Range: ${"%.4g".format(min)} – ${"%.4g".format(max)}",
                    color = ChloeColors.OnSurfaceDim,
                    fontSize = 11.sp
                )
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedTextField(
                    value = textValue,
                    onValueChange = { textValue = it },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = ChloeColors.Teal,
                        unfocusedBorderColor = ChloeColors.SurfaceVariant,
                        focusedTextColor = ChloeColors.OnSurface,
                        unfocusedTextColor = ChloeColors.OnSurface,
                        cursorColor = ChloeColors.Teal
                    ),
                    modifier = Modifier.fillMaxWidth()
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                textValue.toFloatOrNull()?.let { v ->
                    onConfirm(v.coerceIn(min, max))
                } ?: onDismiss()
            }) {
                Text("OK", color = ChloeColors.Teal)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel", color = ChloeColors.OnSurfaceDim)
            }
        },
        containerColor = ChloeColors.Surface
    )
}

// ---------------------------------------------------------------------------
// ADSR Envelope Scope
// ---------------------------------------------------------------------------

@Composable
private fun EnvelopeScopeView(
    attackMs: Float,
    decayMs: Float,
    sustainLevel: Float,
    releaseMs: Float,
    attackCurve: Float,
    decayCurve: Float,
    releaseCurve: Float
) {
    // Sustain gets a proportional display width so the scope looks balanced
    val sustainDisplayMs = (attackMs + decayMs + releaseMs).coerceAtLeast(1f) * 0.3f
    val totalMs = attackMs + decayMs + sustainDisplayMs + releaseMs
    if (totalMs <= 0f) return

    val attackFrac = attackMs / totalMs
    val decayFrac = decayMs / totalMs
    val sustainFrac = sustainDisplayMs / totalMs
    val samples = 48

    // Phase labels with times
    Row(
        modifier = Modifier.fillMaxWidth().padding(bottom = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        @Composable
        fun PhaseLabel(letter: String, timeMs: Float, color: Color, weight: Float) {
            Row(
                modifier = Modifier.weight(weight.coerceAtLeast(0.05f)),
                horizontalArrangement = Arrangement.Center
            ) {
                Text(letter, color = color, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                Spacer(modifier = Modifier.width(3.dp))
                Text(
                    if (letter == "S") "%.2f".format(sustainLevel)
                    else "%.0fms".format(timeMs),
                    color = color.copy(alpha = 0.6f),
                    fontSize = 9.sp
                )
            }
        }
        PhaseLabel("A", attackMs, ChloeColors.Attack, attackFrac)
        PhaseLabel("D", decayMs, ChloeColors.Decay, decayFrac)
        PhaseLabel("S", sustainDisplayMs, ChloeColors.Sustain, sustainFrac)
        PhaseLabel("R", releaseMs, ChloeColors.Release, 1f - attackFrac - decayFrac - sustainFrac)
    }

    Canvas(
        modifier = Modifier
            .fillMaxWidth()
            .height(72.dp)
            .clip(RoundedCornerShape(8.dp))
            .background(ChloeColors.Surface)
    ) {
        val w = size.width
        val h = size.height
        val pad = 4f

        val drawH = h - pad * 2
        val drawW = w - pad * 2
        fun xOf(frac: Float) = pad + frac * drawW
        fun yOf(level: Float) = pad + (1f - level) * drawH

        // Phase x boundaries
        val xA = xOf(attackFrac)
        val xD = xOf(attackFrac + decayFrac)
        val xS = xOf(attackFrac + decayFrac + sustainFrac)
        val xR = xOf(1f)

        // --- Build per-phase paths (fill + stroke) ---

        // Attack: 0 → 1.0
        val attackPath = Path().apply {
            moveTo(pad, yOf(0f))
            for (i in 1..samples) {
                val t = i.toFloat() / samples
                val level = applyCurve(t, attackCurve)
                lineTo(pad + t * (xA - pad), yOf(level))
            }
        }
        val attackFill = Path().apply {
            addPath(attackPath)
            lineTo(xA, yOf(0f))
            lineTo(pad, yOf(0f))
            close()
        }

        // Decay: 1.0 → sustainLevel
        val decayPath = Path().apply {
            moveTo(xA, yOf(1f))
            for (i in 1..samples) {
                val t = i.toFloat() / samples
                val decayFactor = applyCurve(1f - t, decayCurve)
                val level = sustainLevel + (1f - sustainLevel) * decayFactor
                lineTo(xA + t * (xD - xA), yOf(level))
            }
        }
        val decayFill = Path().apply {
            addPath(decayPath)
            lineTo(xD, yOf(0f))
            lineTo(xA, yOf(0f))
            close()
        }

        // Sustain: flat at sustainLevel
        val sustainPath = Path().apply {
            moveTo(xD, yOf(sustainLevel))
            lineTo(xS, yOf(sustainLevel))
        }
        val sustainFill = Path().apply {
            moveTo(xD, yOf(sustainLevel))
            lineTo(xS, yOf(sustainLevel))
            lineTo(xS, yOf(0f))
            lineTo(xD, yOf(0f))
            close()
        }

        // Release: sustainLevel → 0
        val releasePath = Path().apply {
            moveTo(xS, yOf(sustainLevel))
            for (i in 1..samples) {
                val t = i.toFloat() / samples
                val relFactor = applyCurve(1f - t, releaseCurve)
                val level = sustainLevel * relFactor
                lineTo(xS + t * (xR - xS), yOf(level))
            }
        }
        val releaseFill = Path().apply {
            addPath(releasePath)
            lineTo(xR, yOf(0f))
            lineTo(xS, yOf(0f))
            close()
        }

        // Draw fills
        drawPath(attackFill, ChloeColors.Attack.copy(alpha = 0.12f))
        drawPath(decayFill, ChloeColors.Decay.copy(alpha = 0.12f))
        drawPath(sustainFill, ChloeColors.Sustain.copy(alpha = 0.10f))
        drawPath(releaseFill, ChloeColors.Release.copy(alpha = 0.12f))

        // Draw strokes
        val strokeWidth = 2.dp.toPx()
        drawPath(attackPath, ChloeColors.Attack, style = DrawStroke(strokeWidth, cap = StrokeCap.Round))
        drawPath(decayPath, ChloeColors.Decay, style = DrawStroke(strokeWidth, cap = StrokeCap.Round))
        drawPath(sustainPath, ChloeColors.Sustain, style = DrawStroke(strokeWidth, cap = StrokeCap.Round))
        drawPath(releasePath, ChloeColors.Release, style = DrawStroke(strokeWidth, cap = StrokeCap.Round))

        // Phase boundary lines (dashed)
        val dashEffect = PathEffect.dashPathEffect(floatArrayOf(4f, 4f))
        val boundaryColor = ChloeColors.OnSurfaceDim.copy(alpha = 0.25f)
        for (bx in listOf(xA, xD, xS)) {
            drawLine(boundaryColor, Offset(bx, pad), Offset(bx, h - pad),
                strokeWidth = 1f, pathEffect = dashEffect)
        }

        // Baseline
        drawLine(ChloeColors.SurfaceVariant, Offset(pad, yOf(0f)), Offset(xR, yOf(0f)), strokeWidth = 1f)
    }
}
