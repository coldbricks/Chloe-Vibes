// ==========================================================================
// AutoLockSupervisor.kt -- FIND BOOM (AUTO-LOCK): max-dynamic bass-drum tuner
// Ported from desktop auto_lock.rs for Android parity.
//
// One button that listens to the playing material and locks the sweet spot:
//   kick-punch band, gate that opens on hits / closes in the trough,
//   Hybrid trigger at near-ceiling punch, and a boom envelope whose decay
//   spans ~78% of the felt beat into a near-zero floor.
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import java.util.ArrayDeque
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sqrt

sealed class AutoLockState {
    object Idle : AutoLockState()
    data class Listening(val validAudioMs: Float, val elapsedBudgetMs: Float) : AutoLockState()
    data class Locked(val scorePercent: Int, val report: LockReport? = null) : AutoLockState()
    data class NoLock(val reason: String) : AutoLockState()
}

data class LockReport(
    val bpm: Float,
    val bandLabel: String,
    val decayMs: Float,
    val gateThreshold: Float,
    val binaryLevel: Float
)

data class LockParams(
    val frequencyMode: FrequencyMode,
    val targetFrequency: Float,
    val triggerMode: TriggerMode,
    val binaryLevel: Float,
    val hybridBlend: Float,
    val thresholdKnee: Float,
    val dynamicCurve: Float,
    val attackMs: Float,
    val decayMs: Float,
    val sustainLevel: Float,
    val releaseMs: Float,
    val attackCurve: Float,
    val decayCurve: Float,
    val releaseCurve: Float,
    val outputSlewMs: Float,
    val gateThreshold: Float,
    val gateSmoothing: Float,
    val autoGateAmount: Float,
    val climaxEnabled: Boolean
) {
    companion object {
        fun capture(p: ProcessingParams): LockParams = LockParams(
            frequencyMode = p.frequencyMode,
            targetFrequency = p.targetFrequency,
            triggerMode = p.triggerMode,
            binaryLevel = p.binaryLevel,
            hybridBlend = p.hybridBlend,
            thresholdKnee = p.thresholdKnee,
            dynamicCurve = p.dynamicCurve,
            attackMs = p.attackMs,
            decayMs = p.decayMs,
            sustainLevel = p.sustainLevel,
            releaseMs = p.releaseMs,
            attackCurve = p.attackCurve,
            decayCurve = p.decayCurve,
            releaseCurve = p.releaseCurve,
            outputSlewMs = p.outputSlewMs,
            gateThreshold = p.gateThreshold,
            gateSmoothing = p.gateSmoothing,
            autoGateAmount = p.autoGateAmount,
            climaxEnabled = p.climaxEnabled
        )

        fun lerp(a: LockParams, b: LockParams, t: Float): LockParams {
            fun l(x: Float, y: Float): Float = x + (y - x) * t
            return LockParams(
                frequencyMode = b.frequencyMode,
                triggerMode = b.triggerMode,
                targetFrequency = l(a.targetFrequency, b.targetFrequency),
                binaryLevel = l(a.binaryLevel, b.binaryLevel),
                hybridBlend = l(a.hybridBlend, b.hybridBlend),
                thresholdKnee = l(a.thresholdKnee, b.thresholdKnee),
                dynamicCurve = l(a.dynamicCurve, b.dynamicCurve),
                attackMs = l(a.attackMs, b.attackMs),
                decayMs = l(a.decayMs, b.decayMs),
                sustainLevel = l(a.sustainLevel, b.sustainLevel),
                releaseMs = l(a.releaseMs, b.releaseMs),
                attackCurve = l(a.attackCurve, b.attackCurve),
                decayCurve = l(a.decayCurve, b.decayCurve),
                releaseCurve = l(a.releaseCurve, b.releaseCurve),
                outputSlewMs = l(a.outputSlewMs, b.outputSlewMs),
                gateThreshold = l(a.gateThreshold, b.gateThreshold),
                gateSmoothing = l(a.gateSmoothing, b.gateSmoothing),
                autoGateAmount = b.autoGateAmount,
                climaxEnabled = b.climaxEnabled
            )
        }
    }

    fun applyTo(p: ProcessingParams): ProcessingParams = p.copy(
        frequencyMode = frequencyMode,
        targetFrequency = targetFrequency,
        triggerMode = triggerMode,
        binaryLevel = binaryLevel,
        hybridBlend = hybridBlend,
        thresholdKnee = thresholdKnee,
        dynamicCurve = dynamicCurve,
        attackMs = attackMs,
        decayMs = decayMs,
        sustainLevel = sustainLevel,
        releaseMs = releaseMs,
        attackCurve = attackCurve,
        decayCurve = decayCurve,
        releaseCurve = releaseCurve,
        outputSlewMs = outputSlewMs,
        gateThreshold = gateThreshold,
        gateSmoothing = gateSmoothing,
        autoGateAmount = autoGateAmount,
        climaxEnabled = climaxEnabled
    )

    fun hasDiverged(p: ProcessingParams, eps: Float = 1e-3f): Boolean {
        return frequencyMode != p.frequencyMode ||
            triggerMode != p.triggerMode ||
            abs(targetFrequency - p.targetFrequency) > eps ||
            abs(binaryLevel - p.binaryLevel) > eps ||
            abs(hybridBlend - p.hybridBlend) > eps ||
            abs(thresholdKnee - p.thresholdKnee) > eps ||
            abs(dynamicCurve - p.dynamicCurve) > eps ||
            abs(attackMs - p.attackMs) > eps ||
            abs(decayMs - p.decayMs) > eps ||
            abs(sustainLevel - p.sustainLevel) > eps ||
            abs(releaseMs - p.releaseMs) > eps ||
            abs(attackCurve - p.attackCurve) > eps ||
            abs(decayCurve - p.decayCurve) > eps ||
            abs(releaseCurve - p.releaseCurve) > eps ||
            abs(outputSlewMs - p.outputSlewMs) > eps ||
            abs(gateThreshold - p.gateThreshold) > eps ||
            abs(gateSmoothing - p.gateSmoothing) > eps ||
            abs(autoGateAmount - p.autoGateAmount) > eps ||
            climaxEnabled != p.climaxEnabled
    }
}

