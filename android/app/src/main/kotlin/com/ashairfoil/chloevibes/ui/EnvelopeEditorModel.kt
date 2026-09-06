package com.ashairfoil.chloevibes.ui

import kotlin.math.pow

internal const val ENVELOPE_HOLD_PREVIEW_MS = 200f

internal data class EnvelopeShape(val attack: Float, val decay: Float, val sustain: Float, val release: Float) {
    val total: Float get() = attack + decay + ENVELOPE_HOLD_PREVIEW_MS + release
}

internal enum class EnvelopeHandle { Attack, Decay, Sustain, Release }

/** Coordinates are physical pixels; durations retain Android's existing slider limits. */
internal data class EnvelopePlot(val left: Float, val top: Float, val width: Float, val height: Float) {
    fun point(time: Float, level: Float, span: Float): Pair<Float, Float> =
        Pair(left + time / span * width, top + (1f - level) * height)

    fun handles(shape: EnvelopeShape, span: Float): List<Pair<Float, Float>> = listOf(
        point(shape.attack, 1f, span),
        point(shape.attack + shape.decay, shape.sustain, span),
        point(shape.attack + shape.decay + ENVELOPE_HOLD_PREVIEW_MS / 2f, shape.sustain, span),
        point(shape.total, 0f, span)
    )

    fun nearest(shape: EnvelopeShape, span: Float, x: Float, y: Float, radius: Float): EnvelopeHandle? =
        handles(shape, span).mapIndexed { index, point ->
            Pair(index, (x - point.first).pow(2) + (y - point.second).pow(2))
        }.filter { it.second <= radius * radius }.minByOrNull { it.second }
            ?.let { EnvelopeHandle.entries[it.first] }
}

internal class EnvelopeEditorModel {
    var span = 400f
        private set
    private var lastShape: EnvelopeShape? = null
    private var drag: Drag? = null

    private data class Drag(val handle: EnvelopeHandle, val x: Float, val y: Float,
        val shape: EnvelopeShape, val msPerPixel: Float, val height: Float)

    fun sync(shape: EnvelopeShape) {
        if (lastShape != shape) {
            // A preset or slider change takes ownership; own drag publications
            // set lastShape before the UI callback and do not refit the view.
            drag = null
            span = (shape.total * 1.25f).coerceAtLeast(400f)
            lastShape = shape
        }
    }

    fun begin(handle: EnvelopeHandle, x: Float, y: Float, plot: EnvelopePlot) {
        val shape = lastShape ?: return
        drag = Drag(handle, x, y, shape, span / plot.width.coerceAtLeast(1f), plot.height.coerceAtLeast(1f))
    }

    fun move(x: Float, y: Float): EnvelopeShape? {
        val start = drag ?: return null
        if (!x.isFinite() || !y.isFinite()) return null
        val dt = (x - start.x) * start.msPerPixel
        val ds = (start.y - y) / start.height
        val shape = when (start.handle) {
            EnvelopeHandle.Attack -> start.shape.copy(attack = (start.shape.attack + dt).coerceIn(0.5f, 5000f))
            EnvelopeHandle.Decay -> start.shape.copy(decay = (start.shape.decay + dt).coerceIn(0.5f, 5000f),
                sustain = (start.shape.sustain + ds).coerceIn(0f, 1f))
            EnvelopeHandle.Sustain -> start.shape.copy(sustain = (start.shape.sustain + ds).coerceIn(0f, 1f))
            EnvelopeHandle.Release -> start.shape.copy(release = (start.shape.release + dt).coerceIn(0.5f, 5000f))
        }
        lastShape = shape
        return shape
    }

    fun end() {
        drag = null
        lastShape?.let { if (it.total > span) span = it.total * 1.25f }
    }

    fun fit() { lastShape?.let { span = (it.total * 1.25f).coerceAtLeast(400f) } }
}
