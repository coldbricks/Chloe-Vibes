// ==========================================================================
// ParityTest.kt -- Golden-file parity test for the Kotlin audio port
//
// Runs the complete Kotlin signal chain over the same deterministic
// synthetic PCM that the Rust parity test (tests/parity.rs) uses, then
// reads the Rust-generated golden CSV (tests/parity_golden.csv at the
// repo root) and asserts that every per-frame envelope + climax pair
// matches within epsilon 1e-3.
//
// Epsilon is slightly looser than the Rust self-check (1e-4) to tolerate
// differences in floating-point op ordering between the Rust rustfft
// library and the hand-rolled Kotlin radix-2 FFT.
//
// Uses plain JUnit 5 -- no Android framework dependencies. Run via
// `./gradlew :app:test` from the android/ directory.
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.fail
import org.junit.jupiter.api.Test
import java.io.File
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.floor
import kotlin.math.sin

class ParityTest {

    // -----------------------------------------------------------------------
    // Constants -- must match tests/parity.rs byte-for-byte.
    // -----------------------------------------------------------------------

    private val sampleRate: Float = 48_000f
    private val durationSec: Float = 10f
    private val totalSamples: Int = (sampleRate.toInt()) * (durationSec.toInt())
    private val frameSize: Int = 1024
    private val lcgSeed: Int = 0x51ED5EED.toInt()

    // Test preset (must match Rust TestPreset exactly)
    private val gateThreshold = 0.02f
    private val gateAutoAmount = 0f
    private val gateSmoothing = 0f

    private val triggerMode = TriggerMode.Dynamic
    private val threshold = 0.02f
    private val thresholdKnee = 0f
    private val dynamicCurve = 1f
    private val binaryLevel = 1f
    private val hybridBlend = 0.5f
    private val attackMs = 20f
    private val decayMs = 120f
    private val sustainLevel = 0.5f
    private val releaseMs = 180f
    private val attackCurve = 1f
    private val decayCurve = 1f
    private val releaseCurve = 1f

    private val climaxEnabled = true
    private val climaxIntensity = 0.5f
    private val climaxBuildUpMs = 8_000f
    private val climaxTeaseRatio = 0.25f
    private val climaxTeaseDrop = 0.4f
    private val climaxSurgeBoost = 0.6f
    private val climaxPulseDepth = 0.2f
    private val climaxPattern = ClimaxPattern.Wave

    private val freqMode = FrequencyMode.Full
    private val freqTarget = 1000f

    // -----------------------------------------------------------------------
    // Deterministic synthetic PCM -- formula must match Rust exactly.
    // -----------------------------------------------------------------------

    /**
     * 32-bit LCG using Numerical Recipes constants. Operates on an Int
     * holding a uint32 bit pattern; returns the next state and the noise
     * value. Unsigned-shift-right (ushr 8) and a 2^24-normalized float
     * mirror the Rust implementation bit-for-bit.
     */
    private fun lcgStep(state: Int): Pair<Int, Float> {
        val next = state * 1_664_525 + 1_013_904_223
        val unit = (next ushr 8).toFloat() / (1 shl 24).toFloat()
        val noise = (unit - 0.5f) * 0.04f
        return next to noise
    }

    /** 50ms exponential decay, retriggered at 2 Hz (0.6 peak, 10ms tau). */
    private fun drumHit(t: Float): Float {
        val period = 0.5f
        val trigger = floor(t / period) * period
        val u = t - trigger
        return if (u < 0.05f) 0.6f * exp(-u / 0.010f) else 0f
    }

    /** 50ms raised-cosine fade-in/out envelope. */
    private fun hannEnvelope(t: Float, duration: Float): Float {
        val fade = 0.050f
        return when {
            t < fade -> 0.5f * (1f - cos(PI.toFloat() * t / fade))
            t > duration - fade -> {
                val u = (duration - t) / fade
                0.5f * (1f - cos(PI.toFloat() * u))
            }
            else -> 1f
        }
    }

    private fun generatePcm(): FloatArray {
        val pcm = FloatArray(totalSamples)
        var state = lcgSeed
        val tau = 2f * PI.toFloat()
        for (i in 0 until totalSamples) {
            val (next, noise) = lcgStep(state)
            state = next
            val t = i.toFloat() / sampleRate
            val sig = 0.05f +
                    0.50f * sin(tau * 100f * t) +
                    0.30f * sin(tau * 1000f * t) +
                    drumHit(t) +
                    noise
            pcm[i] = sig * hannEnvelope(t, durationSec)
        }
        return pcm
    }

    // -----------------------------------------------------------------------
    // Signal chain -- same shape as tests/parity.rs run_chain().
    // -----------------------------------------------------------------------

    private data class FrameResult(val envelope: Float, val climax: Float)

