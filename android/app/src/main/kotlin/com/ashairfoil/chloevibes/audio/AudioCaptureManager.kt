// ==========================================================================
// AudioCaptureManager.kt -- Android audio capture
//
// Uses Android's Visualizer API to capture system audio FFT, with
// AudioRecord (microphone) as fallback. The Visualizer API attaches to
// the system audio output session and provides frequency data directly.
//
// The processing loop runs on a dedicated thread at ~60Hz, feeding
// spectral data through the signal chain (analyzer -> gate -> envelope
// -> climax engine -> device output).
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.audiofx.Visualizer
import android.util.Log
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicReference

// ---------------------------------------------------------------------------
// Audio source mode
// ---------------------------------------------------------------------------

enum class AudioSourceMode {
    /** System audio via Visualizer API (recommended). */
    SystemAudio,
    /** Microphone input via AudioRecord. */
    Microphone
}

// ---------------------------------------------------------------------------
// Processing state -- holds all live signal chain state
// ---------------------------------------------------------------------------

/**
 * Mutable state for the entire signal processing chain.
 * Updated on the processing thread, read by the UI thread.
 */
class ProcessingState {
    val analyzer = SpectralAnalyzer(48000f)
    val gate = Gate()
    val envelope = EnvelopeProcessor()
    val beatDetector = BeatDetector()
    val climaxEngine = ClimaxEngine()

    @Volatile var lastSpectralData = SpectralData()
    @Volatile var lastEnergy: Float = 0f
    @Volatile var lastGateOpen: Boolean = false
    @Volatile var lastEnvelopeOutput: Float = 0f
    @Volatile var lastFinalOutput: Float = 0f
    @Volatile var lastEnvelopeState: EnvelopeState = EnvelopeState.Idle
}

// ---------------------------------------------------------------------------
// ProcessingParams -- immutable snapshot of all signal processing parameters
// ---------------------------------------------------------------------------

/**
 * Immutable snapshot of all processing parameters. Swapped atomically via
 * AtomicReference so the processing loop reads a consistent set of values
 * each frame, preventing partial-read tearing during preset changes.
 */
data class ProcessingParams(
    val mainVolume: Float = 1.15f,
    val frequencyMode: FrequencyMode = FrequencyMode.Full,
    val targetFrequency: Float = 200f,
    val gateThreshold: Float = 0.07f,
    val autoGateAmount: Float = 0f,
    val gateSmoothing: Float = 0.22f,
    val thresholdKnee: Float = 0.22f,
    val triggerMode: TriggerMode = TriggerMode.Dynamic,
    val binaryLevel: Float = 0.8f,
    val hybridBlend: Float = 0.5f,
    val dynamicCurve: Float = 1f,
    val attackMs: Float = 30f,
    val decayMs: Float = 160f,
    val sustainLevel: Float = 0.9f,
    val releaseMs: Float = 320f,
    val attackCurve: Float = 1f,
    val decayCurve: Float = 1f,
    val releaseCurve: Float = 1.15f,
    val minVibe: Float = 0f,
    val maxVibe: Float = 1f,
    val outputGain: Float = 1f,
    val climaxEnabled: Boolean = false,
    val climaxIntensity: Float = 0.7f,
    val climaxBuildUpMs: Float = 90_000f,
    val climaxTeaseRatio: Float = 0.18f,
    val climaxTeaseDrop: Float = 0.35f,
    val climaxSurgeBoost: Float = 0.5f,
    val climaxPulseDepth: Float = 0.18f,
    val climaxPattern: ClimaxPattern = ClimaxPattern.Wave
)

// ---------------------------------------------------------------------------
// AudioCaptureManager
// ---------------------------------------------------------------------------

/**
 * Manages audio capture from either system audio (Visualizer) or microphone
 * (AudioRecord), and runs the signal processing loop.
 */
class AudioCaptureManager(private val context: Context) {

    // Processing state (thread-safe via volatile fields)
    val state = ProcessingState()

    // Audio source
    private var sourceMode: AudioSourceMode = AudioSourceMode.SystemAudio
    private var visualizer: Visualizer? = null
    private var audioRecord: AudioRecord? = null

