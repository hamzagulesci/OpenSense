//! Line icons drawn with egui shapes. Each icon is designed on a 24×24
//! grid centred on `c` and scaled to `size`.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Gauge,
    VideoCard,
    Keyboard,
    Battery,
    Bicycle,
    Car,
    Rocket,
    Fan,
    Flower,
    Gamepad,
    Gear,
    Star,
    Download,
    Quit,
}

pub fn paint(p: &Painter, c: Pos2, size: f32, icon: Icon, color: Color32) {
    let k = size / 24.0;
    let v = |x: f32, y: f32| c + Vec2::new(x, y) * k;
    let s = Stroke::new((1.6 * k).max(1.1), color);
    let arc = |o: Pos2, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        let n = 28;
        (0..=n).map(|i| o + Vec2::angled(a0 + (a1 - a0) * i as f32 / n as f32) * r * k).collect()
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(v(x0, y0), v(x1, y1));
    match icon {
        Icon::Gauge => {
            p.add(Shape::line(arc(c, 9.5, 0.75 * PI, 2.25 * PI), s));
            p.line_segment([c, v(5.0, -5.5)], s);
            p.circle_filled(c, 2.0 * k, color);
            for a in [0.75 * PI, 1.125 * PI, 1.5 * PI, 1.875 * PI, 2.25 * PI] {
                p.line_segment([c + Vec2::angled(a) * 6.5 * k, c + Vec2::angled(a) * 8.5 * k], s);
            }
        }
        Icon::VideoCard => {
            p.rect_stroke(rrect(-9.0, -6.0, 10.0, 5.0), 1.5 * k, s);
            p.line_segment([v(-11.0, -8.0), v(-11.0, 8.0)], s);
            p.line_segment([v(-11.0, -8.0), v(-9.0, -8.0)], s);
            p.circle_stroke(v(-2.5, -0.5), 3.5 * k, s);
            p.circle_filled(v(-2.5, -0.5), 1.0 * k, color);
            p.rect_filled(rrect(4.0, -3.0, 8.0, 2.0), 0.5 * k, color);
            p.line_segment([v(-6.0, 7.5), v(7.0, 7.5)], Stroke::new(2.2 * k, color));
        }
        Icon::Keyboard => {
            p.rect_stroke(rrect(-11.0, -6.5, 11.0, 6.5), 2.0 * k, s);
            for row in 0..2 {
                for col in 0..6 {
                    let x = -7.5 + col as f32 * 3.0;
                    let y = -3.0 + row as f32 * 3.0;
                    p.rect_filled(Rect::from_center_size(v(x, y), Vec2::splat(1.6 * k)), 0.3, color);
                }
            }
            p.line_segment([v(-5.0, 3.5), v(5.0, 3.5)], s);
        }
        Icon::Battery => {
            p.rect_stroke(rrect(-11.0, -6.0, 9.0, 6.0), 2.0 * k, s);
            p.rect_filled(rrect(9.5, -2.5, 11.5, 2.5), 0.8 * k, color);
            p.add(Shape::convex_polygon(vec![v(1.0, -5.0), v(-4.5, 1.0), v(-0.5, 1.0)], color, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![v(-1.0, 5.0), v(0.5, -1.0), v(4.5, -1.0)], color, Stroke::NONE));
        }
        Icon::Bicycle => {
            p.circle_stroke(v(-6.5, 4.0), 5.0 * k, s);
            p.circle_stroke(v(6.5, 4.0), 5.0 * k, s);
            p.add(Shape::closed_line(vec![v(-6.5, 4.0), v(-2.0, -4.0), v(5.0, -4.0), v(0.0, 4.0)], s));
            p.line_segment([v(5.0, -4.0), v(6.5, 4.0)], s);
            p.line_segment([v(-2.0, -4.0), v(-3.0, -7.0)], s);
            p.line_segment([v(-4.5, -7.0), v(-1.5, -7.0)], s);
            p.line_segment([v(5.0, -4.0), v(4.0, -7.5)], s);
            p.line_segment([v(4.0, -7.5), v(7.0, -7.5)], s);
        }
        Icon::Car => {
            let mut body = vec![v(-11.0, 4.0), v(-11.0, 0.0), v(-8.0, -1.5)];
            body.extend(arc(v(-0.5, 0.5), 7.0, PI + 0.35, TAU - 0.35));
            body.extend([v(9.5, -0.5), v(11.0, 1.0), v(11.0, 4.0)]);
            p.add(Shape::line(body, s));
            p.line_segment([v(-11.0, 4.0), v(-9.5, 4.0)], s);
            p.line_segment([v(-3.0, 4.0), v(3.5, 4.0)], s);
            p.line_segment([v(9.5, 4.0), v(11.0, 4.0)], s);
            p.circle_stroke(v(-6.3, 4.5), 3.0 * k, s);
            p.circle_stroke(v(6.5, 4.5), 3.0 * k, s);
            p.line_segment([v(-5.5, -1.5), v(4.5, -1.5)], s);
            p.line_segment([v(-0.5, -1.5), v(-0.5, -5.5)], s);
        }
        Icon::Rocket => {
            // Pointing up-right: work in a rotated frame.
            let rot = |x: f32, y: f32| {
                let (sa, ca) = (-PI / 4.0).sin_cos();
                v(x * ca - y * sa, x * sa + y * ca)
            };
            let body = vec![rot(-7.0, -3.5), rot(3.0, -3.5), rot(10.0, 0.0), rot(3.0, 3.5), rot(-7.0, 3.5)];
            p.add(Shape::closed_line(body, s));
            p.circle_stroke(rot(2.0, 0.0), 1.8 * k, s);
            p.add(Shape::line(vec![rot(-4.0, -3.5), rot(-8.0, -7.5), rot(-8.0, -3.5)], s));
            p.add(Shape::line(vec![rot(-4.0, 3.5), rot(-8.0, 7.5), rot(-8.0, 3.5)], s));
            p.add(Shape::line(vec![rot(-7.0, -1.8), rot(-11.5, 0.0), rot(-7.0, 1.8)], s));
        }
        Icon::Fan => {
            p.circle_stroke(c, 10.5 * k, s);
            for i in 0..3 {
                let a = i as f32 * TAU / 3.0 - PI / 2.0;
                let d = Vec2::angled(a);
                let n = Vec2::angled(a + PI / 2.0);
                let pts = vec![
                    c + d * 2.0 * k,
                    c + (d * 5.0 + n * 3.0) * k,
                    c + (d * 8.0 + n * 1.5) * k,
                    c + (d * 7.5 - n * 1.8) * k,
                    c + (d * 3.0 - n * 1.0) * k,
                ];
                p.add(Shape::closed_line(pts, s));
            }
            p.circle_filled(c, 1.6 * k, color);
        }
        Icon::Flower => {
            for i in 0..5 {
                let a = i as f32 * TAU / 5.0 - PI / 2.0;
                p.circle_stroke(c + Vec2::angled(a) * 5.5 * k, 3.8 * k, s);
            }
            p.circle_filled(c, 2.2 * k, color);
        }
        Icon::Gamepad => {
            let mut outline = arc(v(-6.5, 1.0), 5.5, 0.5 * PI, 1.55 * PI);
            outline.extend(arc(v(6.5, 1.0), 5.5, 1.45 * PI, 2.5 * PI));
            outline.push(v(3.0, 4.5));
            outline.push(v(-3.0, 4.5));
            p.add(Shape::closed_line(outline, s));
            p.line_segment([v(-8.5, 0.5), v(-4.5, 0.5)], s);
            p.line_segment([v(-6.5, -1.5), v(-6.5, 2.5)], s);
            p.circle_filled(v(5.5, -0.5), 1.1 * k, color);
            p.circle_filled(v(7.8, 1.8), 1.1 * k, color);
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                p.line_segment(
                    [c + Vec2::angled(a) * 7.0 * k, c + Vec2::angled(a) * 10.0 * k],
                    Stroke::new(3.0 * k, color),
                );
            }
            p.circle_stroke(c, 7.0 * k, s);
            p.circle_stroke(c, 3.0 * k, s);
        }
        Icon::Star => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let r = if i % 2 == 0 { 10.0 } else { 4.2 };
                    c + Vec2::angled(-PI / 2.0 + i as f32 * PI / 5.0) * r * k
                })
                .collect();
            p.add(Shape::closed_line(pts, s));
        }
        Icon::Download => {
            p.add(Shape::line(vec![v(-10.0, 3.0), v(-10.0, 9.0), v(10.0, 9.0), v(10.0, 3.0)], s));
            p.line_segment([v(0.0, -10.0), v(0.0, 4.0)], s);
            p.add(Shape::line(vec![v(-5.0, -1.0), v(0.0, 4.0), v(5.0, -1.0)], s));
        }
        Icon::Quit => {
            p.add(Shape::line(arc(v(2.0, 0.0), 9.0, -0.75 * PI, 0.75 * PI), s));
            p.line_segment([v(-11.0, 0.0), v(3.0, 0.0)], s);
            p.add(Shape::line(vec![v(-1.0, -4.0), v(3.0, 0.0), v(-1.0, 4.0)], s));
        }
    }
}
