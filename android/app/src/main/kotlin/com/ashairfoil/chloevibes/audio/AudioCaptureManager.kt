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
import androidx.core.content.ContextCompat

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

    // Visualizer FFT magnitude data (0.0-1.0 per bin, already dB-normalized)
    private var capturedMagnitudes = FloatArray(0)
    private var hasFreshMagnitudes = false
    @Volatile private var useVisualizerFft = false
    @Volatile private var visualizerSampleRate = 48000

    // Silence / stall detection for Visualizer fallback
    @Volatile private var silentFrameCount = 0
    private companion object {
        const val SILENT_FRAMES_BEFORE_FALLBACK = 90
        const val STALL_TIMEOUT_MS = 3000L
    }

    // Signal processing parameters (set from UI thread)
    @Volatile var mainVolume: Float = 1.15f
    @Volatile var frequencyMode: FrequencyMode = FrequencyMode.Full
    @Volatile var targetFrequency: Float = 200f
    @Volatile var gateThreshold: Float = 0.07f
    @Volatile var autoGateAmount: Float = 0f
    @Volatile var gateSmoothing: Float = 0.22f
    @Volatile var thresholdKnee: Float = 0.22f
    @Volatile var triggerMode: TriggerMode = TriggerMode.Dynamic
    @Volatile var binaryLevel: Float = 0.8f
    @Volatile var hybridBlend: Float = 0.5f
    @Volatile var dynamicCurve: Float = 1f
    @Volatile var attackMs: Float = 30f
    @Volatile var decayMs: Float = 160f
    @Volatile var sustainLevel: Float = 0.9f
    @Volatile var releaseMs: Float = 320f
    @Volatile var attackCurve: Float = 1f
    @Volatile var decayCurve: Float = 1f
    @Volatile var releaseCurve: Float = 1.15f
    @Volatile var minVibe: Float = 0f
    @Volatile var maxVibe: Float = 1f

    // Climax engine params
    @Volatile var climaxEnabled: Boolean = false
    @Volatile var climaxIntensity: Float = 0.7f
    @Volatile var climaxBuildUpMs: Float = 90_000f
    @Volatile var climaxTeaseRatio: Float = 0.18f
    @Volatile var climaxTeaseDrop: Float = 0.35f
    @Volatile var climaxSurgeBoost: Float = 0.5f
    @Volatile var climaxPulseDepth: Float = 0.18f
    @Volatile var climaxPattern: ClimaxPattern = ClimaxPattern.Wave

    // Output callback
    var onOutputUpdate: ((Float) -> Unit)? = null

    /** Called when Visualizer produces silence and we auto-fallback to mic. */
    var onFallbackToMic: (() -> Unit)? = null

    /** Apply a preset to all signal processing parameters. */
    fun applyPreset(preset: Preset) {
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

    /**
     * Start audio capture and processing.
     *
     * @param mode which audio source to use
     * @return true if started successfully
     */
    fun start(mode: AudioSourceMode = AudioSourceMode.SystemAudio): Boolean {
        if (running) return true
        sourceMode = mode

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
        }
        return started
    }

    /** Stop audio capture and processing. */
    fun stop() {
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
                        // minDb/maxDb matching the HTML version's AnalyserNode
                        val minDb = -80f
                        val maxDb = -10f
                        val dbRange = maxDb - minDb

                        for (i in 0 until numBins) {
                            val re = fft[2 * i].toFloat()
                            val im = fft[2 * i + 1].toFloat()
                            val mag = kotlin.math.sqrt(re * re + im * im)
                            // Convert to dB, then normalize to 0-1 like Web Audio
                            val db = if (mag > 0f) 20f * kotlin.math.log10(mag / 128f) else minDb
                            mags[i] = ((db - minDb) / dbRange).coerceIn(0f, 1f)
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
            false
        }
    }

    // -----------------------------------------------------------------------
    // Processing loop (~60Hz)
    // -----------------------------------------------------------------------

    private fun processingLoop() {
        val startMs = System.nanoTime() / 1_000_000f
        lastSampleTimeMs = System.currentTimeMillis()

        while (running) {
          try {
            val currentTimeMs = System.nanoTime() / 1_000_000f - startMs

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
                        startVisualizer()
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

                    // Apply volume as a gain on the energy
                    energy = SpectralAnalyzer.extractEnergy(spectralData, frequencyMode, targetFrequency) * mainVolume
                }
            } else {
                // ---- Raw sample path (mic or fallback) ----
                val samples: FloatArray
                synchronized(sampleLock) {
                    if (!hasFreshSamples) {
                        samples = FloatArray(FFT_SIZE)
                    } else {
                        samples = capturedSamples
                        hasFreshSamples = false
                    }
                }
                val gained = FloatArray(samples.size) { i -> samples[i] * mainVolume }
                spectralData = state.analyzer.analyze(gained, 1)
                energy = SpectralAnalyzer.extractEnergy(spectralData, frequencyMode, targetFrequency)
            }

            state.lastSpectralData = spectralData
            state.lastEnergy = energy

            // Step 3: Gate
            val gateOpen = state.gate.process(
                energy, gateThreshold, autoGateAmount, gateSmoothing, thresholdKnee
            )
            state.lastGateOpen = gateOpen

            // Step 4: Beat detection
            val (isOnset, onsetStrength) = state.beatDetector.process(
                spectralData.spectralFlux, currentTimeMs
            )

            // Step 5: Envelope
            val envelopeOutput = state.envelope.drive(
                gateOpen = gateOpen,
                energy = energy,
                isOnset = isOnset,
                onsetStrength = onsetStrength,
                currentTimeMs = currentTimeMs,
                triggerMode = triggerMode,
                threshold = gateThreshold,
                thresholdKnee = thresholdKnee,
                dynamicCurve = dynamicCurve,
                binaryLevel = binaryLevel,
                hybridBlend = hybridBlend,
                attackMs = attackMs,
                decayMs = decayMs,
                sustainLevel = sustainLevel,
                releaseMs = releaseMs,
                attackCurve = attackCurve,
                decayCurve = decayCurve,
                releaseCurve = releaseCurve
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
                enabled = climaxEnabled,
                intensity = climaxIntensity,
                buildUpMs = climaxBuildUpMs,
                teaseRatio = climaxTeaseRatio,
                teaseDrop = climaxTeaseDrop,
                surgeBoost = climaxSurgeBoost,
                pulseDepth = climaxPulseDepth,
                pattern = climaxPattern
            )

            // Step 7: Apply output range mapping
            val mapped = if (climaxOutput > 0.001f) {
                minVibe + (maxVibe - minVibe) * climaxOutput
            } else {
                0f
            }
            val finalOutput = mapped.coerceIn(0f, 1f)
            state.lastFinalOutput = finalOutput

            // Notify listener
            onOutputUpdate?.invoke(finalOutput)

            // ~60Hz processing rate
            try {
                Thread.sleep(16)
            } catch (_: InterruptedException) {
                break
            }
          } catch (_: Exception) {
              // Don't let a single frame crash kill the processing thread
          }
        }
    }
}
