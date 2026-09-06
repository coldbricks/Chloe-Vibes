package com.ashairfoil.chloevibes.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.pow

@Composable
internal fun EnvelopeEditor(shape: EnvelopeShape, attackCurve: Float, decayCurve: Float,
    releaseCurve: Float, onChange: (EnvelopeShape) -> Unit) {
    val model = remember { EnvelopeEditorModel() }
    model.sync(shape)
    val latestShape by rememberUpdatedState(shape)
    val latestChange by rememberUpdatedState(onChange)
    var active by remember { mutableStateOf<EnvelopeHandle?>(null) }
    var fitRevision by remember { mutableIntStateOf(0) }
    val colors = listOf(ChloeColors.Attack, ChloeColors.Decay, ChloeColors.Sustain, ChloeColors.Release)
    Column(Modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text("ENVELOPE", color = ChloeColors.Teal, fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(top = 12.dp), fontSize = 13.sp)
            TextButton(onClick = { model.fit(); fitRevision++ }) { Text("Fit view", color = ChloeColors.OnSurfaceDim) }
        }
        Text("Drag A/R sideways · D in both directions · S up/down", color = ChloeColors.OnSurfaceDim,
            fontSize = 11.sp, modifier = Modifier.padding(bottom = 8.dp))
        Row(Modifier.fillMaxWidth().padding(bottom = 6.dp), horizontalArrangement = Arrangement.SpaceBetween) {
            val values = listOf("A %.0f ms".format(shape.attack), "D %.0f ms".format(shape.decay),
                "S %.0f%%".format(shape.sustain * 100f), "R %.0f ms".format(shape.release))
            values.forEachIndexed { index, value ->
                Text(value, color = colors[index], fontSize = 11.sp, maxLines = 1,
                    overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            }
        }
        Canvas(Modifier.fillMaxWidth().height(200.dp)
            .clip(RoundedCornerShape(12.dp)).background(Color(0xFF0A0F17))
            .border(1.dp, Color(0xFF2D3F50), RoundedCornerShape(12.dp))
            .semantics { contentDescription = "Interactive attack, decay, sustain and release envelope. Detailed sliders are available in Full controls." }
            .pointerInput(Unit) {
                // Keys deliberately exclude parameter values: publishing a
                // drag must not cancel and restart its own gesture coroutine.
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    val pad = 22.dp.toPx()
                    val plot = EnvelopePlot(pad, pad, size.width - pad * 2f, size.height - pad * 2f)
                    val handle = plot.nearest(latestShape, model.span, down.position.x, down.position.y, 24.dp.toPx())
                        ?: return@awaitEachGesture // Empty space remains available to page scrolling.
                    down.consume()
                    active = handle
                    model.begin(handle, down.position.x, down.position.y, plot)
                    try {
                        while (true) {
                            val event = awaitPointerEvent()
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (!change.pressed || change.isConsumed) break
                            change.consume()
                            val next = model.move(change.position.x, change.position.y) ?: break
                            if (next != latestShape) latestChange(next)
                        }
                    } finally {
                        model.end()
                        active = null
                    }
                }
            }) {
            @Suppress("UNUSED_VARIABLE") val redrawOnFit = fitRevision
            val pad = 22.dp.toPx()
            val plot = EnvelopePlot(pad, pad, size.width - pad * 2f, size.height - pad * 2f)
            fun point(time: Float, level: Float): Offset = plot.point(time, level, model.span).let { Offset(it.first, it.second) }
            for (i in 0..4) {
                val f = i / 4f
                drawLine(Color(0xFF26303D), Offset(pad, pad + plot.height * f), Offset(pad + plot.width, pad + plot.height * f), 0.5.dp.toPx())
                drawLine(Color(0xFF1E2834), Offset(pad + plot.width * f, pad), Offset(pad + plot.width * f, pad + plot.height), 0.5.dp.toPx())
            }
            val segments = List(4) { phase ->
                List(49) { index ->
                    val t = index / 48f
                    when (phase) {
                        0 -> point(shape.attack * t, t.pow(attackCurve))
                        1 -> point(shape.attack + shape.decay * t, shape.sustain + (1f - shape.sustain) * (1f - t).pow(decayCurve))
                        2 -> point(shape.attack + shape.decay + ENVELOPE_HOLD_PREVIEW_MS * t, shape.sustain)
                        else -> point(shape.attack + shape.decay + ENVELOPE_HOLD_PREVIEW_MS + shape.release * t, shape.sustain * (1f - t).pow(releaseCurve))
                    }
                }
            }
            segments.forEachIndexed { index, points ->
                val line = Path().apply {
                    moveTo(points.first().x, points.first().y)
                    points.drop(1).forEach { lineTo(it.x, it.y) }
                }
                val fill = Path().apply {
                    addPath(line)
                    lineTo(points.last().x, pad + plot.height)
                    lineTo(points.first().x, pad + plot.height)
                    close()
                }
                drawPath(fill, colors[index].copy(alpha = 0.08f))
                drawPath(line, colors[index], style = Stroke(2.dp.toPx()))
            }
            plot.handles(shape, model.span).forEachIndexed { index, pair ->
                val point = Offset(pair.first, pair.second)
                if (active?.ordinal == index) drawCircle(colors[index].copy(alpha = 0.18f), 15.dp.toPx(), point)
                drawCircle(Color(0xFF0C121B), 7.dp.toPx(), point)
                drawCircle(colors[index], 7.dp.toPx(), point, style = Stroke(2.dp.toPx()))
                drawCircle(colors[index], 2.dp.toPx(), point)
            }
        }
        Text("Sustain width is a preview, not a timed hold.", color = ChloeColors.OnSurfaceDim,
            fontSize = 10.sp, modifier = Modifier.padding(top = 6.dp))
    }
}
