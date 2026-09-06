package com.ashairfoil.chloevibes.ui

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test

class EnvelopeEditorModelTest {
    private val shape = EnvelopeShape(20f, 375f, 0.08f, 240f)
    private val plot = EnvelopePlot(22f, 22f, 500f, 150f)

    @Test fun `each handle edits its own duration or level with a stable scale`() {
        for (handle in EnvelopeHandle.entries) {
            val model = EnvelopeEditorModel()
            model.sync(shape)
            val span = model.span
            model.begin(handle, 100f, 100f, plot)
            val next = model.move(125f, 70f)!!
            model.sync(next)
            assertEquals(span, model.span)
            assertEquals(next, model.move(125f, 70f))
            when (handle) {
                EnvelopeHandle.Attack -> assertEquals(shape.copy(attack = 20f + 25f * span / 500f), next)
                EnvelopeHandle.Decay -> { assertEquals(375f + 25f * span / 500f, next.decay); assertEquals(0.28f, next.sustain) }
                EnvelopeHandle.Sustain -> assertEquals(shape.copy(sustain = 0.28f), next)
                EnvelopeHandle.Release -> assertEquals(shape.copy(release = 240f + 25f * span / 500f), next)
            }
            model.end()
            assertNull(model.move(200f, 100f))
        }
    }
    @Test fun `touch targeting chooses the nearest actual handle and leaves empty space free`() {
        val model = EnvelopeEditorModel().apply { sync(shape) }
        plot.handles(shape, model.span).forEachIndexed { index, point ->
            assertEquals(EnvelopeHandle.entries[index], plot.nearest(shape, model.span, point.first, point.second, 24f))
        }
        assertNull(plot.nearest(shape, model.span, -1000f, -1000f, 24f))
    }
    @Test fun `presets cancel old drags instead of being overwritten on the next move`() {
        val model = EnvelopeEditorModel().apply { sync(shape); begin(EnvelopeHandle.Attack, 100f, 100f, plot) }
        assertNotNull(model.move(120f, 100f))
        model.sync(shape.copy(attack = 1000f))
        assertNull(model.move(130f, 100f))
    }
    @Test fun `drag extremes retain Android slider bounds and reject invalid coordinates`() {
        val model = EnvelopeEditorModel().apply { sync(shape); begin(EnvelopeHandle.Decay, 100f, 100f, plot) }
        assertNull(model.move(Float.NaN, 0f))
        val high = model.move(1e6f, -1e6f)!!
        assertEquals(5000f, high.decay)
        assertEquals(1f, high.sustain)
        val low = model.move(-1e6f, 1e6f)!!
        assertEquals(0.5f, low.decay)
        assertEquals(0f, low.sustain)
    }
}