internal data class FrameSample(
    val tMs: Float,
    val bandEnergies: FloatArray,
    val centroid: Float,
    val preVolumeEnergy: Float,
    val valid: Boolean
)

internal data class Glide(
    val startedMs: Float,
    val from: LockParams,
    val to: LockParams
)

internal data class Features(
    val ioiMedian: Float,
    val ioiIqr: Float,
    val bestBand: Int,
    val salienceMargin: Float,
    val crest: Float,
    val centroidMedian: Float,
    val silenceRatio: Float,
    val energyHit: Float,
    val energyFloor: Float
)

class AutoLockSupervisor {

    companion object {
        const val N_BANDS: Int = 8
        val BAND_EDGES = floatArrayOf(
            20f, 60f, 250f, 500f, 2000f, 4000f, 6000f, 12000f, 20000f
        )
        const val RING_WINDOW_MS: Float = 8000f
        const val LISTEN_BUDGET_MS: Float = 15000f
        const val LISTEN_MIN_VALID_MS: Float = 4000f
        const val GLIDE_MS: Float = 1500f
        const val SALIENCE_MARGIN: Float = 1.3f
        const val MIN_LOCK_SCORE: Float = 0.45f
        const val ONSET_MERGE_MS: Float = 120f
        const val ONSET_ALIGN_MS: Float = 100f
        const val VALID_ENERGY: Float = 0.002f

        fun bandLabel(band: Int): String = when (band) {
            0 -> "Sub"
            1 -> "Bass"
            2 -> "Lo-Mid"
            3 -> "Mid"
            4 -> "Hi-Mid"
            5 -> "Pres"
            6 -> "Brill"
            else -> "Air"
        }

        fun percentileSorted(sorted: FloatArray, p: Float): Float {
            if (sorted.isEmpty()) return 0f
            val idx = ((sorted.size - 1) * p).roundToInt()
            return sorted[idx.coerceIn(0, sorted.size - 1)]
        }
    }

    var state: AutoLockState = AutoLockState.Idle
        private set

    var snapshot: LockParams? = null
        private set
    var lastReport: LockReport? = null
        private set