    // Processing thread
    private var processingThread: Thread? = null
    @Volatile private var running = false

    // Latest captured samples (written by capture, read by processing)
    private val sampleLock = Object()
    private var capturedSamples = FloatArray(0)
    private var hasFreshSamples = false
    @Volatile private var lastSampleTimeMs = 0L

    // Visualizer FFT magnitude data (linear, 0.0-1.0 per bin, Rust-parity)
    private var capturedMagnitudes = FloatArray(0)
    private var hasFreshMagnitudes = false
    @Volatile private var useVisualizerFft = false
    @Volatile private var visualizerSampleRate = 48000

    // Silence / stall detection for Visualizer fallback
    @Volatile private var silentFrameCount = 0
    private companion object {
        const val SILENT_FRAMES_BEFORE_FALLBACK = 90
        const val STALL_TIMEOUT_MS = 3000L
        const val TARGET_FRAME_MS = 16L
    }

    // Signal processing parameters -- bundled into an immutable data class
    // and swapped atomically so the processing loop reads a consistent snapshot.
    private val paramsRef = AtomicReference(ProcessingParams())

    // Public accessors for UI thread to read/write individual parameters.
    // Each setter creates a new ProcessingParams snapshot via atomic swap.
    var mainVolume: Float
        get() = paramsRef.get().mainVolume
        set(v) { paramsRef.updateAndGet { it.copy(mainVolume = v) } }
    var frequencyMode: FrequencyMode
        get() = paramsRef.get().frequencyMode
        set(v) { paramsRef.updateAndGet { it.copy(frequencyMode = v) } }
    var targetFrequency: Float
        get() = paramsRef.get().targetFrequency
        set(v) { paramsRef.updateAndGet { it.copy(targetFrequency = v) } }
    var gateThreshold: Float
        get() = paramsRef.get().gateThreshold
        set(v) { paramsRef.updateAndGet { it.copy(gateThreshold = v) } }
    var autoGateAmount: Float
        get() = paramsRef.get().autoGateAmount
        set(v) { paramsRef.updateAndGet { it.copy(autoGateAmount = v) } }
    var gateSmoothing: Float
        get() = paramsRef.get().gateSmoothing
        set(v) { paramsRef.updateAndGet { it.copy(gateSmoothing = v) } }
    var thresholdKnee: Float
        get() = paramsRef.get().thresholdKnee
        set(v) { paramsRef.updateAndGet { it.copy(thresholdKnee = v) } }
    var triggerMode: TriggerMode
        get() = paramsRef.get().triggerMode
        set(v) { paramsRef.updateAndGet { it.copy(triggerMode = v) } }
    var binaryLevel: Float
        get() = paramsRef.get().binaryLevel
        set(v) { paramsRef.updateAndGet { it.copy(binaryLevel = v) } }
    var hybridBlend: Float
        get() = paramsRef.get().hybridBlend
        set(v) { paramsRef.updateAndGet { it.copy(hybridBlend = v) } }
    var dynamicCurve: Float
        get() = paramsRef.get().dynamicCurve
        set(v) { paramsRef.updateAndGet { it.copy(dynamicCurve = v) } }
    var attackMs: Float
        get() = paramsRef.get().attackMs
        set(v) { paramsRef.updateAndGet { it.copy(attackMs = v) } }
    var decayMs: Float
        get() = paramsRef.get().decayMs
        set(v) { paramsRef.updateAndGet { it.copy(decayMs = v) } }
    var sustainLevel: Float
        get() = paramsRef.get().sustainLevel
        set(v) { paramsRef.updateAndGet { it.copy(sustainLevel = v) } }
    var releaseMs: Float
        get() = paramsRef.get().releaseMs
        set(v) { paramsRef.updateAndGet { it.copy(releaseMs = v) } }
    var attackCurve: Float
        get() = paramsRef.get().attackCurve
        set(v) { paramsRef.updateAndGet { it.copy(attackCurve = v) } }
    var decayCurve: Float
        get() = paramsRef.get().decayCurve
        set(v) { paramsRef.updateAndGet { it.copy(decayCurve = v) } }
    var releaseCurve: Float
        get() = paramsRef.get().releaseCurve
        set(v) { paramsRef.updateAndGet { it.copy(releaseCurve = v) } }
    var minVibe: Float
        get() = paramsRef.get().minVibe
        set(v) { paramsRef.updateAndGet { it.copy(minVibe = v) } }
    var maxVibe: Float
        get() = paramsRef.get().maxVibe
        set(v) { paramsRef.updateAndGet { it.copy(maxVibe = v) } }
    var outputGain: Float
        get() = paramsRef.get().outputGain
        set(v) { paramsRef.updateAndGet { it.copy(outputGain = v) } }
    var climaxEnabled: Boolean
        get() = paramsRef.get().climaxEnabled
        set(v) { paramsRef.updateAndGet { it.copy(climaxEnabled = v) } }
    var climaxIntensity: Float
        get() = paramsRef.get().climaxIntensity
        set(v) { paramsRef.updateAndGet { it.copy(climaxIntensity = v) } }
    var climaxBuildUpMs: Float
        get() = paramsRef.get().climaxBuildUpMs
        set(v) { paramsRef.updateAndGet { it.copy(climaxBuildUpMs = v) } }
    var climaxTeaseRatio: Float
        get() = paramsRef.get().climaxTeaseRatio
        set(v) { paramsRef.updateAndGet { it.copy(climaxTeaseRatio = v) } }
    var climaxTeaseDrop: Float
        get() = paramsRef.get().climaxTeaseDrop
        set(v) { paramsRef.updateAndGet { it.copy(climaxTeaseDrop = v) } }
    var climaxSurgeBoost: Float
        get() = paramsRef.get().climaxSurgeBoost
        set(v) { paramsRef.updateAndGet { it.copy(climaxSurgeBoost = v) } }
    var climaxPulseDepth: Float
        get() = paramsRef.get().climaxPulseDepth
        set(v) { paramsRef.updateAndGet { it.copy(climaxPulseDepth = v) } }
    var climaxPattern: ClimaxPattern
        get() = paramsRef.get().climaxPattern
        set(v) { paramsRef.updateAndGet { it.copy(climaxPattern = v) } }

