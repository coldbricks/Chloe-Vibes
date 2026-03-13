// ==========================================================================
// MainScreen.kt -- Main Compose screen
//
// Provides: preset selector, ADSR sliders, output visualization,
// device connection status, climax engine controls.
// Signal chain: Audio In -> Freq Filter -> Gate -> Trigger -> ADSR -> Output -> Device
// ==========================================================================

package com.ashairfoil.chloevibes.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.*
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke as DrawStroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.text.KeyboardOptions
import com.ashairfoil.chloevibes.audio.*
import com.ashairfoil.chloevibes.device.ConnectionState
import kotlin.math.exp
import kotlin.math.ln

// ---------------------------------------------------------------------------
// ViewModel state
// ---------------------------------------------------------------------------

class MainScreenState {
    // Preset
    var selectedPresetName by mutableStateOf("Ride Intensity")
    var selectedCategory by mutableStateOf(PresetCategory.Init)

    // Input
    var mainVolume by mutableFloatStateOf(1.15f)
    var frequencyMode by mutableStateOf(FrequencyMode.Full)
    var targetFrequency by mutableFloatStateOf(200f)

    // Gate
    var gateThreshold by mutableFloatStateOf(0.07f)
    var autoGateAmount by mutableFloatStateOf(0f)
    var gateSmoothing by mutableFloatStateOf(0.22f)

    // Trigger
    var triggerMode by mutableStateOf(TriggerMode.Dynamic)
    var binaryLevel by mutableFloatStateOf(0.8f)
    var hybridBlend by mutableFloatStateOf(0.5f)

    // ADSR
    var attackMs by mutableFloatStateOf(30f)
    var decayMs by mutableFloatStateOf(160f)
    var sustainLevel by mutableFloatStateOf(0.9f)
    var releaseMs by mutableFloatStateOf(320f)
    var attackCurve by mutableFloatStateOf(1f)
    var decayCurve by mutableFloatStateOf(1f)
    var releaseCurve by mutableFloatStateOf(1.15f)

    // Output
    var minVibe by mutableFloatStateOf(0f)
    var maxVibe by mutableFloatStateOf(1f)

    // Climax
    var climaxEnabled by mutableStateOf(false)
    var climaxIntensity by mutableFloatStateOf(0.7f)
    var climaxBuildUpMs by mutableFloatStateOf(90_000f)
    var climaxTeaseRatio by mutableFloatStateOf(0.18f)
    var climaxTeaseDrop by mutableFloatStateOf(0.35f)
    var climaxSurgeBoost by mutableFloatStateOf(0.5f)
    var climaxPulseDepth by mutableFloatStateOf(0.18f)
    var climaxPattern by mutableStateOf(ClimaxPattern.Wave)

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