    private fun runChain(pcm: FloatArray): List<FrameResult> {
        val analyzer = SpectralAnalyzer(sampleRate)
        val gate = Gate()
        val beat = BeatDetector()
        val env = EnvelopeProcessor()
        val climax = ClimaxEngine()

        val numFrames = pcm.size / frameSize
        val out = ArrayList<FrameResult>(numFrames)

        for (frameIdx in 0 until numFrames) {
            val start = frameIdx * frameSize
            val chunk = pcm.copyOfRange(start, start + frameSize)

            val currentTimeMs = frameIdx.toFloat() * frameSize.toFloat() * 1000f / sampleRate

            // 1) Spectral analysis (mono)
            val spectral = analyzer.analyze(chunk, channels = 1)

            // 2) Extract energy
            val energy = SpectralAnalyzer.extractEnergy(spectral, freqMode, freqTarget)

            // 3) Gate
            val gateOpen = gate.process(energy, gateThreshold, gateAutoAmount, gateSmoothing)

            // 4) Beat detector
            val (isOnset, onsetStrength) = beat.process(spectral.spectralFlux, currentTimeMs)

            // 5) Envelope
            val envOut = env.drive(
                gateOpen = gateOpen,
                energy = energy,
                isOnset = isOnset,
                onsetStrength = onsetStrength,
                currentTimeMs = currentTimeMs,
                triggerMode = triggerMode,
                threshold = threshold,
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
                releaseCurve = releaseCurve,
                spectralCentroid = spectral.spectralCentroid
            )

            // 6) Climax
            val climaxOut = climax.process(
                input = envOut,
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

            out.add(FrameResult(envOut, climaxOut))
        }

        return out
    }

    // -----------------------------------------------------------------------
    // Golden CSV location
    //
    // Gradle's working directory for tests is `android/app`. The golden
    // file is committed at `<repo>/tests/parity_golden.csv`, so we walk
    // up two dirs (../../tests/...). The `chloevibes.goldenCsv` system
    // property can override this in CI or IDE configs.
    // -----------------------------------------------------------------------

    private fun goldenFile(): File {
        System.getProperty("chloevibes.goldenCsv")?.let { return File(it) }
        val candidates = listOf(
            File("../../tests/parity_golden.csv"), // android/app working dir
            File("../tests/parity_golden.csv"),    // android working dir
            File("tests/parity_golden.csv")        // repo root working dir
        )
        for (c in candidates) {
            if (c.isFile) return c.absoluteFile
        }
        return candidates.first().absoluteFile
    }

    private data class GoldenRow(val frame: Int, val envelope: Float, val climax: Float)

    private fun parseGolden(file: File): List<GoldenRow> {
        val rows = ArrayList<GoldenRow>()
        file.useLines { seq ->
            seq.forEachIndexed { i, line ->
                if (i == 0) return@forEachIndexed // header
                if (line.isBlank()) return@forEachIndexed
                val parts = line.split(',')
                rows.add(
                    GoldenRow(
                        frame = parts[0].trim().toInt(),
                        envelope = parts[1].trim().toFloat(),
                        climax = parts[2].trim().toFloat()
                    )
                )
            }
        }
        return rows
    }

    // -----------------------------------------------------------------------
    // The test
    // -----------------------------------------------------------------------

    @Test
    fun parityKotlinMatchesRustGolden() {
        val pcm = generatePcm()
        assertEquals(totalSamples, pcm.size, "PCM length mismatch")

        val frames = runChain(pcm)

        val golden = goldenFile()
        if (!golden.isFile) {
            fail<Unit>(
                "Golden file not found at ${golden.absolutePath}. " +
                        "Run `cargo test --test parity` from the repo root first to generate it."
            )
        }
        val goldenRows = parseGolden(golden)
        assertEquals(
            goldenRows.size, frames.size,
            "frame count differs: golden=${goldenRows.size} kotlin=${frames.size}"
        )

        val epsilon = 1e-3f
        var maxEnvDiff = 0f
        var maxClimaxDiff = 0f
        var worstFrame = 0
        val mismatches = ArrayList<String>()

        for (i in frames.indices) {
            val k = frames[i]
            val g = goldenRows[i]
            assertEquals(i, g.frame, "frame index mismatch at row $i")
            val de = abs(k.envelope - g.envelope)
            val dc = abs(k.climax - g.climax)
            if (de > maxEnvDiff) maxEnvDiff = de
            if (dc > maxClimaxDiff) maxClimaxDiff = dc
            if (de > epsilon || dc > epsilon) {
                if (mismatches.size < 8) {
                    mismatches.add(
                        "frame $i: env ${k.envelope} vs ${g.envelope} " +
                                "(Δ=${"%.3e".format(de)}), climax ${k.climax} vs ${g.climax} " +
                                "(Δ=${"%.3e".format(dc)})"
                    )
                }
                worstFrame = i
            }
        }

        if (mismatches.isNotEmpty()) {
            fail<Unit>(
                "Kotlin parity regression vs Rust golden (epsilon=$epsilon):\n" +
                        "  worst frame: $worstFrame\n" +
                        "  max env diff: ${"%.3e".format(maxEnvDiff)}\n" +
                        "  max climax diff: ${"%.3e".format(maxClimaxDiff)}\n" +
                        "  first mismatches:\n    " + mismatches.joinToString("\n    ") + "\n" +
                        "  (Kotlin and Rust signal chains have drifted. " +
                        "Check recent edits to audio.rs or files under " +
                        "android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/.)"
            )
        }
    }
}