    // Output callback -- single motor (legacy) and dual motor
    var onOutputUpdate: ((Float) -> Unit)? = null
    /** Dual-motor callback: (motor1, motor2) for devices with independent motors. */
    var onDualOutputUpdate: ((Float, Float) -> Unit)? = null
    @Volatile private var lastSentOutput: Float = 0f

    /** Called when Visualizer produces silence and we auto-fallback to mic. */
    var onFallbackToMic: (() -> Unit)? = null

    /** Apply a preset to all signal processing parameters atomically. */
    fun applyPreset(preset: Preset) {
        paramsRef.set(ProcessingParams(
            mainVolume = preset.mainVolume,
            frequencyMode = preset.frequencyMode,
            targetFrequency = preset.targetFrequency,
            gateThreshold = preset.gateThreshold,
            autoGateAmount = preset.autoGateAmount,
            gateSmoothing = preset.gateSmoothing,
            thresholdKnee = preset.thresholdKnee,
            triggerMode = preset.triggerMode,
            binaryLevel = preset.binaryLevel,
            hybridBlend = preset.hybridBlend,
            dynamicCurve = preset.dynamicCurve,
            attackMs = preset.attackMs,
            decayMs = preset.decayMs,
            sustainLevel = preset.sustainLevel,
            releaseMs = preset.releaseMs,
            attackCurve = preset.attackCurve,
            decayCurve = preset.decayCurve,
            releaseCurve = preset.releaseCurve,
            minVibe = preset.minVibe,
            maxVibe = preset.maxVibe,
            climaxEnabled = preset.climaxEnabled,
            climaxIntensity = preset.climaxIntensity,
            climaxBuildUpMs = preset.climaxBuildUpMs,
            climaxTeaseRatio = preset.climaxTeaseRatio,
            climaxTeaseDrop = preset.climaxTeaseDrop,
            climaxSurgeBoost = preset.climaxSurgeBoost,
            climaxPulseDepth = preset.climaxPulseDepth,
            climaxPattern = preset.climaxPattern
        ))
    }

