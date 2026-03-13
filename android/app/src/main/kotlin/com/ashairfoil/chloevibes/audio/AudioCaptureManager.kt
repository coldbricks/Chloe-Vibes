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

    // Silence / stall detection for Visualizer fallback
    @Volatile private var silentFrameCount = 0
    private companion object {
        /** After this many consecutive silent frames, restart Visualizer. */
        const val SILENT_FRAMES_BEFORE_FALLBACK = 90 // ~1.5 seconds at 60Hz
        /** If Visualizer stops delivering samples for this long, it's dead. */
        const val STALL_TIMEOUT_MS = 3000L
        /**
         * Gain applied to Visualizer waveform data. The Visualizer API delivers
         * 8-bit unsigned bytes (resolution ~0.008 per step), which produces
         * energy values roughly 10x lower than raw PCM. This gain brings the
         * signal into the range the gate/envelope chain expects.
         */
        const val VISUALIZER_GAIN = 10f
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
                        // Convert unsigned byte waveform to float and apply
                        // Visualizer gain. The API gives 8-bit data (~10x quieter
                        // than raw PCM), so we scale up here before the pipeline.
                        val samples = FloatArray(waveform.size) { i ->
                            val raw = (waveform[i].toInt() and 0xFF).toFloat() / 128f - 1f
                            (raw * VISUALIZER_GAIN).coerceIn(-1f, 1f)
                        }
                        synchronized(sampleLock) {
                            capturedSamples = samples
                            hasFreshSamples = true
                        }
                        lastSampleTimeMs = System.currentTimeMillis()
                    }

                    override fun onFftDataCapture(
                        visualizer: Visualizer,
                        fft: ByteArray,
                        samplingRate: Int
                    ) {
                        // We use waveform capture + our own FFT for consistency
                        // with the Rust version's spectral analysis
                    }
                },
                Visualizer.getMaxCaptureRate(),
                true,  // waveform
                false  // fft
            )
            viz.enabled = true
            visualizer = viz
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

            // Grab latest samples
            val samples: FloatArray
            synchronized(sampleLock) {
                if (!hasFreshSamples) {
                    samples = FloatArray(FFT_SIZE)
                } else {
                    samples = capturedSamples
                    hasFreshSamples = false
                }
            }

            // Detect dead Visualizer: either stopped calling back or producing silence.
            // When detected, tear it down and re-create it (Samsung kills session 0
            // periodically but a fresh Visualizer usually works again).
            if (sourceMode == AudioSourceMode.SystemAudio) {
                val stalled = visualizer != null &&
                    (System.currentTimeMillis() - lastSampleTimeMs) > STALL_TIMEOUT_MS
                var rmsCheck = 0f
                for (s in samples) rmsCheck += s * s
                val isSilent = samples.isEmpty() || (rmsCheck / samples.size.coerceAtLeast(1)) < 1e-10f

                if (isSilent) silentFrameCount++ else silentFrameCount = 0

                if (stalled || silentFrameCount >= SILENT_FRAMES_BEFORE_FALLBACK) {
                    // Tear down and re-create the Visualizer
                    silentFrameCount = 0
                    try { visualizer?.apply { enabled = false; release() } } catch (_: Exception) {}
                    visualizer = null
                    startVisualizer()
                    lastSampleTimeMs = System.currentTimeMillis()
                }
            }

            // Apply volume gain
            val gained = FloatArray(samples.size) { i -> samples[i] * mainVolume }

            // Step 1: Spectral analysis
            val spectralData = state.analyzer.analyze(gained, 1)
            state.lastSpectralData = spectralData

            // Step 2: Extract energy for current frequency mode
            val energy = SpectralAnalyzer.extractEnergy(spectralData, frequencyMode, targetFrequency)
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