    private var listenFromMs: Float = 0f
    private val frames = ArrayDeque<FrameSample>()
    private val onsets = ArrayDeque<Float>()
    private var glide: Glide? = null
    private var expected: LockParams? = null

    fun isListening(): Boolean = state is AutoLockState.Listening
    fun isLocked(): Boolean = state is AutoLockState.Locked
    fun canRevert(): Boolean = snapshot != null

    fun buttonLabel(nowMs: Float): String {
        return when (val s = state) {
            is AutoLockState.Idle -> "FIND BOOM"
            is AutoLockState.Listening -> {
                val sec = ((nowMs - listenFromMs) / 1000f).coerceAtLeast(0f)
                "TUNING %.0fs…".format(sec)
            }
            is AutoLockState.Locked -> "BOOM ${s.scorePercent}%"
            is AutoLockState.NoLock -> "NO LOCK — ${s.reason}"
        }
    }

    fun reportLine(): String? {
        val r = lastReport ?: return null
        val punchPct = (r.binaryLevel * 100f).coerceIn(0f, 100f).roundToInt()
        return "%.0f BPM · %s · decay %.0fms · gate %.2f · punch %d%%".format(
            r.bpm, r.bandLabel, r.decayMs, r.gateThreshold, punchPct
        )
    }

    fun onButton(nowMs: Float) {
        when (state) {
            is AutoLockState.Idle, is AutoLockState.NoLock -> startListening(nowMs)
            is AutoLockState.Listening -> cancel()
            is AutoLockState.Locked -> startListening(nowMs)
        }
    }

    fun startListening(nowMs: Float) {
        listenFromMs = nowMs
        state = AutoLockState.Listening(validAudioMs = 0f, elapsedBudgetMs = 0f)
    }

    fun cancel() {
        glide = null
        expected = null
        state = AutoLockState.Idle
    }

    fun revert(currentParams: ProcessingParams): ProcessingParams {
        glide = null
        expected = null
        state = AutoLockState.Idle
        val snap = snapshot ?: return currentParams
        snapshot = null
        return snap.applyTo(currentParams)
    }

    fun keep() {
        glide = null
        expected = null
        snapshot = null
        state = AutoLockState.Idle
    }

    fun pushFrame(
        currentTimeMs: Float,
        spectral: SpectralData,
        preVolumeEnergy: Float,
        isOnset: Boolean
    ) {
        val valid = preVolumeEnergy >= VALID_ENERGY
        val sample = FrameSample(
            tMs = currentTimeMs,
            bandEnergies = spectral.bandEnergies.copyOf(),
            centroid = spectral.spectralCentroid,
            preVolumeEnergy = preVolumeEnergy,
            valid = valid
        )
        frames.addLast(sample)

        if (isOnset && valid) {
            val lastOnset = onsets.peekLast()
            if (lastOnset == null || currentTimeMs - lastOnset >= ONSET_MERGE_MS) {
                onsets.addLast(currentTimeMs)
            }
        }

        val cutoff = currentTimeMs - RING_WINDOW_MS
        while (frames.isNotEmpty() && frames.first.tMs < cutoff) {
            frames.removeFirst()
        }
        while (onsets.isNotEmpty() && onsets.first < cutoff) {
            onsets.removeFirst()
        }
    }

    fun update(
        nowMs: Float,
        engineConfidence: Float,
        currentParams: ProcessingParams
    ): ProcessingParams {
        var params = currentParams

        // Manual edit detection: if user edited knobs, dissolve lock
        expected?.let { exp ->
            if (exp.hasDiverged(params)) {
                glide = null
                expected = null
                snapshot = null
                state = AutoLockState.Idle
            }
        }

        // Advance glide
        glide?.let { g ->
            val p = ((nowMs - g.startedMs) / GLIDE_MS).coerceIn(0f, 1f)
            val eased = p * p * (3f - 2f * p)
            val current = LockParams.lerp(g.from, g.to, eased)
            params = current.applyTo(params)
            expected = LockParams.capture(params)
            if (p >= 1f) {
                glide = null
            }
        }

        val listening = state as? AutoLockState.Listening
        if (listening != null) {
            val validMs = calculateValidWindowMs(listenFromMs)
            val elapsedBudget = max(0f, nowMs - listenFromMs)
            state = AutoLockState.Listening(validMs, elapsedBudget)

            val canTryEarly = validMs >= LISTEN_MIN_VALID_MS
            val budgetExhausted = elapsedBudget >= LISTEN_BUDGET_MS

            if (canTryEarly || budgetExhausted) {
                params = tryCommit(nowMs, engineConfidence, params)
            }
        }

        return params
    }

