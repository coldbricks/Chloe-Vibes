// ==========================================================================
// SpectralAnalyzer.kt -- FFT-based frequency analysis
// Ported from audio.rs SpectralAnalyzer
//
// Implements a radix-2 Cooley-Tukey FFT with Hann windowing, then extracts
// perceptual band energies, RMS power, spectral centroid, spectral flux,
// and dominant frequency. Pure Kotlin -- no Android dependencies.
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import kotlin.math.*

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** FFT window size. 2048 at 48kHz = ~42ms windows, ~23Hz bin resolution. */
const val FFT_SIZE = 2048

/** Number of perceptual frequency bands. */
const val NUM_BANDS = 8

/** Frequency edges for 8 bands (Hz): Sub | Bass | Lo-Mid | Mid | Hi-Mid | Presence | Brilliance | Air */
val BAND_EDGES = floatArrayOf(20f, 60f, 250f, 500f, 2000f, 4000f, 6000f, 12000f, 20000f)

/** Labels for the bands (UI display). */
val BAND_NAMES = arrayOf("Sub", "Bass", "Lo-Mid", "Mid", "Hi-Mid", "Pres", "Brill", "Air")

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/** How audio energy is converted to trigger magnitude. */
enum class TriggerMode {
    /** Intensity scales continuously with audio energy above threshold. */
    Dynamic,
    /** Fixed output level when energy exceeds threshold, zero otherwise. */
    Binary,
    /** Blend between dynamic and binary. Adjustable via hybridBlend. */
    Hybrid
}

/** Which part of the frequency spectrum to analyze. */
enum class FrequencyMode {
    /** Full audible spectrum (weighted toward lower freqs). */
    Full,
    /** Only frequencies below targetFrequency. */
    LowPass,
    /** Only frequencies above targetFrequency. */
    HighPass,
    /** Narrow band around targetFrequency. */
    BandPass
}

/** High-level modulation pattern for the climax engine. */
enum class ClimaxPattern {
    /** Smooth continuous rise toward end of cycle. */
    Wave,
    /** Step-like intensity increases (plateaus then jumps). */
    Stairs,
    /** Aggressive exponential ramp in the final third. */
    Surge
}

// ---------------------------------------------------------------------------
// SpectralData -- analysis output
// ---------------------------------------------------------------------------

data class SpectralData(
    /** Energy in each of the 8 frequency bands (0.0 - ~1.0). */
    val bandEnergies: FloatArray = FloatArray(NUM_BANDS),
    /** Overall RMS power of the audio signal. */
    val rmsPower: Float = 0f,
    /** Spectral centroid in Hz (higher = brighter sound). */
    val spectralCentroid: Float = 0f,
    /** Spectral flux -- how much the spectrum changed since last frame. */
    val spectralFlux: Float = 0f,
    /** Frequency of the loudest bin. */
    val dominantFrequency: Float = 0f
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SpectralData) return false
        return bandEnergies.contentEquals(other.bandEnergies) &&
                rmsPower == other.rmsPower &&
                spectralCentroid == other.spectralCentroid &&
                spectralFlux == other.spectralFlux &&
                dominantFrequency == other.dominantFrequency
    }

    override fun hashCode(): Int {
        var result = bandEnergies.contentHashCode()
        result = 31 * result + rmsPower.hashCode()
        result = 31 * result + spectralCentroid.hashCode()
        result = 31 * result + spectralFlux.hashCode()
        result = 31 * result + dominantFrequency.hashCode()
        return result
    }
}

// ---------------------------------------------------------------------------
// SpectralAnalyzer
// ---------------------------------------------------------------------------

/**
 * Performs FFT on audio samples and extracts perceptual features.
 * Runs on the processing thread.
 */
class SpectralAnalyzer(private val sampleRate: Float = 48000f) {

    /** Hann window function -- reduces spectral leakage. */
    private val window = FloatArray(FFT_SIZE) { i ->
        0.5f * (1f - cos(2f * PI.toFloat() * i / (FFT_SIZE - 1)))
    }

    /** Hz per FFT bin. */
    private val binResolution = sampleRate / FFT_SIZE

    /** Pre-calculated bin index ranges for each frequency band. */
    private val bandBinRanges = Array(NUM_BANDS) { i ->
        val start = (BAND_EDGES[i] / binResolution).roundToInt().coerceAtMost(FFT_SIZE / 2)
        val end = (BAND_EDGES[i + 1] / binResolution).roundToInt().coerceAtMost(FFT_SIZE / 2)
        start to end
    }

