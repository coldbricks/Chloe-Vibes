// ==========================================================================
// AutoLockSupervisorTest.kt -- Unit tests for Android FIND BOOM / AutoLock
// ==========================================================================

package com.ashairfoil.chloevibes.audio

import org.junit.jupiter.api.Assertions.*
import org.junit.jupiter.api.Test

class AutoLockSupervisorTest {

    @Test
    fun `no lock on silence`() {
        val supervisor = AutoLockSupervisor()
        supervisor.startListening(1000f)

        var params = ProcessingParams()
        var t = 1000f
        val dt = 25f

        // Feed 16 seconds of silence (exceeds 15s budget)
        while (t <= 18000f) {
            supervisor.pushFrame(
                currentTimeMs = t,
                spectral = SpectralData(),
                preVolumeEnergy = 0.0001f,
                isOnset = false
            )
            params = supervisor.update(t, 0f, params)
            t += dt
        }

        assertTrue(supervisor.state is AutoLockState.NoLock, "Silence must result in NoLock")
    }

    @Test
    fun `locks on rhythmic bass material and fits decay to tempo`() {
        val supervisor = AutoLockSupervisor()
        val initialParams = ProcessingParams(
            decayMs = 120f,
            frequencyMode = FrequencyMode.Full,
            sustainLevel = 0.5f
        )
        supervisor.startListening(1000f)

        var params = initialParams
        var t = 1000f
        val dt = 20f // 50 Hz frame rate
        val beatInterval = 500f // 120 BPM

        // Feed 6 seconds of steady 120 BPM kicks in Sub/Bass band
        while (t <= 7500f) {
            val isBeat = (t % beatInterval) < dt
            val energies = FloatArray(8)
            val preVol: Float

            if (isBeat) {
                // Sub & Bass bands punch
                energies[0] = 0.85f
                energies[1] = 0.65f
                preVol = 0.8f
            } else {
                // Trough floor
                energies[0] = 0.02f
                energies[1] = 0.01f
                preVol = 0.03f
            }

            val spectral = SpectralData(
                bandEnergies = energies,
                spectralCentroid = 80f,
                spectralFlux = if (isBeat) 8f else 0.02f
            )

            supervisor.pushFrame(
                currentTimeMs = t,
                spectral = spectral,
                preVolumeEnergy = preVol,
                isOnset = isBeat
            )

            params = supervisor.update(t, 0.85f, params)
            t += dt
        }

        assertTrue(supervisor.isLocked(), "Expected supervisor to lock on rhythmic bass, state is ${supervisor.state}")
        val lockedState = supervisor.state as AutoLockState.Locked
        assertTrue(lockedState.scorePercent >= 45, "Expected lock score >= 45%, got ${lockedState.scorePercent}%")

        // Bass/sub band should be chosen
        assertEquals(FrequencyMode.LowPass, params.frequencyMode)

        // Decay should fit ~78% of 500ms beat (~390ms)
        val expectedDecay = 0.78f * 500f
        assertEquals(expectedDecay, params.decayMs, 25f)

        // Low sustain target (near zero floor)
        assertTrue(params.sustainLevel <= 0.15f, "Expected low sustain floor, got ${params.sustainLevel}")

        // Test Revert:
        val reverted = supervisor.revert(params)
        assertEquals(initialParams.decayMs, reverted.decayMs)
        assertEquals(initialParams.frequencyMode, reverted.frequencyMode)
        assertEquals(initialParams.sustainLevel, reverted.sustainLevel)
        assertEquals(AutoLockState.Idle, supervisor.state)
    }

    @Test
    fun `keep dissolves lock and preserves locked parameters`() {
        val supervisor = AutoLockSupervisor()
        val initialParams = ProcessingParams(decayMs = 120f)
        supervisor.startListening(1000f)

        var params = initialParams
        var t = 1000f
        val dt = 20f

        while (t <= 7500f) {
            val isBeat = (t % 500f) < dt
            val energies = FloatArray(8)
            val preVol: Float

            if (isBeat) {
                energies[1] = 0.80f
                preVol = 0.75f
            } else {
                energies[1] = 0.01f
                preVol = 0.02f
            }

            supervisor.pushFrame(
                currentTimeMs = t,
                spectral = SpectralData(bandEnergies = energies),
                preVolumeEnergy = preVol,
                isOnset = isBeat
            )
            params = supervisor.update(t, 0.85f, params)
            t += dt
        }

        assertTrue(supervisor.isLocked())
        val tunedDecay = params.decayMs

        supervisor.keep()
        assertEquals(AutoLockState.Idle, supervisor.state)
        assertNull(supervisor.snapshot)

        // Parameters remain at tuned decay
        assertEquals(tunedDecay, params.decayMs)
    }

    @Test
    fun `manual knob tweak dissolves lock safely`() {
        val supervisor = AutoLockSupervisor()
        supervisor.startListening(1000f)

        var params = ProcessingParams()
        var t = 1000f
        val dt = 20f

        while (t <= 7500f) {
            val isBeat = (t % 500f) < dt
            val energies = FloatArray(8)
            val preVol: Float

            if (isBeat) {
                energies[1] = 0.80f
                preVol = 0.75f
            } else {
                energies[1] = 0.01f
                preVol = 0.02f
            }

            supervisor.pushFrame(
                currentTimeMs = t,
                spectral = SpectralData(bandEnergies = energies),
                preVolumeEnergy = preVol,
                isOnset = isBeat
            )
            params = supervisor.update(t, 0.85f, params)
            t += dt
        }

        assertTrue(supervisor.isLocked())

        // User manually tweaks attackMs
        params = params.copy(attackMs = 85f)
        params = supervisor.update(t + 20f, 0.85f, params)

        assertEquals(AutoLockState.Idle, supervisor.state, "Manual edit must dissolve lock")
        assertNull(supervisor.snapshot)
        assertEquals(85f, params.attackMs)
    }
}
