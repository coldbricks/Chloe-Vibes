//! Direct envelope editing. Pointer motion uses the scale captured on press,
//! so changing a duration cannot move the coordinate system under the hand.
use eframe::egui::{
    self, pos2, vec2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
};

use crate::{
    audio::{EnvelopeProcessor, EnvelopeState},
    settings::Settings,
};

const HOLD_PREVIEW_MS: f32 = 200.0;
const COLORS: [Color32; 4] = [
    Color32::from_rgb(62, 225, 207),
    Color32::from_rgb(181, 142, 255),
    Color32::from_rgb(255, 191, 97),
    Color32::from_rgb(249, 126, 164),
];

#[derive(Clone, Copy, Debug, PartialEq)]
struct EnvelopeShape {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl EnvelopeShape {
    fn read(settings: &Settings) -> Self {
        Self {
            attack: settings.attack_ms,
            decay: settings.decay_ms,
            sustain: settings.sustain_level,
            release: settings.release_ms,
        }
    }
    fn total(self) -> f32 {
        self.attack + self.decay + HOLD_PREVIEW_MS + self.release
    }
    fn write(self, settings: &mut Settings) {
        settings.attack_ms = self.attack;
        settings.decay_ms = self.decay;
        settings.sustain_level = self.sustain;
        settings.release_ms = self.release;
    }
}

#[derive(Clone, Copy)]
struct Drag {
    handle: usize,
    origin: Pos2,
    shape: EnvelopeShape,
    ms_per_pixel: f32,
    height: f32,
}

impl Drag {
    fn at(self, point: Pos2) -> EnvelopeShape {
        let mut shape = self.shape;
        let dt = (point.x - self.origin.x) * self.ms_per_pixel;
        let ds = (self.origin.y - point.y) / self.height;
        match self.handle {
            0 => shape.attack = (shape.attack + dt).clamp(0.5, 500.0),
            1 => {
                shape.decay = (shape.decay + dt).clamp(1.0, 1000.0);
                shape.sustain = (shape.sustain + ds).clamp(0.0, 1.0);
            }
            2 => shape.sustain = (shape.sustain + ds).clamp(0.0, 1.0),
            3 => shape.release = (shape.release + dt).clamp(1.0, 2000.0),
            _ => unreachable!(),
        }
        shape
    }
}

#[derive(Default)]
pub struct AdsrEditor {
    drag: Option<Drag>,
    last_shape: Option<EnvelopeShape>,
    span_ms: f32,
}

fn plot_rect(rect: Rect) -> Rect {
    Rect::from_min_max(
        pos2(rect.left() + 18.0, rect.top() + 43.0),
        pos2(rect.right() - 22.0, rect.bottom() - 30.0),
    )
}

fn handles(shape: EnvelopeShape, plot: Rect, span: f32) -> [Pos2; 4] {
    let p = |time, level| {
        pos2(
            plot.left() + time / span * plot.width(),
            plot.bottom() - level * plot.height(),
        )
    };
    [
        p(shape.attack, 1.0),
        p(shape.attack + shape.decay, shape.sustain),
        p(
            shape.attack + shape.decay + HOLD_PREVIEW_MS * 0.5,
            shape.sustain,
        ),
        p(shape.total(), 0.0),
    ]
}

fn nearest(points: &[Pos2; 4], pointer: Pos2) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(i, point)| (i, point.distance_sq(pointer)))
        .filter(|(_, distance)| *distance <= 24.0 * 24.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

pub fn show(
    ui: &mut Ui,
    editor: &mut AdsrEditor,
    settings: &mut Settings,
    envelope: &EnvelopeProcessor,
) -> bool {
    let before = EnvelopeShape::read(settings);
    if editor.drag.is_none() && editor.last_shape != Some(before) {
        editor.span_ms = (before.total() * 1.25).max(400.0);
    }
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 210.0), Sense::click_and_drag());
    let plot = plot_rect(rect);
    let points = handles(before, plot, editor.span_ms);
    let hovered = response.hover_pos().and_then(|p| nearest(&points, p));
    if hovered.is_some() || editor.drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(origin) = ui.input(|i| i.pointer.press_origin()) {
            if let Some(handle) = nearest(&points, origin) {
                editor.drag = Some(Drag {
                    handle,
                    origin,
                    shape: before,
                    ms_per_pixel: editor.span_ms / plot.width().max(1.0),
                    height: plot.height().max(1.0),
                });
            }
        }
    }
    let primary_down = ui.input(|i| i.pointer.primary_down());
    if let Some(drag) = editor.drag {
        if let Some(point) = response.interact_pointer_pos() {
            drag.at(point).write(settings);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if !primary_down {
            editor.drag = None;
        }
    }
    let shape = EnvelopeShape::read(settings);
    // Retain the view after direct edits; fit again on a preset/slider change
    // or double-click. Only expand after release if a drag went off the graph.
    if editor.drag.is_none() && (response.double_clicked() || shape.total() > editor.span_ms) {
        editor.span_ms = (shape.total() * 1.25).max(400.0);
    }
    editor.last_shape = Some(shape);
    paint(
        ui.painter(),
        rect,
        plot,
        shape,
        editor.span_ms,
        settings,
        envelope,
        editor.drag.map(|d| d.handle).or(hovered),
    );
    response.on_hover_text("Drag A left/right for attack. Drag D left/right for decay and up/down for sustain.\nDrag S up/down for sustain, R left/right for release. Double-click empty space to fit the view.\nThe sustain width is a preview, not a timed hold. Short attacks below 50 ms use the immediate-punch path.");
    shape != before
}