    /** Previous frame magnitude spectrum (for flux calculation). */
    private val prevMagnitude = FloatArray(FFT_SIZE / 2)

    /** Reusable FFT real/imag buffers. */
    private val fftReal = FloatArray(FFT_SIZE)
    private val fftImag = FloatArray(FFT_SIZE)

    /** Reusable magnitude buffer. */
    private val magnitudes = FloatArray(FFT_SIZE / 2)

    /**
     * Analyze a buffer of interleaved audio samples.
     * Returns a SpectralData with all extracted features.
     */
    fun analyze(samples: FloatArray, channels: Int = 1): SpectralData {
        // Step 1: Mix to mono
        val monoLen = samples.size / channels.coerceAtLeast(1)
        val mono = FloatArray(monoLen)
        val ch = channels.coerceAtLeast(1)
        for (i in 0 until monoLen) {
            var sum = 0f
            for (c in 0 until ch) {
                val idx = i * ch + c
                if (idx < samples.size) sum += samples[idx]
            }
            mono[i] = sum / ch
        }

        // Step 2: Take last FFT_SIZE samples (or zero-pad)
        val start = if (monoLen > FFT_SIZE) monoLen - FFT_SIZE else 0
        val available = mono.copyOfRange(start, monoLen)

        // Step 3: Apply Hann window and fill FFT buffers
        for (i in 0 until FFT_SIZE) {
            val sample = if (i < available.size) available[i] else 0f
            fftReal[i] = sample * window[i]
            fftImag[i] = 0f
        }

        // Step 4: Run in-place radix-2 Cooley-Tukey FFT
        fft(fftReal, fftImag)

        // Step 5: Compute magnitude spectrum
        val half = FFT_SIZE / 2
        val scale = 2f / FFT_SIZE
        for (i in 0 until half) {
            val re = fftReal[i]
            val im = fftImag[i]
            magnitudes[i] = sqrt(re * re + im * im) * scale
        }

        // Step 6: Calculate band energies
        val bandEnergies = FloatArray(NUM_BANDS)
        for (i in 0 until NUM_BANDS) {
            val (binStart, binEnd) = bandBinRanges[i]
            if (binEnd > binStart && binEnd <= magnitudes.size) {
                var energy = 0f
                for (b in binStart until binEnd) {
                    energy += magnitudes[b] * magnitudes[b]
                }
                val count = binEnd - binStart
                bandEnergies[i] = sqrt(energy / count)
            }
        }

        // Step 7: RMS power (time domain)
        var rmsSum = 0f
        for (s in available) rmsSum += s * s
        val rmsPower = if (available.isNotEmpty()) sqrt(rmsSum / available.size) else 0f

        // Step 8: Spectral centroid
        var weightedSum = 0f
        var totalMag = 0f
        for (i in 0 until half) {
            val freq = i * binResolution
            weightedSum += freq * magnitudes[i]
            totalMag += magnitudes[i]
        }
        val spectralCentroid = if (totalMag > 1e-10f) weightedSum / totalMag else 0f

        // Step 9: Spectral flux (half-wave rectified)
        var spectralFlux = 0f
        for (i in 0 until half) {
            val diff = magnitudes[i] - prevMagnitude[i]
            if (diff > 0f) spectralFlux += diff
        }

        // Step 10: Dominant frequency
        var maxMag = 0f
        var dominantBin = 0
        for (i in 0 until half) {
            if (magnitudes[i] > maxMag) {
                maxMag = magnitudes[i]
                dominantBin = i
            }
        }
        val dominantFrequency = dominantBin * binResolution

        // Save magnitude for next frame flux
        magnitudes.copyInto(prevMagnitude)

        return SpectralData(
            bandEnergies = bandEnergies,
            rmsPower = rmsPower,
            spectralCentroid = spectralCentroid,
            spectralFlux = spectralFlux,
            dominantFrequency = dominantFrequency
        )
    }