    /**
     * Start audio capture and processing.
     *
     * @param mode which audio source to use
     * @return true if started successfully
     */
    fun start(mode: AudioSourceMode = AudioSourceMode.SystemAudio): Boolean {
        if (running) return true
        sourceMode = mode

        Log.i("ChloeVibes", "Starting audio capture in mode: $mode")
        val started = when (mode) {
            AudioSourceMode.SystemAudio -> startVisualizer()
            AudioSourceMode.Microphone -> startMicrophone()
        }

        if (started) {
            running = true
            processingThread = Thread(::processingLoop, "ChloeVibes-Processing").apply {
                priority = Thread.MAX_PRIORITY
                isDaemon = true
                start()
            }
            Log.i("ChloeVibes", "Audio capture started successfully")
        } else {
            Log.w("ChloeVibes", "Audio capture failed to start in mode: $mode")
        }
        return started
    }

    /** Stop audio capture and processing. */
    fun stop() {
        Log.i("ChloeVibes", "Stopping audio capture")
        running = false
        processingThread?.join(500)
        processingThread = null

        visualizer?.apply {
            enabled = false
            release()
        }
        visualizer = null

        audioRecord?.apply {
            stop()
            release()
        }
        audioRecord = null
    }

    val isRunning: Boolean get() = running

    // -----------------------------------------------------------------------
    // Visualizer API (system audio)
    // -----------------------------------------------------------------------

    private fun startVisualizer(): Boolean {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) return false

