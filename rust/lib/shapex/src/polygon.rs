//! Predicates on straight segments and closed polylines.

use cgmath::prelude::*;

use crate::Point2;

/// Distance from `point` to the closed segment between `start` and `end`.
pub(crate) fn point_segment_distance<P>(point: P, start: P, end: P) -> f64
where
    P: EuclideanSpace<Scalar = f64> + MetricSpace<Metric = f64>,
    P::Diff: InnerSpace<Scalar = f64>,
{
    let direction = end - start;
    let length2 = direction.magnitude2();
    if length2 == 0.0 {
        return point.distance(start);
    }
    let parameter = (point - start).dot(direction) / length2;
    point.distance(start + direction * parameter.clamp(0.0, 1.0))
}

/// Whether `point` lies within `tolerance` of the closed segment.
pub(crate) fn on_segment(point: Point2, start: Point2, end: Point2, tolerance: f64) -> bool {
    point_segment_distance(point, start, end) <= tolerance
}

/// Whether `point` lies within `tolerance` of the boundary of `polygon`.
pub(crate) fn on_polygon_boundary(point: Point2, polygon: &[Point2], tolerance: f64) -> bool {
    polygon_edges(polygon).any(|(start, end)| on_segment(point, start, end, tolerance))
}

/// Even-odd crossing test. Points on the boundary may classify either way.
pub(crate) fn point_in_polygon(point: Point2, polygon: &[Point2]) -> bool {
    polygon_edges(polygon)
        .filter(|(first, second)| {
            (first.y > point.y) != (second.y > point.y)
                && point.x
                    < (second.x - first.x) * (point.y - first.y) / (second.y - first.y) + first.x
        })
        .count()
        % 2
        == 1
}

/// The undirected key of an edge between two vertex indices.
pub(crate) fn ordered(first: usize, second: usize) -> (usize, usize) {
    (first.min(second), first.max(second))
}

/// The closing sequence of edges of a polygon given by its vertices.
pub(crate) fn polygon_edges(polygon: &[Point2]) -> impl Iterator<Item = (Point2, Point2)> + '_ {
    (0..polygon.len()).map(|index| (polygon[index], polygon[(index + 1) % polygon.len()]))
}