    private fun calculateValidWindowMs(fromMs: Float): Float {
        var total = 0f
        val frameList = frames.toList()
        for (i in 1 until frameList.size) {
            val prev = frameList[i - 1]
            val curr = frameList[i]
            if (curr.tMs < fromMs) continue
            val span = curr.tMs - max(prev.tMs, fromMs)
            if (prev.valid && curr.valid && span in 0f..100f) {
                total += span
            }
        }
        return total
    }

    private fun tryCommit(
        nowMs: Float,
        engineConfidence: Float,
        params: ProcessingParams
    ): ProcessingParams {
        val features = estimate(listenFromMs)
        if (features == null) {
            val elapsedBudget = max(0f, nowMs - listenFromMs)
            if (elapsedBudget < LISTEN_BUDGET_MS) return params
            state = AutoLockState.NoLock("not enough audio")
            return params
        }

        val ownConf = if (features.ioiMedian > 0f) {
            (1f - 4f * (features.ioiIqr / features.ioiMedian)).coerceIn(0f, 1f)
        } else {
            0f
        }
        val conf = max(engineConfidence, ownConf)
        val marginNorm = ((features.salienceMargin - 1f) / 1.5f).coerceIn(0f, 1f)
        val crestNorm = ((features.crest - 1f) / 5f).coerceIn(0f, 1f)
        val score = 0.45f * conf +
            0.25f * marginNorm +
            0.15f * (1f - features.silenceRatio) +
            0.15f * crestNorm

        if (conf < 0.35f || score < MIN_LOCK_SCORE) {
            val elapsedBudget = max(0f, nowMs - listenFromMs)
            if (elapsedBudget < LISTEN_BUDGET_MS) return params
            state = AutoLockState.NoLock("no steady rhythm")
            return params
        }

        val target = mapFeatures(features, params)
        if (snapshot == null) {
            snapshot = LockParams.capture(params)
        }

        val from = LockParams.capture(params)
        glide = Glide(startedMs = nowMs, from = from, to = target)

        // Enums switch immediately; auto-gate zeroed; climax disabled during boom; floats glide
        val committed = params.copy(
            frequencyMode = target.frequencyMode,
            triggerMode = target.triggerMode,
            autoGateAmount = target.autoGateAmount,
            climaxEnabled = false
        )
        expected = LockParams.capture(committed)

        val beatMs = BeatDetector.foldToPerceptualBeat(features.ioiMedian)
        val bpm = if (beatMs > 1f) 60000f / beatMs else 0f
        lastReport = LockReport(
            bpm = bpm,
            bandLabel = bandLabel(features.bestBand),
            decayMs = target.decayMs,
            gateThreshold = target.gateThreshold,
            binaryLevel = target.binaryLevel
        )
        state = AutoLockState.Locked(
            scorePercent = (score * 100f).roundToInt(),
            report = lastReport
        )
        return committed
    }

