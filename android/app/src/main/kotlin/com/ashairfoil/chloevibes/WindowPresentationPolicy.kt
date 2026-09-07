package com.ashairfoil.chloevibes

/** Keep the phone orientation policy while opening Horizon panels in landscape. */
internal object WindowPresentationPolicy {
    private const val HORIZON_OS = "horizonos.software.horizon_os"
    private const val STANDALONE_VR = "oculus.hardware.standalone_vr"

    fun prefersLandscape(hasSystemFeature: (String) -> Boolean): Boolean =
        hasSystemFeature(HORIZON_OS) || hasSystemFeature(STANDALONE_VR)
}
