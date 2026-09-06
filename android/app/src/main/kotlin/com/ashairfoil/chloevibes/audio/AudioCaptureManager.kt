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
import android.media.AudioManager
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.audiofx.Visualizer
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.exp
import kotlin.math.pow

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
    var analyzer = SpectralAnalyzer(48000f)
    var gate = Gate()
    val envelope = EnvelopeProcessor()
    var beatDetector = BeatDetector()
    val climaxEngine = ClimaxEngine()

    @Volatile var lastSpectralData = SpectralData()
    @Volatile var lastEnergy: Float = 0f
    @Volatile var lastGateOpen: Boolean = false
    @Volatile var lastEnvelopeOutput: Float = 0f
    @Volatile var lastFinalOutput: Float = 0f
    @Volatile var lastEnvelopeState: EnvelopeState = EnvelopeState.Idle
    /** Climax cycle progress in [0, 1) for UI CYCLE % readout. */
    @Volatile var lastClimaxPhase: Float = 0f
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
    // Bass-drum boom defaults (~125 BPM): kick lock, long natural decay, low floor.
    val mainVolume: Float = 1.90f,
    val frequencyMode: FrequencyMode = FrequencyMode.LowPass,
    val targetFrequency: Float = 120f,
    val gateThreshold: Float = 0.14f,
    val autoGateAmount: Float = 0f,
    val gateSmoothing: Float = 0.06f,
    val thresholdKnee: Float = 0.15f,
    val triggerMode: TriggerMode = TriggerMode.Hybrid,
    val binaryLevel: Float = 0.82f,
    val hybridBlend: Float = 0.58f,
    val dynamicCurve: Float = 1.2f,
    val attackMs: Float = 20f,
    val decayMs: Float = 375f,
    val sustainLevel: Float = 0.08f,
    val releaseMs: Float = 240f,
    val attackCurve: Float = 0.7f,
    val decayCurve: Float = 1.8f,
    val releaseCurve: Float = 1.3f,
    val minVibe: Float = 0f,
    val maxVibe: Float = 1f,
    val outputGain: Float = 1f,
    // Fall path is 1.0× this; rise is 0.35× (0.15× on large jumps). 42 ms boom trough.
    val outputSlewMs: Float = 42f,
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
    private val processingClock = AudioProcessingClock()

    /** Queue UI resets so the processing thread supplies the correct epoch. */
    fun requestClimaxReset() = processingClock.requestClimaxReset()

    // Audio source
    @Volatile var sourceMode: AudioSourceMode = AudioSourceMode.SystemAudio
        private set
    @Volatile private var visualizer: Visualizer? = null
    @Volatile private var audioRecord: AudioRecord? = null

    // Processing thread
    @Volatile private var processingThread: Thread? = null
    @Volatile private var microphoneThread: Thread? = null
    @Volatile private var running = false

    // Latest captured samples (written by capture, read by processing)
    private val sampleLock = Object()
    private var capturedSamples = FloatArray(0)
    private val captureCadence = AudioFrameCadence()
    @Volatile private var lastSampleTimeMs = 0L

    // Visualizer FFT magnitude data (linear, 0.0-1.0 per bin, Rust-parity)
    private var capturedMagnitudes = FloatArray(0)
    @Volatile private var useVisualizerFft = false
    @Volatile private var visualizerSampleRate = 48000

    // Silent audio is valid input. Only missing callbacks trigger recovery.
    private companion object {
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
    var outputSlewMs: Float
        get() = paramsRef.get().outputSlewMs
        set(v) { paramsRef.updateAndGet { it.copy(outputSlewMs = v) } }
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
    @Volatile var hasRecentInput: Boolean = false
        private set
    private var outputLevel: Float = 0f
    private var outputLevel2: Float = 0f

    /**
     * Monotonic [SystemClock.elapsedRealtime] stamp updated once per successful
     * processing frame. Application dead-man ages this; stale → stop motors.
     */
    @Volatile var lastPipelineHeartbeatMs: Long = 0L
        private set

    /**
     * Invoked on the processing thread when the loop exits after persistent
     * errors. Application must fence output and stop motors (fail-closed).
     */
    var onPipelineFailClosed: (() -> Unit)? = null

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
            outputSlewMs = preset.outputSlewMs,
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
    @Synchronized
    fun start(mode: AudioSourceMode = AudioSourceMode.SystemAudio): Boolean {
        if (running) return true
        // A stopped producer may still be unwinding a platform callback.
        // Never let it share engine state with a new processing thread.
        if (processingThread?.isAlive == true) return false
        sourceMode = mode
        synchronized(sampleLock) {
            capturedSamples = FloatArray(0)
            capturedMagnitudes = FloatArray(0)
            captureCadence.clear()
        }
        state.envelope.reset()
        state.beatDetector = BeatDetector()
        state.gate = Gate()
        state.analyzer = SpectralAnalyzer(48000f)
        state.lastSpectralData = SpectralData()
        outputLevel = 0f
        outputLevel2 = 0f
        hasRecentInput = false
        visualizerRestartFailures = 0
        running = true // The microphone worker must see this before it starts.

        Log.i("ChloeVibes", "Starting audio capture in mode: $mode")
        val started = when (mode) {
            AudioSourceMode.SystemAudio -> startVisualizer()
            AudioSourceMode.Microphone -> startMicrophone()
        }

        processingClock.onCaptureStartResult(started)
        if (started) {
            running = true
            // Arm heartbeat before the first frame so the Application watchdog
            // has a fresh baseline (avoids an immediate false trip).
            lastPipelineHeartbeatMs = SystemClock.elapsedRealtime()
            processingThread = Thread(::processingLoop, "ChloeVibes-Processing").apply {
                priority = Thread.MAX_PRIORITY
                isDaemon = true
                start()
            }
            Log.i("ChloeVibes", "Audio capture started successfully")
        } else {
            running = false
            Log.w("ChloeVibes", "Audio capture failed to start in mode: $mode")
        }
        return started
    }

    /** Stop audio capture and processing. */
    @Synchronized
    fun stop() {
        Log.i("ChloeVibes", "Stopping audio capture")
        running = false
        processingThread?.interrupt()
        val (oldVisualizer, oldRecord) = synchronized(sampleLock) {
            val old = Pair(visualizer, audioRecord)
            visualizer = null
            audioRecord = null
            captureCadence.clear()
            old
        }
        val oldMicThread = microphoneThread
        microphoneThread = null
        useVisualizerFft = false
        // Fence callbacks before releasing resources. Platform AudioRecord.stop
        // may wait for an in-flight read, so cleanup must not block the UI.
        oldMicThread?.interrupt()
        Thread({
            runCatching { oldVisualizer?.enabled = false }
            runCatching { oldVisualizer?.release() }
            runCatching { oldRecord?.stop() }
            oldMicThread?.join(500L)
            runCatching { oldRecord?.release() }
        }, "ChloeVibes-CaptureCleanup").apply { isDaemon = true; start() }
        synchronized(sampleLock) { captureCadence.clear() }
        state.lastFinalOutput = 0f
        hasRecentInput = false
        lastPipelineHeartbeatMs = 0L
    }

    val isRunning: Boolean get() = running

    // -----------------------------------------------------------------------
    // Visualizer API (system audio)
    // -----------------------------------------------------------------------

    private fun startVisualizer(): Boolean {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) return false

        var pendingVisualizer: Visualizer? = null
        return try {
            val viz = Visualizer(0) // session 0 = system audio output mix
            pendingVisualizer = viz
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
                        if (!running || visualizer !== this@AudioCaptureManager.visualizer || fft.size < 4) return
                        val numBins = fft.size / 2
                        val mags = FloatArray(numBins)
                        // Linear magnitudes (no dB conversion). Matches Rust
                        // SpectralAnalyzer's linear FFT output so band energies
                        // have the same tonal balance on both platforms.
                        // Raw Visualizer bytes are in ~[-128,127], so sqrt(re^2+im^2)
                        // peaks around 181; normalize by captureSize/2 (matches
                        // Rust's 2.0/FFT_SIZE convention) and clamp.
                        val linearScale = 2f / numBins.toFloat()
                        // Visualizer FFT layout: fft[0] = Re(DC), fft[1] =
                        // Re(Nyquist), then Re/Im pairs for bins 1..N/2-1.
                        // Treating [0]/[1] as a complex pair mixed DC with
                        // Nyquist energy in bin 0.
                        mags[0] = (kotlin.math.abs(fft[0].toFloat()) * linearScale)
                            .coerceIn(0f, 1f)
                        for (i in 1 until numBins) {
                            val re = fft[2 * i].toFloat()
                            val im = fft[2 * i + 1].toFloat()
                            val mag = kotlin.math.sqrt(re * re + im * im) * linearScale
                            mags[i] = mag.coerceIn(0f, 1f)
                        }
                        synchronized(sampleLock) {
                            if (!running || visualizer !== this@AudioCaptureManager.visualizer) return
                            capturedMagnitudes = mags
                            captureCadence.received(SystemClock.elapsedRealtime())
                            visualizerSampleRate = samplingRate / 1000
                            lastSampleTimeMs = SystemClock.elapsedRealtime()
                        }
                    }
                },
                Visualizer.getMaxCaptureRate(),
                false, // waveform — not needed
                true   // fft — use this instead
            )
            synchronized(sampleLock) {
                if (!running) {
                    viz.release()
                    return false
                }
                visualizer = viz
                useVisualizerFft = true
                viz.enabled = true
            }
            lastSampleTimeMs = SystemClock.elapsedRealtime()
            true
        } catch (e: Exception) {
            if (visualizer === pendingVisualizer) visualizer = null
            runCatching { pendingVisualizer?.release() }
            Log.e("ChloeVibes", "Visualizer initialization failed", e)
            false
        }
    }

    // -----------------------------------------------------------------------
    // AudioRecord (microphone fallback)
    // -----------------------------------------------------------------------

    /**
     * True while telephony or VoIP audio is active. The silence/stall fallback
     * must never open the microphone mid-call and turn a private conversation
     * into toy output. AudioManager.mode needs no extra permission.
     */
    private fun isCallActive(): Boolean {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
        return am != null &&
            (am.mode == AudioManager.MODE_IN_CALL ||
                am.mode == AudioManager.MODE_IN_COMMUNICATION ||
                am.mode == AudioManager.MODE_RINGTONE)
    }

    private fun startMicrophone(): Boolean {
        if (isCallActive()) return false
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) return false

        var pendingRecord: AudioRecord? = null
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
            pendingRecord = record

            if (record.state != AudioRecord.STATE_INITIALIZED) {
                record.release()
                return false
            }

            record.startRecording()
            audioRecord = record
            useVisualizerFft = false
            lastSampleTimeMs = SystemClock.elapsedRealtime()

            // Mic capture runs on its own thread feeding samples
            microphoneThread = Thread({
                val buffer = FloatArray(FFT_SIZE)
                try {
                    while (running && audioRecord === record) {
                        val read = record.read(buffer, 0, buffer.size, AudioRecord.READ_BLOCKING)
                        if (read < 0) {
                            Log.w("ChloeVibes", "Microphone read failed: $read")
                            break
                        }
                        if (read > 0) {
                            val samples = buffer.copyOf(read)
                            synchronized(sampleLock) {
                                if (running && audioRecord === record) {
                                    capturedSamples = samples
                                    captureCadence.received(SystemClock.elapsedRealtime())
                                    lastSampleTimeMs = SystemClock.elapsedRealtime()
                                }
                            }
                        }
                    }
                } catch (e: Exception) {
                    if (running && audioRecord === record) Log.w("ChloeVibes", "Microphone read stopped", e)
                }
            }, "ChloeVibes-MicCapture").apply {
                isDaemon = true
                start()
            }

            true
        } catch (e: Exception) {
            if (audioRecord === pendingRecord) audioRecord = null
            runCatching { pendingRecord?.release() }
            Log.w("ChloeVibes", "Microphone capture initialization failed", e)
            false
        }
    }

    // A failed recovery stays on the selected source; it never activates a mic.
    private var visualizerRestartFailures = 0

    // -----------------------------------------------------------------------
    // Processing loop (~60Hz)
    // -----------------------------------------------------------------------

    private fun processingLoop() {
        var captureWasMissing = true
        val outputDispatch = AudioOutputDispatch()
        var consecutiveErrors = 0
        var lastFrameNs = System.nanoTime()

        while (running) {
          try {
            val frameStartNs = System.nanoTime()
            val deltaTimeS = ((frameStartNs - lastFrameNs).toFloat() / 1_000_000_000f)
                .coerceIn(0.001f, 0.1f)
            lastFrameNs = frameStartNs
            val currentTimeMs = processingClock.elapsedMs(frameStartNs)
            processingClock.applyPendingClimaxReset(state.climaxEngine, frameStartNs)

            // Read all parameters once per frame for a consistent snapshot
            val params = paramsRef.get()

            val nowCaptureMs = SystemClock.elapsedRealtime()
            if (sourceMode == AudioSourceMode.Microphone && isCallActive()) {
                running = false
                onPipelineFailClosed?.invoke()
                break
            }
            if (nowCaptureMs - lastSampleTimeMs > STALL_TIMEOUT_MS) {
                if (sourceMode == AudioSourceMode.SystemAudio && visualizerRestartFailures < 3) {
                    visualizerRestartFailures++
                    val oldVisualizer = visualizer
                    visualizer = null
                    runCatching { oldVisualizer?.enabled = false }
                    runCatching { oldVisualizer?.release() }
                    synchronized(sampleLock) { captureCadence.clear() }
                    startVisualizer()
                    lastSampleTimeMs = nowCaptureMs
                } else {
                    Log.w("ChloeVibes", "Selected audio source stopped delivering frames")
                    running = false
                    onPipelineFailClosed?.invoke()
                    break
                }
            }

            val frameStatus: AudioFrameCadence.Frame
            val samples: FloatArray
            val mags: FloatArray
            val frameSampleRate: Int
            synchronized(sampleLock) {
                frameStatus = captureCadence.poll(SystemClock.elapsedRealtime())
                samples = capturedSamples
                mags = capturedMagnitudes
                frameSampleRate = visualizerSampleRate
            }
            val freshCapture = frameStatus == AudioFrameCadence.Frame.Fresh
            val missingCapture = frameStatus == AudioFrameCadence.Frame.Missing
            hasRecentInput = !missingCapture
            if (freshCapture) visualizerRestartFailures = 0
            if (missingCapture && !captureWasMissing) {
                state.gate = Gate()
                state.beatDetector = BeatDetector()
                state.analyzer = SpectralAnalyzer(48000f)
            }
            captureWasMissing = missingCapture
            val spectralData = when {
                missingCapture -> SpectralData()
                !freshCapture -> state.lastSpectralData
                useVisualizerFft && mags.isNotEmpty() -> {
                    // Build SpectralData from Visualizer magnitudes.
                    // Bin resolution depends on capture size and sample rate.
                    val sr = frameSampleRate.toFloat()
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

                    // Spectral centroid — skip the DC bin, mirroring
                    // SpectralAnalyzer and the Rust engine (skip(1)): DC
                    // magnitude inflates totalMag and biases the centroid
                    // downward. This path missed the 2026-05-29 fix.
                    var wSum = 0f; var tMag = 0f
                    for (i in 1 until mags.size) {
                        val freq = i * binRes
                        wSum += freq * mags[i]; tMag += mags[i]
                    }
                    val centroid = if (tMag > 1e-6f) wSum / tMag else 0f

                    // Spectral flux (use previous data stored in analyzer)
                    val flux = state.analyzer.computeFluxFrom(mags)

                    SpectralData(
                        bandEnergies = bandEnergies,
                        rmsPower = rmsPower,
                        spectralCentroid = centroid,
                        spectralFlux = flux,
                        dominantFrequency = 0f
                    )
                }
                else -> state.analyzer.analyze(samples, 1)
            }
            val captureEnergy = SpectralAnalyzer.extractEnergy(
                spectralData, params.frequencyMode, params.targetFrequency
            )
            val rawEnergy = normalizeCaptureEnergy(captureEnergy)
            val energy = sanitizeUnit(rawEnergy * params.mainVolume)

            state.lastSpectralData = spectralData
            state.lastEnergy = energy

            // Step 3: Gate (uses raw energy so threshold isn't defeated by volume)
            val gateOpen = when {
                missingCapture -> false
                freshCapture -> state.gate.process(
                    rawEnergy, params.gateThreshold, params.autoGateAmount, params.gateSmoothing
                )
                else -> state.lastGateOpen
            }
            val effectiveThreshold = state.gate.effectiveThreshold(
                params.gateThreshold, params.autoGateAmount
            )
            state.lastGateOpen = gateOpen

            // Step 4: Beat detection
            val (detectedOnset, onsetStrength) = if (freshCapture) {
                state.beatDetector.process(spectralData.spectralFlux, currentTimeMs)
            } else {
                state.beatDetector.advanceTime(currentTimeMs)
                Pair(false, 0f)
            }

            // Predictive onset: takePrefire injects strength floor + one-shot
            // latch (mirrors Rust BeatDetector::take_prefire). Confidence
            // decays without onsets so stale locks cannot ghost-fire.
            var isOnset = detectedOnset && !state.beatDetector.isPrefiredOnset(currentTimeMs)
            var onsetStr = onsetStrength
            var syntheticPrefire = false
            if (!detectedOnset && gateOpen && !missingCapture) {
                val preStr = state.beatDetector.takePrefire(currentTimeMs)
                if (preStr != null) {
                    isOnset = true
                    syntheticPrefire = true
                    onsetStr = preStr
                }
            }

            // Rust-parity onset pre-gate: single threshold 1.05 (matches envelope
            // drive; kills 1.02–1.05 dead band). Prefire strength is already ≥1.15.
            if (isOnset && (onsetStr <= 1.05f || energy <= params.gateThreshold * 0.40f)) {
                isOnset = false
            }

            // Step 5: Envelope
            val envelopeOutput = state.envelope.drive(
                gateOpen = gateOpen,
                energy = energy,
                isOnset = isOnset,
                onsetStrength = onsetStr,
                currentTimeMs = currentTimeMs,
                triggerMode = params.triggerMode,
                threshold = effectiveThreshold,
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
            if (syntheticPrefire && isOnset && envelopeOutput > 0f && state.envelope.triggeredAt(currentTimeMs)) {
                state.beatDetector.confirmPrefire(currentTimeMs)
            }
            state.lastEnvelopeOutput = envelopeOutput
            state.lastEnvelopeState = state.envelope.state

            // Step 6: Climax engine
            val climaxOutput = state.climaxEngine.process(
                input = envelopeOutput,
                energy = energy,
                gateOpen = gateOpen,
                isOnset = isOnset,
                onsetStrength = onsetStr,
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
            state.lastClimaxPhase = if (params.climaxEnabled) {
                state.climaxEngine.phaseProgress(currentTimeMs, params.climaxBuildUpMs)
            } else {
                0f
            }

            // Step 7: Output mapping (shared with Rust via map_output) + slew.
            // mapOutput zeroes the target when silent or effectively zero, so
            // the motor pumps back to rest; the slew then smooths toward it.
            val isSilent = energy < 0.005f &&
                    !gateOpen &&
                    state.envelope.state == EnvelopeState.Idle
            val silenceClass = missingCapture || isSilent
                    || state.envelope.silenceEvent
                    || state.climaxEngine.silenceEvent
            val targetOutput = mapOutput(
                climaxOutput, params.minVibe, params.maxVibe, params.outputGain, silenceClass
            )
            // Silence-class (micro-pause / boom rest / deep deny) hard-snaps
            // past slew so the motor actually stops; large upward jumps punch.
            outputLevel = if (silenceClass || targetOutput <= 0.001f) {
                0f
            } else {
                smoothOutput(outputLevel, targetOutput, deltaTimeS, params.outputSlewMs)
            }
            val finalOutput = outputLevel.coerceIn(0f, sanitizeUnit(params.maxVibe))
            state.lastFinalOutput = finalOutput

            // Compute both motors even when motor 1 is already at zero.
            val motor2Target = if (silenceClass) {
                0f
            } else if (params.climaxEnabled) {
                mapOutput(state.climaxEngine.motor2Output, params.minVibe,
                    params.maxVibe, params.outputGain, false)
            } else {
                val be = spectralData.bandEnergies
                val lowE = be[0] + be[1] + be[2] + be[3]
                val highE = be[4] + be[5] + be[6] + be[7]
                val total = lowE + highE
                val trebleWeight = if (total > 1e-9f) (3f * highE / total).coerceIn(0f, 1f) else 0f
                finalOutput * trebleWeight
            }
            outputLevel2 = if (silenceClass || motor2Target <= 0.001f) 0f else
                smoothOutput(outputLevel2, motor2Target, deltaTimeS, params.outputSlewMs)
            val motor2Final = outputLevel2.coerceIn(0f, sanitizeUnit(params.maxVibe))
            if (running && outputDispatch.shouldSend(finalOutput, motor2Final)) {
                val dualCb = onDualOutputUpdate
                if (dualCb != null) dualCb.invoke(finalOutput, motor2Final)
                else onOutputUpdate?.invoke(finalOutput)
            }

            // Frame completed successfully -- stamp dead-man heartbeat and
            // reset the error counter. Stamp after output so a hang inside
            // the BLE write path still ages out.
            lastPipelineHeartbeatMs = SystemClock.elapsedRealtime()
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
                  // Fail-closed: do not leave the last intensity on a body.
                  Log.w(
                      "ChloeVibes",
                      "Persistent processing errors ($consecutiveErrors consecutive), fail-closed"
                  )
                  running = false
                  outputLevel = 0f
                  outputLevel2 = 0f
                  outputDispatch.reset()
                  state.lastFinalOutput = 0f
                  try {
                      val dualCb = onDualOutputUpdate
                      if (dualCb != null) {
                          dualCb.invoke(0f, 0f)
                      } else {
                          onOutputUpdate?.invoke(0f)
                      }
                  } catch (zeroErr: Exception) {
                      Log.e("ChloeVibes", "Fail-closed zero emit failed", zeroErr)
                  }
                  try {
                      onPipelineFailClosed?.invoke()
                  } catch (cbErr: Exception) {
                      Log.e("ChloeVibes", "onPipelineFailClosed failed", cbErr)
                  }
                  break
              }
          }
        }
    }
}