        return try {
            val viz = Visualizer(0) // session 0 = system audio output mix
            val maxCapture = Visualizer.getCaptureSizeRange()[1]
            viz.captureSize = maxCapture.coerceAtMost(FFT_SIZE)
            viz.setDataCaptureListener(
                object : Visualizer.OnDataCaptureListener {
                    override fun onWaveFormDataCapture(
                        visualizer: Visualizer,
                        waveform: ByteArray,
                        samplingRate: Int
                    ) {
                        // Not used — we use FFT data instead for reliable amplitude
                    }

                    override fun onFftDataCapture(
                        visualizer: Visualizer,
                        fft: ByteArray,
                        samplingRate: Int
                    ) {
                        // Android Visualizer FFT format: [DC_real, DC_imag,
                        // bin1_real, bin1_imag, ...]. Convert to magnitudes
                        // normalized to 0.0-1.0, matching the Web Audio
                        // AnalyserNode's getByteFrequencyData() output that
                        // the original HTML ChloeVibes used.
                        val numBins = fft.size / 2
                        val mags = FloatArray(numBins)
                        // Linear magnitudes (no dB conversion). Matches Rust
                        // SpectralAnalyzer's linear FFT output so band energies
                        // have the same tonal balance on both platforms.
                        // Raw Visualizer bytes are in ~[-128,127], so sqrt(re^2+im^2)
                        // peaks around 181; normalize by captureSize/2 (matches
                        // Rust's 2.0/FFT_SIZE convention) and clamp.
                        val linearScale = 2f / numBins.toFloat()
                        for (i in 0 until numBins) {
                            val re = fft[2 * i].toFloat()
                            val im = fft[2 * i + 1].toFloat()
                            val mag = kotlin.math.sqrt(re * re + im * im) * linearScale
                            mags[i] = mag.coerceIn(0f, 1f)
                        }
                        synchronized(sampleLock) {
                            capturedMagnitudes = mags
                            hasFreshMagnitudes = true
                        }
                        visualizerSampleRate = samplingRate / 1000 // API gives milliHz
                        lastSampleTimeMs = System.currentTimeMillis()
                    }
                },
                Visualizer.getMaxCaptureRate(),
                false, // waveform — not needed
                true   // fft — use this instead
            )
            viz.enabled = true
            visualizer = viz
            useVisualizerFft = true
            true
        } catch (e: Exception) {
            Log.e("ChloeVibes", "Visualizer initialization failed", e)
            false
        }
    }

    // -----------------------------------------------------------------------
    // AudioRecord (microphone fallback)
    // -----------------------------------------------------------------------

    private fun startMicrophone(): Boolean {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) return false

        return try {
            val sampleRate = 48000
            val bufferSize = AudioRecord.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_FLOAT
            ).coerceAtLeast(FFT_SIZE * 4) // ensure at least FFT_SIZE float samples

            @Suppress("MissingPermission")
            val record = AudioRecord(
                MediaRecorder.AudioSource.MIC,
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_FLOAT,
                bufferSize
            )

            if (record.state != AudioRecord.STATE_INITIALIZED) {
                record.release()
                return false
            }

            record.startRecording()
            audioRecord = record

            // Mic capture runs on its own thread feeding samples
            Thread({
                val buffer = FloatArray(FFT_SIZE)
                while (running) {
                    val read = record.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
                    if (read > 0) {
                        val samples = buffer.copyOf(read)
                        synchronized(sampleLock) {
                            capturedSamples = samples
                            hasFreshSamples = true
                        }
                    }
                }
            }, "ChloeVibes-MicCapture").apply {
                isDaemon = true
                start()
            }

            true
        } catch (e: Exception) {
            Log.w("ChloeVibes", "Microphone capture initialization failed", e)
            false
        }
    }

    // Visualizer restart failure counter (Fix 13)
    private var visualizerRestartFailures = 0

    // -----------------------------------------------------------------------
    // Processing loop (~60Hz)
    // -----------------------------------------------------------------------

    private fun processingLoop() {
        val startMs = System.nanoTime() / 1_000_000f
        lastSampleTimeMs = System.currentTimeMillis()
        var consecutiveErrors = 0

        while (running) {
          try {
            val frameStartNs = System.nanoTime()
            val currentTimeMs = System.nanoTime() / 1_000_000f - startMs

            // Read all parameters once per frame for a consistent snapshot
            val params = paramsRef.get()

            val spectralData: SpectralData
            val energy: Float

            if (useVisualizerFft) {
                // ---- Visualizer FFT path ----
                // Use pre-computed magnitude data (dB-normalized to 0-1),
                // matching the Web Audio AnalyserNode used by the HTML version.
                val mags: FloatArray
                synchronized(sampleLock) {
                    if (!hasFreshMagnitudes) {
                        mags = FloatArray(0)
                    } else {
                        mags = capturedMagnitudes.copyOf()
                        hasFreshMagnitudes = false
                    }
                }

                // Stall detection for Visualizer
                if (sourceMode == AudioSourceMode.SystemAudio) {
                    val stalled = visualizer != null &&
                        (System.currentTimeMillis() - lastSampleTimeMs) > STALL_TIMEOUT_MS
                    val isSilent = mags.isEmpty() || mags.all { it < 0.001f }
                    if (isSilent) silentFrameCount++ else silentFrameCount = 0
                    if (stalled || silentFrameCount >= SILENT_FRAMES_BEFORE_FALLBACK) {
                        silentFrameCount = 0
                        try { visualizer?.apply { enabled = false; release() } } catch (_: Exception) {}
                        visualizer = null
                        useVisualizerFft = false
                        val restarted = startVisualizer()
                        if (restarted) {
                            visualizerRestartFailures = 0
                        } else {
                            visualizerRestartFailures++
                            if (visualizerRestartFailures >= 3) {
                                Log.w("ChloeVibes", "Visualizer restart failed 3 times, falling back to mic mode")
                                visualizerRestartFailures = 0
                                startMicrophone()
                            }
                        }
                        lastSampleTimeMs = System.currentTimeMillis()
                    }
                }

                if (mags.isEmpty()) {
                    spectralData = SpectralData()
                    energy = 0f
                } else {
                    // Build SpectralData from Visualizer magnitudes.
                    // Bin resolution depends on capture size and sample rate.
                    val sr = visualizerSampleRate.toFloat()
                    val captureSize = mags.size * 2
                    val binRes = sr / captureSize

                    // Calculate band energies from magnitude bins
                    val bandEnergies = FloatArray(NUM_BANDS)
                    for (b in 0 until NUM_BANDS) {
                        val loHz = BAND_EDGES[b]
                        val hiHz = BAND_EDGES[b + 1]
                        val loBin = (loHz / binRes).toInt().coerceIn(0, mags.size - 1)
                        val hiBin = (hiHz / binRes).toInt().coerceIn(loBin + 1, mags.size)
                        var sum = 0f
                        for (i in loBin until hiBin) sum += mags[i] * mags[i]
                        val count = (hiBin - loBin).coerceAtLeast(1)
                        bandEnergies[b] = kotlin.math.sqrt(sum / count)
                    }

                    // RMS of magnitudes as overall power proxy
                    var rmsSum = 0f
                    for (m in mags) rmsSum += m * m
                    val rmsPower = kotlin.math.sqrt(rmsSum / mags.size)

                    // Spectral centroid
                    var wSum = 0f; var tMag = 0f
                    for (i in mags.indices) {
                        val freq = i * binRes
                        wSum += freq * mags[i]; tMag += mags[i]
                    }
                    val centroid = if (tMag > 1e-6f) wSum / tMag else 0f

                    // Spectral flux (use previous data stored in analyzer)
                    val flux = state.analyzer.computeFluxFrom(mags)

                    spectralData = SpectralData(
                        bandEnergies = bandEnergies,
                        rmsPower = rmsPower,
                        spectralCentroid = centroid,
                        spectralFlux = flux,
                        dominantFrequency = 0f
                    )

                    // Extract raw energy (pre-volume) for gate, boosted for envelope
                    val rawEnergy = SpectralAnalyzer.extractEnergy(spectralData, params.frequencyMode, params.targetFrequency)
                    energy = rawEnergy * params.mainVolume
                }
            } else {
                // ---- Raw sample path (mic or fallback) ----
                // Do NOT apply volume before FFT so the gate sees true
                // pre-volume energy (matching the Visualizer path and
                // Rust desktop behavior). Volume is applied to energy
                // after extractEnergy() below.
                val samples: FloatArray
                synchronized(sampleLock) {
                    if (!hasFreshSamples) {
                        samples = FloatArray(FFT_SIZE)
                    } else {
                        samples = capturedSamples
                        hasFreshSamples = false
                    }
                }
                spectralData = state.analyzer.analyze(samples, 1)
                val rawEnergy = SpectralAnalyzer.extractEnergy(spectralData, params.frequencyMode, params.targetFrequency)
                energy = rawEnergy * params.mainVolume
            }

            // Raw energy (0-1 normalized, no volume) for the gate so the
            // threshold slider maps cleanly: 0% = always open, 100% = closed.
            // Volume-boosted energy feeds the envelope for dynamics.
            val rawEnergy = if (params.mainVolume > 0.001f) (energy / params.mainVolume).coerceIn(0f, 1f) else 0f

            state.lastSpectralData = spectralData
            state.lastEnergy = energy

            // Step 3: Gate (uses raw energy so threshold isn't defeated by volume)
            val gateOpen = state.gate.process(
                rawEnergy, params.gateThreshold, params.autoGateAmount, params.gateSmoothing
            )
            state.lastGateOpen = gateOpen

            // Step 4: Beat detection
            val (detectedOnset, onsetStrength) = state.beatDetector.process(
                spectralData.spectralFlux, currentTimeMs
            )

            // Predictive onset: when tempo is locked, pre-trigger ~2 BLE
            // frames early so the attack command arrives on-beat instead
            // of 85-115ms late. False positives feel like syncopation;
            // late delivery feels like lag.
            var isOnset = detectedOnset
            if (!isOnset && state.beatDetector.tempoConfidence > 0.6f) {
                val predicted = state.beatDetector.predictedNextOnsetMs
                if (predicted > 0f) {
                    val leadTimeMs = 76f // ~2 BLE write intervals
                    val timeToPredicted = predicted - currentTimeMs
                    if (timeToPredicted in 0f..leadTimeMs && gateOpen) {
                        isOnset = true
                    }
                }
            }

            // Rust-parity onset pre-gate (gui.rs:1408-1410): reject weak
            // or low-energy "onsets" before they reach the envelope, so
            // rhythm rider presets don't retrigger on background texture.
            if (isOnset && (onsetStrength <= 1.02f || energy <= params.gateThreshold * 0.40f)) {
                isOnset = false
            }

            // Step 5: Envelope
            val envelopeOutput = state.envelope.drive(
                gateOpen = gateOpen,
                energy = energy,
                isOnset = isOnset,
                onsetStrength = onsetStrength,
                currentTimeMs = currentTimeMs,
                triggerMode = params.triggerMode,
                threshold = params.gateThreshold,
                thresholdKnee = params.thresholdKnee,
                dynamicCurve = params.dynamicCurve,
                binaryLevel = params.binaryLevel,
                hybridBlend = params.hybridBlend,
                attackMs = params.attackMs,
                decayMs = params.decayMs,
                sustainLevel = params.sustainLevel,
                releaseMs = params.releaseMs,
                attackCurve = params.attackCurve,
                decayCurve = params.decayCurve,
                releaseCurve = params.releaseCurve,
                spectralCentroid = spectralData.spectralCentroid
            )
            state.lastEnvelopeOutput = envelopeOutput
            state.lastEnvelopeState = state.envelope.state

            // Step 6: Climax engine
            val climaxOutput = state.climaxEngine.process(
                input = envelopeOutput,
                energy = energy,
                gateOpen = gateOpen,
                isOnset = isOnset,
                onsetStrength = onsetStrength,
                currentTimeMs = currentTimeMs,
                enabled = params.climaxEnabled,
                intensity = params.climaxIntensity,
                buildUpMs = params.climaxBuildUpMs,
                teaseRatio = params.climaxTeaseRatio,
                teaseDrop = params.climaxTeaseDrop,
                surgeBoost = params.climaxSurgeBoost,
                pulseDepth = params.climaxPulseDepth,
                pattern = params.climaxPattern
            )

            // Step 7: Apply output range mapping
            val mapped = if (climaxOutput > 0.001f) {
                params.minVibe + (params.maxVibe - params.minVibe) * climaxOutput
            } else {
                0f
            }
            val finalOutput = (mapped * params.outputGain).coerceIn(0f, 1f)
            state.lastFinalOutput = finalOutput

            // Notify listener -- skip redundant Vibrate:0 commands so the BLE
            // write gate is clear when a real trigger arrives.  Send the stop
            // command once when output drops to zero, then go silent.
            if (finalOutput > 0.001f || lastSentOutput > 0.001f) {
                // Dual-motor output: climax engine generates phase-offset
                // signal for motor 2, creating spatial movement
                val dualCb = onDualOutputUpdate
                if (dualCb != null && params.climaxEnabled) {
                    val motor2Raw = state.climaxEngine.motor2Output
                    val motor2Mapped = if (motor2Raw > 0.001f) {
                        params.minVibe + (params.maxVibe - params.minVibe) * motor2Raw
                    } else {
                        0f
                    }
                    val motor2Final = (motor2Mapped * params.outputGain).coerceIn(0f, 1f)
                    dualCb.invoke(finalOutput, motor2Final)
                } else {
                    onOutputUpdate?.invoke(finalOutput)
                }
                lastSentOutput = finalOutput
            }

            // Frame completed successfully -- reset error counter
            consecutiveErrors = 0

            // Maintain ~60Hz by sleeping only the remainder of this frame budget.
            val elapsedMs = (System.nanoTime() - frameStartNs) / 1_000_000L
            val sleepMs = TARGET_FRAME_MS - elapsedMs
            if (sleepMs > 0L) {
                try {
                    Thread.sleep(sleepMs)
                } catch (_: InterruptedException) {
                    break
                }
            } else {
                Thread.yield()
            }
          } catch (e: Exception) {
              Log.e("ChloeVibes", "Processing frame error", e)
              consecutiveErrors++
              if (consecutiveErrors > 100) {
                  Log.w("ChloeVibes", "Persistent processing errors ($consecutiveErrors consecutive), breaking out of processing loop")
                  break
              }
          }
        }
    }
}