    fun applyPreset(preset: Preset) {
        selectedPresetName = preset.name
        mainVolume = preset.mainVolume
        frequencyMode = preset.frequencyMode
        targetFrequency = preset.targetFrequency
        gateThreshold = preset.gateThreshold
        autoGateAmount = preset.autoGateAmount
        gateSmoothing = preset.gateSmoothing
        triggerMode = preset.triggerMode
        binaryLevel = preset.binaryLevel
        hybridBlend = preset.hybridBlend
        attackMs = preset.attackMs
        decayMs = preset.decayMs
        sustainLevel = preset.sustainLevel
        releaseMs = preset.releaseMs
        attackCurve = preset.attackCurve
        decayCurve = preset.decayCurve
        releaseCurve = preset.releaseCurve
        minVibe = preset.minVibe
        maxVibe = preset.maxVibe
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
    onScanDevices: () -> Unit,
    onConnectDevice: (String) -> Unit,
    onDisconnectDevice: () -> Unit,
    discoveredDevices: List<Pair<String, String>>, // (name, address)
    onParameterChanged: () -> Unit
) {
    val scrollState = rememberScrollState()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(ChloeColors.Background)
            .verticalScroll(scrollState)
            .padding(16.dp)
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

        Spacer(modifier = Modifier.height(16.dp))

        // Output meter + device status
        OutputMeter(
            output = state.currentOutput,
            gateOpen = state.gateOpen,
            envelopeState = state.envelopeState,
            connectionState = state.connectionState,
            deviceName = state.connectedDeviceName,
            batteryLevel = state.batteryLevel,
            isCapturing = state.isCapturing
        )

        Spacer(modifier = Modifier.height(16.dp))

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

        Spacer(modifier = Modifier.height(20.dp))

        // Spectrum visualizer
        SpectrumVisualizer(bandEnergies = state.bandEnergies)

        Spacer(modifier = Modifier.height(20.dp))

        // Preset selector
        SectionHeader("PRESETS")
        PresetSelector(
            selectedCategory = state.selectedCategory,
            selectedPresetName = state.selectedPresetName,
            onCategorySelected = { state.selectedCategory = it },
            onPresetSelected = onPresetSelected
        )

        Spacer(modifier = Modifier.height(20.dp))

        // INPUT section
        SectionHeader("INPUT")
        LabeledSlider("Volume", state.mainVolume, 0f, 3f, "%.2f") {
            state.mainVolume = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        FrequencyModeSelector(state.frequencyMode) {
            state.frequencyMode = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        if (state.frequencyMode != FrequencyMode.Full) {
            LabeledSlider("Target Freq", state.targetFrequency, 20f, 16000f, "%.0f Hz",
                logarithmic = true) {
                state.targetFrequency = it; state.selectedPresetName = "Custom"; onParameterChanged()
            }
        }

        Spacer(modifier = Modifier.height(16.dp))

        // GATE section
        SectionHeader("GATE", trailingContent = {
            GateIndicator(state.gateOpen)
        })
        LabeledSlider("Threshold", state.gateThreshold, 0f, 0.5f, "%.2f") {
            state.gateThreshold = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Auto-Sense", state.autoGateAmount, 0f, 1f, "%.2f") {
            state.autoGateAmount = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Smooth", state.gateSmoothing, 0f, 1f, "%.2f") {
            state.gateSmoothing = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }

        Spacer(modifier = Modifier.height(16.dp))

        // TRIGGER section
        SectionHeader("TRIGGER")
        TriggerModeSelector(state.triggerMode) {
            state.triggerMode = it; state.selectedPresetName = "Custom"; onParameterChanged()
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

        Spacer(modifier = Modifier.height(16.dp))

        // ENVELOPE section (color-coded ADSR)
        SectionHeader("ENVELOPE")
        LabeledSlider("Attack", state.attackMs, 0.5f, 500f, "%.0f ms", ChloeColors.Attack) {
            state.attackMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Decay", state.decayMs, 1f, 500f, "%.0f ms", ChloeColors.Decay) {
            state.decayMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Sustain", state.sustainLevel, 0f, 1f, "%.2f", ChloeColors.Sustain) {
            state.sustainLevel = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Release", state.releaseMs, 1f, 2000f, "%.0f ms", ChloeColors.Release) {
            state.releaseMs = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }

        Spacer(modifier = Modifier.height(10.dp))

        // ADSR waveform scope
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

        Spacer(modifier = Modifier.height(16.dp))

        // OUTPUT section
        SectionHeader("OUTPUT")
        LabeledSlider("Floor", state.minVibe, 0f, 1f, "%.2f") {
            state.minVibe = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }
        LabeledSlider("Ceiling", state.maxVibe, 0f, 1f, "%.2f") {
            state.maxVibe = it; state.selectedPresetName = "Custom"; onParameterChanged()
        }

        Spacer(modifier = Modifier.height(20.dp))

        // CLIMAX section
        SectionHeader("CLIMAX")
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(
                checked = state.climaxEnabled,
                onCheckedChange = { state.climaxEnabled = it; onParameterChanged() },
                colors = SwitchDefaults.colors(
                    checkedTrackColor = ChloeColors.Pink,
                    checkedThumbColor = Color.White
                )
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                if (state.climaxEnabled) "ACTIVE" else "OFF",
                color = if (state.climaxEnabled) ChloeColors.Pink else ChloeColors.OnSurfaceDim,
                fontSize = 13.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = 1.sp
            )
        }

        if (state.climaxEnabled) {
            Spacer(modifier = Modifier.height(8.dp))
            ClimaxPatternSelector(state.climaxPattern) {
                state.climaxPattern = it; onParameterChanged()
            }
            LabeledSlider("Intensity", state.climaxIntensity, 0f, 1f, "%.2f", ChloeColors.Pink) {
                state.climaxIntensity = it; onParameterChanged()
            }
            LabeledSlider("Build Cycle", state.climaxBuildUpMs, 8000f, 240000f, "%.0f s") {
                state.climaxBuildUpMs = it; onParameterChanged()
            }
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

        Spacer(modifier = Modifier.height(32.dp))
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
    isCapturing: Boolean
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

                // Capture status
                StatusChip(
                    if (isCapturing) "LIVE" else "STOPPED",
                    if (isCapturing) ChloeColors.Teal else ChloeColors.OnSurfaceDim
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
private fun StatusChip(label: String, color: Color) {
    Text(
        text = label,
        color = color,
        fontSize = 10.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 1.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis
    )
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
    discoveredDevices: List<Pair<String, String>>,
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
                containerColor = if (isCapturing) ChloeColors.SurfaceVariant else ChloeColors.Teal
            ),
            modifier = Modifier.weight(1f)
        ) {
            Icon(
                if (isCapturing) Icons.Default.Stop else Icons.Default.PlayArrow,
                contentDescription = null,
                modifier = Modifier.size(18.dp)
            )
            Spacer(modifier = Modifier.width(4.dp))
            Text(if (isCapturing) "Stop" else "Start", fontSize = 13.sp)
        }

        // Scan / Connect
        if (connectionState == ConnectionState.Ready || connectionState == ConnectionState.Connected) {
            Button(
                onClick = onDisconnectDevice,
                colors = ButtonDefaults.buttonColors(containerColor = ChloeColors.SurfaceVariant),
                modifier = Modifier.weight(1f)
            ) {
                Icon(Icons.Default.BluetoothDisabled, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(4.dp))
                Text("Disconnect", fontSize = 13.sp)
            }
        } else {
            Button(
                onClick = {
                    onScanDevices()
                    showDeviceDialog = true
                },
                colors = ButtonDefaults.buttonColors(containerColor = ChloeColors.Purple),
                modifier = Modifier.weight(1f)
            ) {
                Icon(Icons.Default.BluetoothSearching, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(4.dp))
                Text("Scan", fontSize = 13.sp)
            }
        }
    }

    // Device picker dialog
    if (showDeviceDialog) {
        AlertDialog(
            onDismissRequest = { showDeviceDialog = false },
            title = { Text("Select Device") },
            text = {
                Column {
                    if (discoveredDevices.isEmpty()) {
                        Text("Scanning for devices...", color = ChloeColors.OnSurfaceDim)
                    }
                    discoveredDevices.forEach { (name, address) ->
                        TextButton(
                            onClick = {
                                onConnectDevice(address)
                                showDeviceDialog = false
                            },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text(name, color = ChloeColors.OnSurface)
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
    // Category tabs
    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(PresetCategory.all()) { category ->
            val isSelected = category == selectedCategory
            FilterChip(
                selected = isSelected,
                onClick = { onCategorySelected(category) },
                label = { Text(category.label, fontSize = 11.sp, letterSpacing = 1.sp) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Purple,
                    selectedLabelColor = Color.White,
                    containerColor = ChloeColors.SurfaceVariant,
                    labelColor = ChloeColors.OnSurfaceDim
                )
            )
        }
    }

    Spacer(modifier = Modifier.height(8.dp))

    // Presets in category
    val presets = presetsInCategory(selectedCategory)
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(presets) { preset ->
            val isSelected = preset.name == selectedPresetName
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = if (isSelected) ChloeColors.PurpleDark else ChloeColors.SurfaceVariant
                ),
                shape = RoundedCornerShape(8.dp),
                modifier = Modifier
                    .width(140.dp)
                    .clickable { onPresetSelected(preset) }
            ) {
                Column(modifier = Modifier.padding(10.dp)) {
                    Text(
                        preset.name,
                        color = if (isSelected) Color.White else ChloeColors.OnSurface,
                        fontSize = 13.sp,
                        fontWeight = FontWeight.Bold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                    Spacer(modifier = Modifier.height(2.dp))
                    Text(
                        preset.description,
                        color = if (isSelected) ChloeColors.PurpleLight else ChloeColors.OnSurfaceDim,
                        fontSize = 10.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis
                    )
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
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        modifier = Modifier.padding(vertical = 4.dp)
    ) {
        ClimaxPattern.entries.forEach { pattern ->
            FilterChip(
                selected = pattern == current,
                onClick = { onChange(pattern) },
                label = { Text(pattern.name, fontSize = 11.sp) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = ChloeColors.Pink,
                    selectedLabelColor = Color.White
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
    onValueChange: (Float) -> Unit
) {
    var showDialog by remember { mutableStateOf(false) }

    // For logarithmic sliders, map actual value ↔ 0..1 slider position
    val sliderValue = if (logarithmic) {
        val minLog = ln(min)
        val maxLog = ln(max)
        ((ln(value.coerceIn(min, max)) - minLog) / (maxLog - minLog)).coerceIn(0f, 1f)
    } else {
        value
    }
    val sliderRange = if (logarithmic) 0f..1f else min..max

    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 2.dp)
    ) {
        Text(
            label,
            color = accentColor,
            fontSize = 12.sp,
            modifier = Modifier.width(80.dp)
        )
        Slider(
            value = sliderValue,
            onValueChange = { pos ->
                if (logarithmic) {
                    val minLog = ln(min)
                    val maxLog = ln(max)
                    onValueChange(exp(minLog + pos * (maxLog - minLog)))
                } else {
                    onValueChange(pos)
                }
            },
            valueRange = sliderRange,
            modifier = Modifier.weight(1f),
            colors = SliderDefaults.colors(
                thumbColor = accentColor,
                activeTrackColor = accentColor,
                inactiveTrackColor = ChloeColors.SurfaceVariant
            )
        )
        Text(
            format.format(value),
            color = ChloeColors.OnSurfaceDim,
            fontSize = 11.sp,
            modifier = Modifier
                .width(56.dp)
                .clip(RoundedCornerShape(4.dp))
                .clickable { showDialog = true }
                .background(ChloeColors.SurfaceVariant.copy(alpha = 0.4f))
                .padding(horizontal = 4.dp, vertical = 2.dp),
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