#[allow(clippy::too_many_arguments)]
fn paint(
    painter: &egui::Painter,
    rect: Rect,
    plot: Rect,
    shape: EnvelopeShape,
    span: f32,
    settings: &Settings,
    envelope: &EnvelopeProcessor,
    active: Option<usize>,
) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    painter.rect_filled(rect, 8.0, Color32::from_rgb(10, 15, 23));
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0_f32, Color32::from_rgb(45, 63, 80)),
        StrokeKind::Inside,
    );
    let values = [
        format!("A  {:.1} ms", shape.attack),
        format!("D  {:.0} ms", shape.decay),
        format!("S  {:.0}%", shape.sustain * 100.0),
        format!("R  {:.0} ms", shape.release),
    ];
    for (i, value) in values.iter().enumerate() {
        painter.text(
            pos2(
                plot.left() + i as f32 * plot.width() / 4.0,
                rect.top() + 17.0,
            ),
            egui::Align2::LEFT_CENTER,
            value,
            FontId::monospace(12.0),
            COLORS[i],
        );
    }
    let project = |time: f32, level: f32| {
        pos2(
            plot.left() + time / span * plot.width(),
            plot.bottom() - level * plot.height(),
        )
    };
    for i in 0..=4 {
        let fraction = i as f32 / 4.0;
        let y = plot.bottom() - fraction * plot.height();
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(0.5_f32, Color32::from_rgb(38, 48, 61)),
        );
        let x = plot.left() + fraction * plot.width();
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(0.5_f32, Color32::from_rgb(30, 40, 52)),
        );
        painter.text(
            pos2(x, plot.bottom() + 17.0),
            egui::Align2::CENTER_CENTER,
            format!("{:.0} ms", span * fraction),
            FontId::monospace(9.0),
            Color32::from_rgb(111, 130, 150),
        );
    }
    // Fixed per-segment resolution keeps the editor cheap even for long times.
    let mut segments = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for i in 0..=48 {
        let t = i as f32 / 48.0;
        segments[0].push(project(shape.attack * t, t.powf(settings.attack_curve)));
        segments[1].push(project(
            shape.attack + shape.decay * t,
            shape.sustain + (1.0 - shape.sustain) * (1.0 - t).powf(settings.decay_curve),
        ));
        segments[2].push(project(
            shape.attack + shape.decay + HOLD_PREVIEW_MS * t,
            shape.sustain,
        ));
        segments[3].push(project(
            shape.attack + shape.decay + HOLD_PREVIEW_MS + shape.release * t,
            shape.sustain * (1.0 - t).powf(settings.release_curve),
        ));
    }
    for (i, points) in segments.into_iter().enumerate() {
        for pair in points.windows(2) {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(pair[0].x, plot.bottom()),
                    pair[0],
                    pair[1],
                    pos2(pair[1].x, plot.bottom()),
                ],
                Color32::from_rgba_unmultiplied(COLORS[i].r(), COLORS[i].g(), COLORS[i].b(), 14),
                Stroke::NONE,
            ));
        }
        painter.add(Shape::line(points, Stroke::new(2.0_f32, COLORS[i])));
    }
    for (i, point) in handles(shape, plot, span).iter().enumerate() {
        if active == Some(i) {
            painter.circle_filled(
                *point,
                13.0,
                Color32::from_rgba_unmultiplied(COLORS[i].r(), COLORS[i].g(), COLORS[i].b(), 32),
            );
        }
        painter.circle_filled(*point, 6.5, Color32::from_rgb(12, 18, 27));
        painter.circle_stroke(*point, 6.5, Stroke::new(2.0_f32, COLORS[i]));
        painter.circle_filled(*point, 2.0, COLORS[i]);
        // Put captions inside the graph, away from the time axis.
        let label_y = if point.y < plot.center().y {
            point.y + 17.0
        } else {
            point.y - 17.0
        };
        painter.text(
            pos2(point.x, label_y),
            egui::Align2::CENTER_CENTER,
            ["A", "D", "S", "R"][i],
            FontId::monospace(10.0),
            COLORS[i],
        );
    }
    if envelope.state != EnvelopeState::Idle {
        let (time, color) = match envelope.state {
            EnvelopeState::Attack => (shape.attack * envelope.value, COLORS[0]),
            EnvelopeState::Decay => (
                shape.attack
                    + shape.decay
                        * (1.0
                            - (envelope.value - shape.sustain) / (1.0 - shape.sustain).max(0.01)),
                COLORS[1],
            ),
            EnvelopeState::Sustain => (
                shape.attack + shape.decay + HOLD_PREVIEW_MS * 0.5,
                COLORS[2],
            ),
            EnvelopeState::Release => (
                shape.attack
                    + shape.decay
                    + HOLD_PREVIEW_MS
                    + shape.release
                        * (1.0 - envelope.value / shape.sustain.max(0.01)).clamp(0.0, 1.0),
                COLORS[3],
            ),
            EnvelopeState::Idle => unreachable!(),
        };
        let point = project(time.clamp(0.0, span), envelope.value.clamp(0.0, 1.0));
        painter.circle_filled(point, 4.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shape() -> EnvelopeShape {
        EnvelopeShape {
            attack: 20.0,
            decay: 375.0,
            sustain: 0.08,
            release: 240.0,
        }
    }
    fn drag(handle: usize) -> Drag {
        Drag {
            handle,
            origin: pos2(100.0, 100.0),
            shape: shape(),
            ms_per_pixel: 2.0,
            height: 100.0,
        }
    }

    #[test]
    fn dragging_one_time_control_preserves_the_others() {
        let attack = drag(0).at(pos2(150.0, 60.0));
        assert_eq!(
            attack,
            EnvelopeShape {
                attack: 120.0,
                ..shape()
            }
        );
        let release = drag(3).at(pos2(150.0, 60.0));
        assert_eq!(
            release,
            EnvelopeShape {
                release: 340.0,
                ..shape()
            }
        );
    }
    #[test]
    fn decay_corner_edits_time_and_level_but_sustain_handle_only_edits_level() {
        let corner = drag(1).at(pos2(150.0, 50.0));
        assert_eq!(corner.decay, 475.0);
        assert!((corner.sustain - 0.58).abs() < 1e-6);
        let sustain = drag(2).at(pos2(500.0, 50.0));
        assert_eq!(sustain.decay, shape().decay);
        assert_eq!(sustain.release, shape().release);
        assert!((sustain.sustain - 0.58).abs() < 1e-6);
    }
    #[test]
    fn out_of_bounds_drags_respect_the_existing_control_limits() {
        assert_eq!(drag(0).at(pos2(-10000.0, 0.0)).attack, 0.5);
        assert_eq!(drag(0).at(pos2(10000.0, 0.0)).attack, 500.0);
        let decay = drag(1).at(pos2(10000.0, -10000.0));
        assert_eq!((decay.decay, decay.sustain), (1000.0, 1.0));
        assert_eq!(drag(2).at(pos2(0.0, 10000.0)).sustain, 0.0);
        assert_eq!(drag(3).at(pos2(-10000.0, 0.0)).release, 1.0);
        assert_eq!(drag(3).at(pos2(10000.0, 0.0)).release, 2000.0);
    }
    #[test]
    fn real_pointer_gestures_update_settings_without_scale_drift_or_output_limit_changes() {
        for handle in 0..4 {
            let ctx = egui::Context::default();
            let mut editor = AdsrEditor::default();
            let mut settings = Settings::default();
            shape().write(&mut settings);
            let original = EnvelopeShape::read(&settings);
            let mut frame = |events: Vec<egui::Event>| {
                let mut area = Rect::NOTHING;
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 480.0))),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            area = Rect::from_min_size(
                                ui.available_rect_before_wrap().min,
                                vec2(ui.available_width(), 210.0),
                            );
                            show(ui, &mut editor, &mut settings, &EnvelopeProcessor::new());
                        });
                    },
                );
                (
                    area,
                    EnvelopeShape::read(&settings),
                    editor.span_ms,
                    editor.drag.is_some(),
                )
            };
            let (area, _, span, _) = frame(vec![]);
            let origin = handles(original, plot_rect(area), span)[handle];
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(vec![egui::Event::PointerMoved(origin)]);
            frame(vec![button(origin, true)]);
            let point = origin + vec2(40.0, -20.0);
            let (_, changed, during_span, _) = frame(vec![egui::Event::PointerMoved(point)]);
            assert_ne!(
                changed, original,
                "handle {handle} did not respond to an actual egui drag"
            );
            assert_eq!(during_span, span);
            let (_, held, _, _) = frame(vec![]);
            assert_eq!(
                changed, held,
                "holding the pointer accumulated a second edit"
            );
            let (_, released, _, dragging) = frame(vec![button(point, false)]);
            assert_eq!(released, changed);
            assert!(!dragging);
            assert_eq!(settings.min_vibe, Settings::default().min_vibe);
            assert_eq!(settings.max_vibe, Settings::default().max_vibe);
            assert_eq!(settings.output_gain, Settings::default().output_gain);
        }
    }
}
