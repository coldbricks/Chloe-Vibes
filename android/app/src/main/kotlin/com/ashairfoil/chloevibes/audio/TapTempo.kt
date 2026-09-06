package com.ashairfoil.chloevibes.audio

/** Session-only tempo input; taps never produce audio onsets or motor commands. */
class TapTempo {
    private var lastMs: Double? = null
    private val intervals = mutableListOf<Double>()
    var bpm: Float? = null
        private set
    val count: Int get() = intervals.size + if (lastMs == null) 0 else 1

    fun tap(nowMs: Double) {
        if (!nowMs.isFinite()) return
        lastMs?.let { last ->
            val interval = nowMs - last
            if (interval >= 0.0 && interval < 200.0) return
            if (interval in 200.0..2000.0) {
                intervals.add(interval)
                if (intervals.size > 7) intervals.removeAt(0)
                if (intervals.size >= 3) {
                    val ordered = intervals.sorted()
                    val middle = ordered.size / 2
                    val median = if (ordered.size % 2 == 0) (ordered[middle - 1] + ordered[middle]) * 0.5 else ordered[middle]
                    bpm = (60_000.0 / median).toFloat()
                }
            } else reset()
        }
        lastMs = nowMs
    }

    fun reset() { lastMs = null; intervals.clear(); bpm = null }
}
