package com.ashairfoil.chloevibes.audio

/** Platform boundary for setup; integer failures must not be treated as success. */
internal interface VisualizerSetupTarget {
    fun configureCaptureSize(): Int
    fun registerCaptureListener(): Int
    fun enableCapture(): Int
    fun detachOwner()
    fun disableCapture()
    fun release()
}

internal data class VisualizerSetupResult(
    val started: Boolean,
    val failureMessage: String? = null,
    val cause: Exception? = null,
)

/** A failed setup always relinquishes ownership and attempts release. */
internal fun initializeVisualizer(
    successCode: Int,
    create: () -> VisualizerSetupTarget,
): VisualizerSetupResult {
    var target: VisualizerSetupTarget? = null
    var stage = "create"
    var started = false
    return try {
        val session = create().also { target = it }
        val steps = listOf(
            "capture size" to session::configureCaptureSize,
            "capture listener" to session::registerCaptureListener,
            "enable" to session::enableCapture,
        )
        for ((name, action) in steps) {
            stage = name
            val result = action()
            if (result != successCode) {
                return VisualizerSetupResult(false,
                    "System audio setup failed at $stage (status $result)")
            }
        }
        started = true
        VisualizerSetupResult(true)
    } catch (e: Exception) {
        VisualizerSetupResult(false,
            "System audio setup failed at $stage (${e.javaClass.simpleName})", e)
    } finally {
        if (!started) {
            // Cleanup failures must not mask the original setup failure or skip
            // the next cleanup step. release must run even if disabling fails.
            target?.let {
                runCatching { it.detachOwner() }
                runCatching { it.disableCapture() }
                runCatching { it.release() }
            }
        }
    }
}
