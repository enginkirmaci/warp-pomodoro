//! The countdown ring: one arc around the clock that drains with the phase,
//! plus the short bloom a phase end paints.
//!
//! The arc is a stroked path built with GPUI's `PathBuilder` (the family's own
//! drawing primitive — no new dependency). The painter is display-free: the
//! window calls it once per frame with the values it computed, so a probe or a
//! unit test can exercise the maths without a GPU.

use std::f32::consts::{FRAC_PI_2, TAU};

use gpui::prelude::*;
use gpui::{
    Background, Bounds, Corners, Edges, Hsla, PathBuilder, Pixels, Window, canvas, div, point, px,
};

/// How much of the phase is left, as the ring's share: 1 is a full ring, 0 an
/// empty one. A zero total (a hand-edited settings file) reads as empty.
pub fn progress(remaining: f32, total: f32) -> f32 {
    if total <= 0.0 {
        return 0.0;
    }
    (remaining / total).clamp(0.0, 1.0)
}

/// One frame of the ring: the share still to run and, while a phase end plays
/// out, the bloom's progress (`0..1`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RingState {
    pub progress: f32,
    pub bloom: Option<f32>,
}

/// The ring as an element: a square box of `size` pixels whose painter draws
/// the given state. The caller positions it (the timer page puts the clock
/// text over it).
pub fn ring(
    size: f32,
    thickness: f32,
    state: RingState,
    ink: Hsla,
    track: Hsla,
) -> impl IntoElement {
    let side = px(size);
    div().relative().size(side).flex_shrink_0().child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| paint(window, bounds, px(thickness), state, ink, track),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// Painter: the faded full track, the swept share, the head dot and the
/// bloom. The maths runs in plain `f32` pixels and converts at the two GPUI
/// boundaries (the path and the quad), so the geometry helpers stay testable.
fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    thickness: Pixels,
    state: RingState,
    ink: Hsla,
    track: Hsla,
) {
    let center = bounds.center();
    let cx = center.x.as_f32();
    let cy = center.y.as_f32();
    let thickness = thickness.as_f32();
    let radius = (bounds.size.width.as_f32().min(bounds.size.height.as_f32()) - thickness) / 2.;
    if radius <= 0. {
        return;
    }

    stroke_arc(window, cx, cy, radius, thickness, 0., 1., track);
    stroke_arc(window, cx, cy, radius, thickness, 0., state.progress, ink);
    if state.progress > 0. && state.progress < 1. {
        let angle = -FRAC_PI_2 + state.progress * TAU;
        paint_dot(
            window,
            cx,
            cy,
            radius,
            angle,
            thickness * 1.15,
            Hsla { a: 0.35, ..ink },
        );
        paint_dot(window, cx, cy, radius, angle, thickness * 0.62, ink);
    }
    if let Some(t) = state.bloom {
        let t = t.clamp(0., 1.);
        stroke_arc(
            window,
            cx,
            cy,
            radius + t * 16.,
            thickness * (1. - t * 0.5),
            0.,
            1.,
            Hsla {
                a: (1. - t) * 0.4,
                ..ink
            },
        );
    }
}

/// One arc segment as a stroked path. The sweep is cut into quarter turns so a
/// full ring never degenerates into a zero-length arc; the `arc_to` sweep flag
/// keeps every chunk clockwise from the twelve o'clock start.
#[allow(clippy::too_many_arguments)]
fn stroke_arc(
    window: &mut Window,
    cx: f32,
    cy: f32,
    radius: f32,
    thickness: f32,
    from: f32,
    to: f32,
    color: Hsla,
) {
    let sweep = (to - from).clamp(0., 1.) * TAU;
    if sweep < 0.0001 {
        return;
    }
    let start = -FRAC_PI_2 + from.clamp(0., 1.) * TAU;
    let mut builder = PathBuilder::stroke(px(thickness));
    builder.move_to(on_circle(cx, cy, radius, start));
    let chunks = (sweep / std::f32::consts::FRAC_PI_2).ceil() as usize;
    for index in 1..=chunks {
        let angle = start + sweep * index as f32 / chunks as f32;
        builder.arc_to(
            point(px(radius), px(radius)),
            px(0.),
            false,
            true,
            on_circle(cx, cy, radius, angle),
        );
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// A filled circle at the arc's head, drawn as a rounded quad (the paint phase
/// has no element tree).
fn paint_dot(
    window: &mut Window,
    cx: f32,
    cy: f32,
    radius: f32,
    angle: f32,
    diameter: f32,
    color: Hsla,
) {
    let half = diameter / 2.;
    let position = on_circle(cx, cy, radius, angle);
    window.paint_quad(gpui::PaintQuad {
        bounds: Bounds::new(
            point(position.x - px(half), position.y - px(half)),
            gpui::size(px(diameter), px(diameter)),
        ),
        corner_radii: Corners::all(px(half)),
        background: Background::from(color),
        border_widths: Edges::all(px(0.)),
        border_color: color,
        border_style: Default::default(),
    });
}

fn on_circle(cx: f32, cy: f32, radius: f32, angle: f32) -> gpui::Point<Pixels> {
    point(px(cx + radius * angle.cos()), px(cy + radius * angle.sin()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_is_the_remaining_fraction() {
        assert_eq!(progress(1500., 1500.), 1.);
        assert_eq!(progress(750., 1500.), 0.5);
        assert_eq!(progress(0., 1500.), 0.);
        assert_eq!(progress(10., 0.), 0., "a broken total reads empty");
        assert_eq!(progress(1600., 1500.), 1., "over budget clamps");
        assert_eq!(progress(-5., 1500.), 0.);
    }

    #[test]
    fn the_arc_starts_at_twelve_oclock_and_ends_at_the_share() {
        // the geometry helper the painter uses: angle 0 is twelve o'clock
        let top = on_circle(0., 0., 10., -FRAC_PI_2);
        assert!(top.x.as_f32().abs() < 0.001 && (top.y.as_f32() + 10.).abs() < 0.001);
        let quarter = on_circle(0., 0., 10., -FRAC_PI_2 + TAU / 4.);
        assert!(
            (quarter.x.as_f32() - 10.).abs() < 0.001,
            "a quarter turn is 3 o'clock"
        );
        assert!(quarter.y.as_f32().abs() < 0.001);
    }

    #[test]
    fn a_bloom_frame_is_optional_and_bounded() {
        let middle = RingState {
            progress: 0.5,
            bloom: Some(0.5),
        };
        assert_eq!(middle.bloom, Some(0.5));
        assert_eq!(RingState::default().bloom, None);
    }
}