    internal fun estimate(fromMs: Float): Features? {
        val activeFrames = frames.filter { it.tMs >= fromMs }
        if (activeFrames.size < 40) return null

        val silent = activeFrames.count { !it.valid }
        val silenceRatio = silent.toFloat() / activeFrames.size.toFloat()

        val activeOnsets = onsets.filter { it >= fromMs }
        if (activeOnsets.size < 8) return null

        val iois = mutableListOf<Float>()
        for (i in 1 until activeOnsets.size) {
            val diff = activeOnsets[i] - activeOnsets[i - 1]
            if (diff in 150f..2000f) {
                iois.add(diff)
            }
        }
        if (iois.size < 5) return null
        iois.sort()
        val sortedIois = iois.toFloatArray()
        val ioiMedian = percentileSorted(sortedIois, 0.5f)
        val ioiIqr = percentileSorted(sortedIois, 0.75f) - percentileSorted(sortedIois, 0.25f)

        val deltaRows = ArrayList<FloatArray>(activeFrames.size)
        val alignedRows = ArrayList<Boolean>(activeFrames.size)
        for (i in 1 until activeFrames.size) {
            val prev = activeFrames[i - 1]
            val curr = activeFrames[i]
            val row = FloatArray(N_BANDS)
            for (b in 0 until N_BANDS) {
                row[b] = max(0f, curr.bandEnergies[b] - prev.bandEnergies[b])
            }
            deltaRows.add(row)
            val isAligned = activeOnsets.any { o -> curr.tMs >= o && curr.tMs - o <= ONSET_ALIGN_MS }
            alignedRows.add(isAligned)
        }

        val punch = FloatArray(N_BANDS)
        for (b in 0 until N_BANDS) {
            val hitPeaks = mutableListOf<Float>()
            for (o in activeOnsets) {
                var peak = 0f
                var inWindow = false
                for (i in 1 until activeFrames.size) {
                    val t = activeFrames[i].tMs
                    if (t >= o && t - o <= ONSET_ALIGN_MS) {
                        inWindow = true
                        peak = max(peak, deltaRows[i - 1][b])
                    } else if (t > o + ONSET_ALIGN_MS) {
                        break
                    }
                }
                if (inWindow) hitPeaks.add(peak)
            }
            if (hitPeaks.isEmpty()) continue
            hitPeaks.sort()
            val sortedPeaks = hitPeaks.toFloatArray()
            val hitHeight = percentileSorted(sortedPeaks, 0.5f)
            val reliability = hitPeaks.count { it > 1e-4f }.toFloat() / hitPeaks.size.coerceAtLeast(1).toFloat()

            var floorSum = 0f
            var floorN = 0
            for (i in 0 until deltaRows.size) {
                if (!alignedRows[i]) {
                    floorSum += deltaRows[i][b]
                    floorN++
                }
            }
            val floor = floorSum / max(1, floorN).toFloat()
            val contrast = hitHeight / (hitHeight + 3f * floor + 1e-6f)
            punch[b] = hitHeight * reliability * contrast
        }

        val order = (0 until N_BANDS).sortedByDescending { punch[it] }
        val bestBand = order[0]
        val salienceMargin = if (punch[order[1]] > 1e-6f) punch[bestBand] / punch[order[1]] else Float.POSITIVE_INFINITY

        val energies = activeFrames.filter { it.valid }.map { it.preVolumeEnergy }.sorted().toFloatArray()
        if (energies.isEmpty()) return null
        val crest = percentileSorted(energies, 0.95f) / max(1e-4f, percentileSorted(energies, 0.50f))

        val centroids = activeFrames.filter { it.valid }.map { it.centroid }.sorted().toFloatArray()
        val centroidMedian = percentileSorted(centroids, 0.5f)

        val (gateMode, gateFrequency) = frequencyFocus(bestBand, salienceMargin, crest, 0f)
        val gateEnergies = mutableListOf<Float>()
        val hitE = mutableListOf<Float>()
        val floorE = mutableListOf<Float>()
        for (f in activeFrames) {
            if (!f.valid) continue
            val onHit = activeOnsets.any { o -> f.tMs >= o && f.tMs - o <= ONSET_ALIGN_MS }
            val spectral = SpectralData(bandEnergies = f.bandEnergies)
            val targetEnergy = SpectralAnalyzer.extractEnergy(spectral, gateMode, gateFrequency)
            val gateEnergy = (targetEnergy * 6f).coerceIn(0f, 1f).pow(0.65f)
            gateEnergies.add(gateEnergy)
            if (onHit) hitE.add(gateEnergy) else floorE.add(gateEnergy)
        }
        gateEnergies.sort()
        hitE.sort()
        floorE.sort()

        val energyHit = if (hitE.isEmpty()) {
            percentileSorted(gateEnergies.toFloatArray(), 0.90f)
        } else {
            percentileSorted(hitE.toFloatArray(), 0.50f)
        }
        val energyFloor = if (floorE.isEmpty()) {
            percentileSorted(gateEnergies.toFloatArray(), 0.20f)
        } else {
            percentileSorted(floorE.toFloatArray(), 0.50f)
        }

        return Features(
            ioiMedian = ioiMedian,
            ioiIqr = ioiIqr,
            bestBand = bestBand,
            salienceMargin = salienceMargin,
            crest = crest,
            centroidMedian = centroidMedian,
            silenceRatio = silenceRatio,
            energyHit = energyHit,
            energyFloor = energyFloor
        )
    }

