//! Small vector icons drawn with the painter, so no icon font is needed.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::{PI, TAU};

pub fn star(painter: &Painter, center: Pos2, radius: f32, color: Color32, filled: bool) {
    let mut points = Vec::with_capacity(10);
    for i in 0..10 {
        let angle = -PI / 2.0 + i as f32 * PI / 5.0;
        let r = if i % 2 == 0 { radius } else { radius * 0.42 };
        points.push(pos2(center.x + r * angle.cos(), center.y + r * angle.sin()));
    }
    if filled {
        painter.add(Shape::convex_polygon(
            points,
            color,
            Stroke::new(1.0, color),
        ));
    } else {
        painter.add(Shape::closed_line(points, Stroke::new(1.3, color)));
    }
}

pub fn dots(painter: &Painter, center: Pos2, color: Color32) {
    for dx in [-5.0, 0.0, 5.0] {
        painter.circle_filled(pos2(center.x + dx, center.y), 1.5, color);
    }
}

pub fn warning(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let h = size / 2.0;
    let points = vec![
        pos2(center.x, center.y - h),
        pos2(center.x + h, center.y + h * 0.8),
        pos2(center.x - h, center.y + h * 0.8),
    ];
    painter.add(Shape::closed_line(points, Stroke::new(1.4, color)));
    painter.line_segment(
        [
            pos2(center.x, center.y - h * 0.35),
            pos2(center.x, center.y + h * 0.2),
        ],
        Stroke::new(1.4, color),
    );
    painter.circle_filled(pos2(center.x, center.y + h * 0.5), 0.9, color);
}

pub fn plus(painter: &Painter, center: Pos2, half: f32, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    painter.line_segment(
        [
            pos2(center.x - half, center.y),
            pos2(center.x + half, center.y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x, center.y - half),
            pos2(center.x, center.y + half),
        ],
        stroke,
    );
}

pub fn grid(painter: &Painter, center: Pos2, color: Color32) {
    for dx in [-4.5, 1.5] {
        for dy in [-4.5, 1.5] {
            let rect = Rect::from_min_size(pos2(center.x + dx, center.y + dy), vec2(3.0, 3.0));
            painter.rect_filled(rect, 0.0, color);
        }
    }
}

pub fn list(painter: &Painter, center: Pos2, color: Color32) {
    for dy in [-4.5, 0.0, 4.5] {
        let rect = Rect::from_center_size(pos2(center.x, center.y + dy), vec2(12.0, 2.0));
        painter.rect_filled(rect, 0.0, color);
    }
}

pub fn gear(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    for i in 0..8 {
        let angle = i as f32 * TAU / 8.0;
        let (sin, cos) = angle.sin_cos();
        painter.line_segment(
            [
                pos2(center.x + 5.0 * cos, center.y + 5.0 * sin),
                pos2(center.x + 7.5 * cos, center.y + 7.5 * sin),
            ],
            Stroke::new(2.2, color),
        );
    }
    painter.circle_stroke(center, 4.6, stroke);
    painter.circle_filled(center, 1.4, color);
}

/// A circular arrow; `angle` rotates it (used while a scan is running).
pub fn refresh(painter: &Painter, center: Pos2, color: Color32, angle: f32) {
    let radius = 6.5;
    let stroke = Stroke::new(1.6, color);
    let steps = 18;
    let sweep = TAU * 0.78;
    let mut points = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let a = angle + sweep * i as f32 / steps as f32;
        points.push(pos2(
            center.x + radius * a.cos(),
            center.y + radius * a.sin(),
        ));
    }
    let end = angle + sweep;
    let tip = pos2(center.x + radius * end.cos(), center.y + radius * end.sin());
    let tangent = vec2(-end.sin(), end.cos());
    let normal = vec2(end.cos(), end.sin());
    painter.add(Shape::line(points, stroke));
    painter.add(Shape::convex_polygon(
        vec![tip + tangent * 4.0, tip - normal * 3.2, tip + normal * 3.2],
        color,
        Stroke::NONE,
    ));
}

pub fn close(painter: &Painter, center: Pos2, half: f32, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.line_segment(
        [
            pos2(center.x - half, center.y - half),
            pos2(center.x + half, center.y + half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            pos2(center.x - half, center.y + half),
            pos2(center.x + half, center.y - half),
        ],
        stroke,
    );
}