private fun sanitizeUnit(value: Float): Float =
    if (value.isFinite()) value.coerceIn(0f, 1f) else 0f

private fun normalizeCaptureEnergy(value: Float): Float {
    val boosted = sanitizeUnit(value * 6f)
    return sanitizeUnit(boosted.toDouble().pow(0.65).toFloat())
}

private fun smoothingAlpha(deltaTimeS: Float, timeMs: Float): Float {
    if (timeMs <= 1f) return 1f
    val tau = (timeMs / 1000f).coerceAtLeast(0.001f)
    return (1f - exp(-deltaTimeS / tau)).coerceIn(0f, 1f)
}

/**
 * Map a shaped signal in [0,1] to a device output level: apply the active
 * output range [minVibe, maxVibe] and output gain. Returns 0 when silent or
 * when the signal is effectively zero, so the motor pumps back to rest between
 * hits. Mirror of Rust audio::map_output -- covered by the parity golden.
 */
internal fun mapOutput(
    shaped: Float,
    minVibe: Float,
    maxVibe: Float,
    gain: Float,
    isSilent: Boolean
): Float {
    if (isSilent || shaped <= 0.001f || !shaped.isFinite() ||
        !minVibe.isFinite() || !maxVibe.isFinite() || !gain.isFinite()) return 0f
    val ceiling = maxVibe.coerceIn(0f, 1f)
    val floor = minVibe.coerceIn(0f, ceiling)
    return ((floor + shaped * (ceiling - floor)) * gain.coerceAtLeast(0f))
        .coerceIn(0f, ceiling)
}

/**
 * Asymmetric output slew: rises fast (slewMs * 0.35, or 0.15 on large jumps)
 * and falls slower (slewMs), matching the Rust desktop output stage so the
 * haptic "pump" feel is identical on both platforms.
 */
private fun smoothOutput(
    current: Float,
    target: Float,
    deltaTimeS: Float,
    slewMs: Float
): Float {
    val jumpUp = target - current
    val upMs = if (jumpUp > 0.25f) {
        (slewMs * 0.15f).coerceIn(1f, 12f)
    } else {
        (slewMs * 0.35f).coerceAtLeast(1f)
    }
    val downMs = slewMs.coerceAtLeast(1f)
    val timeMs = if (target >= current) upMs else downMs
    val alpha = smoothingAlpha(deltaTimeS, timeMs)
    var next = (current + (target - current) * alpha).coerceIn(0f, 1f)
    // Hard snap to rest when the target is silence. Exponential slew never
    // quite reaches 0; Domi 0-20 steps + dither residual left a permanent
    // level-1 hum after the first hit. Mirrors gui.rs output stage.
    if (target <= 0.001f && next < 0.03f) {
        next = 0f
    }
    return next
}