    companion object {
        /**
         * Extract energy from specific frequency range based on mode.
         * Called by the processing thread using stored SpectralData.
         */
        fun extractEnergy(data: SpectralData, mode: FrequencyMode, targetFreq: Float): Float {
            return when (mode) {
                FrequencyMode.Full -> {
                    // Weighted sum emphasizing lower frequencies
                    val weights = floatArrayOf(0.25f, 0.25f, 0.15f, 0.12f, 0.08f, 0.06f, 0.05f, 0.04f)
                    var energy = 0f
                    for (i in 0 until NUM_BANDS) {
                        energy += data.bandEnergies[i] * weights[i]
                    }
                    energy / weights.sum()
                }

                FrequencyMode.LowPass -> {
                    var energy = 0f
                    var count = 0f
                    for (i in 0 until NUM_BANDS) {
                        if (BAND_EDGES[i + 1] <= targetFreq) {
                            energy += data.bandEnergies[i]
                            count += 1f
                        } else if (BAND_EDGES[i] < targetFreq) {
                            val frac = (targetFreq - BAND_EDGES[i]) / (BAND_EDGES[i + 1] - BAND_EDGES[i])
                            energy += data.bandEnergies[i] * frac
                            count += frac
                        }
                    }
                    if (count > 0f) energy / count else 0f
                }

                FrequencyMode.HighPass -> {
                    var energy = 0f
                    var count = 0f
                    for (i in 0 until NUM_BANDS) {
                        if (BAND_EDGES[i] >= targetFreq) {
                            energy += data.bandEnergies[i]
                            count += 1f
                        } else if (BAND_EDGES[i + 1] > targetFreq) {
                            val frac = (BAND_EDGES[i + 1] - targetFreq) / (BAND_EDGES[i + 1] - BAND_EDGES[i])
                            energy += data.bandEnergies[i] * frac
                            count += frac
                        }
                    }
                    if (count > 0f) energy / count else 0f
                }

                FrequencyMode.BandPass -> {
                    // Focus on the band containing targetFreq
                    for (i in 0 until NUM_BANDS) {
                        if (targetFreq >= BAND_EDGES[i] && targetFreq < BAND_EDGES[i + 1]) {
                            return data.bandEnergies[i]
                        }
                    }
                    0f
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Radix-2 Cooley-Tukey FFT (in-place, iterative)
// ---------------------------------------------------------------------------

/**
 * In-place radix-2 decimation-in-time FFT.
 * [real] and [imag] must have the same power-of-2 length.
 */
private fun fft(real: FloatArray, imag: FloatArray) {
    val n = real.size
    // Bit-reversal permutation
    var j = 0
    for (i in 0 until n) {
        if (i < j) {
            var tmp = real[i]; real[i] = real[j]; real[j] = tmp
            tmp = imag[i]; imag[i] = imag[j]; imag[j] = tmp
        }
        var m = n shr 1
        while (m >= 1 && j >= m) {
            j -= m
            m = m shr 1
        }
        j += m
    }

    // Butterfly stages
    var mLen = 2
    while (mLen <= n) {
        val halfM = mLen shr 1
        val wRe = cos(PI.toFloat() / halfM)
        val wIm = -sin(PI.toFloat() / halfM)
        var i = 0
        while (i < n) {
            var curRe = 1f
            var curIm = 0f
            for (k in 0 until halfM) {
                val tRe = curRe * real[i + k + halfM] - curIm * imag[i + k + halfM]
                val tIm = curRe * imag[i + k + halfM] + curIm * real[i + k + halfM]
                real[i + k + halfM] = real[i + k] - tRe
                imag[i + k + halfM] = imag[i + k] - tIm
                real[i + k] = real[i + k] + tRe
                imag[i + k] = imag[i + k] + tIm
                val nextRe = curRe * wRe - curIm * wIm
                val nextIm = curRe * wIm + curIm * wRe
                curRe = nextRe
                curIm = nextIm
            }
            i += mLen
        }
        mLen = mLen shl 1
    }
}

// ---------------------------------------------------------------------------
// Helper functions (used across signal processing modules)
// ---------------------------------------------------------------------------

/** Linear interpolation between two values. */
fun lerp(a: Float, b: Float, t: Float): Float = a + (b - a) * t

/** Apply a power curve to a value in [0, 1]. exponent 1.0 = linear. */
fun applyCurve(value: Float, exponent: Float): Float =
    value.coerceIn(0f, 1f).pow(exponent)

/** Hermite smooth step (3t^2 - 2t^3). */
fun smoothStep(value: Float): Float {
    val t = value.coerceIn(0f, 1f)
    return t * t * (3f - 2f * t)
}
