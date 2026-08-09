use crate::Point2;

use super::{DiagnosticKind, KernelError, KernelResult, ModelTolerance, Wire2};

/// A validated planar region bounded by exact 2D curve wires.
///
/// The outer wire is normalized counter-clockwise and holes clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    outer: Wire2,
    holes: Vec<Wire2>,
    area: f64,
    tolerance: ModelTolerance,
}

impl Profile {
    pub fn new(
        outer: Wire2,
        holes: impl IntoIterator<Item = Wire2>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let outer = if outer.is_counter_clockwise() {
            outer
        } else {
            outer.reversed()
        };
        let holes = holes
            .into_iter()
            .map(|hole| {
                if hole.is_counter_clockwise() {
                    hole.reversed()
                } else {
                    hole
                }
            })
            .collect::<Vec<_>>();
        let outer_polyline = outer.polyline(tolerance)?;
        for hole in &holes {
            let hole_polyline = hole.polyline(tolerance)?;
            if boundaries_intersect(&outer_polyline, &hole_polyline, tolerance.linear)
                || !point_inside_polygon(hole_polyline[0], &outer_polyline, tolerance.linear)
            {
                return Err(invalid(
                    "profile holes must lie strictly inside the outer wire",
                ));
            }
        }
        for first in 0..holes.len() {
            let first_polyline = holes[first].polyline(tolerance)?;
            for second in holes.iter().skip(first + 1) {
                let second_polyline = second.polyline(tolerance)?;
                if boundaries_intersect(&first_polyline, &second_polyline, tolerance.linear)
                    || point_inside_polygon(first_polyline[0], &second_polyline, tolerance.linear)
                    || point_inside_polygon(second_polyline[0], &first_polyline, tolerance.linear)
                {
                    return Err(invalid("profile holes may not touch, overlap, or nest"));
                }
            }
        }
        let area = outer.area() - holes.iter().map(Wire2::area).sum::<f64>();
        if area <= tolerance.linear.powi(2) {
            return Err(invalid("profile area does not exceed model tolerance"));
        }
        Ok(Self {
            outer,
            holes,
            area,
            tolerance,
        })
    }

    pub fn outer(&self) -> &Wire2 {
        &self.outer
    }
    pub fn holes(&self) -> &[Wire2] {
        &self.holes
    }
    pub fn area(&self) -> f64 {
        self.area
    }
    pub fn tolerance(&self) -> ModelTolerance {
        self.tolerance
    }
}

fn boundaries_intersect(first: &[Point2], second: &[Point2], tolerance: f64) -> bool {
    (0..first.len()).any(|first_index| {
        (0..second.len()).any(|second_index| {
            super::wire::segments_intersect(
                first[first_index],
                first[(first_index + 1) % first.len()],
                second[second_index],
                second[(second_index + 1) % second.len()],
                tolerance,
            )
        })
    })
}

fn point_inside_polygon(point: Point2, polygon: &[Point2], tolerance: f64) -> bool {
    if (0..polygon.len()).any(|index| {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        let cross =
            (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x);
        cross.abs() <= tolerance * tolerance
            && point.x >= start.x.min(end.x) - tolerance
            && point.x <= start.x.max(end.x) + tolerance
            && point.y >= start.y.min(end.y) - tolerance
            && point.y <= start.y.max(end.y) + tolerance
    }) {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        if (start.y > point.y) != (end.y > point.y) {
            let crossing = start.x + (point.y - start.y) * (end.x - start.x) / (end.y - start.y);
            if crossing > point.x {
                inside = !inside;
            }
        }
    }
    inside
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