    private fun frequencyFocus(
        bestBand: Int,
        salienceMargin: Float,
        crest: Float,
        previousTarget: Float
    ): Pair<FrequencyMode, Float> {
        return if (salienceMargin >= SALIENCE_MARGIN) {
            when (bestBand) {
                0 -> Pair(FrequencyMode.LowPass, BAND_EDGES[1])
                1 -> Pair(FrequencyMode.LowPass, BAND_EDGES[2])
                2, 3 -> Pair(FrequencyMode.BandPass, sqrt(BAND_EDGES[bestBand] * BAND_EDGES[bestBand + 1]))
                else -> Pair(FrequencyMode.HighPass, BAND_EDGES[bestBand])
            }
        } else if (crest >= 2f || bestBand <= 1) {
            Pair(FrequencyMode.LowPass, BAND_EDGES[2])
        } else {
            Pair(FrequencyMode.Full, previousTarget)
        }
    }

    internal fun mapFeatures(f: Features, params: ProcessingParams): LockParams {
        val t = BeatDetector.foldToPerceptualBeat(f.ioiMedian)
        val (frequencyMode, targetFrequency) = frequencyFocus(
            f.bestBand,
            f.salienceMargin,
            f.crest,
            params.targetFrequency
        )

        val kickBand = f.bestBand <= 1
        val (triggerMode, hybridBlend, dynamicCurve) = if (kickBand || f.crest >= 2f) {
            val blend = if (f.crest >= 4f) 0.72f else if (f.crest >= 2.5f) 0.62f else 0.55f
            Triple(TriggerMode.Hybrid, blend, 1.15f)
        } else if (f.crest >= 1.5f) {
            Triple(TriggerMode.Hybrid, 0.45f, 1.35f)
        } else {
            Triple(TriggerMode.Dynamic, params.hybridBlend, 1.7f)
        }

        val crestN = ((f.crest - 1f) / 5f).coerceIn(0f, 1f)
        val bandBoost = if (kickBand) 0.06f else 0f
        val binaryLevel = (0.72f + 0.12f * crestN + bandBoost).coerceIn(0.70f, 0.88f)

        val decayMs = (0.78f * t).coerceIn(80f, 1600f)
        val sustainTarget = 0.08f
        val releaseTarget = (0.50f * t).coerceIn(80f, 1200f)

        val cn = ((f.centroidMedian - 100f) / 4000f).coerceIn(0f, 1f)
        val sustainLevel = (sustainTarget / (1f - 0.25f * cn)).coerceIn(0f, 1f)
        val releaseMs = (releaseTarget / (1f + 0.4f * (1f - cn))).coerceIn(0.5f, 5000f)

        val span = max(0f, f.energyHit - f.energyFloor)
        var gateThreshold = f.energyFloor + span * 0.22f
        val cap = max(0.02f, f.energyHit * 0.55f)
        gateThreshold = gateThreshold.coerceIn(0.02f, cap).coerceIn(0.02f, 0.45f)
        val gateSmoothing = if (f.crest >= 2.5f) 0.04f else 0.08f
        val outputSlewMs = (0.10f * t).coerceIn(30f, 55f)

        return LockParams(
            frequencyMode = frequencyMode,
            targetFrequency = targetFrequency,
            triggerMode = triggerMode,
            binaryLevel = binaryLevel,
            hybridBlend = hybridBlend,
            thresholdKnee = 0.15f,
            dynamicCurve = dynamicCurve,
            attackMs = 20f,
            decayMs = decayMs,
            sustainLevel = sustainLevel,
            releaseMs = releaseMs,
            attackCurve = 0.7f,
            decayCurve = 1.8f,
            releaseCurve = 1.3f,
            outputSlewMs = outputSlewMs,
            gateThreshold = gateThreshold,
            gateSmoothing = gateSmoothing,
            autoGateAmount = 0f,
            climaxEnabled = false
        )
    }
}
