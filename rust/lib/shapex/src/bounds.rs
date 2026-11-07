//! Conservative enclosures that prove separation and size construction tools.
//! An unsupported curve or surface is an error, never a sampled guess.
use cgmath::prelude::*;

use super::{
    Compound, Curve2, Curve3, DiagnosticKind, KernelError, KernelResult, ParameterRange, Surface3,
};
use crate::{Point3, Vec3};
use crate::unsupported;

#[derive(Clone, Copy)]
pub(crate) struct Bounds(pub Point3, pub Point3);

impl Bounds {
    pub(crate) fn separated(self, other: Self, tolerance: f64) -> bool {
        (0..3).any(|axis| {
            self.1[axis] < other.0[axis] - tolerance
                || other.1[axis] < self.0[axis] - tolerance
        })
    }

    fn points(points: impl IntoIterator<Item = Point3>) -> Self {
        let mut result = Self(
            Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for point in points {
            for axis in 0..3 {
                result.0[axis] = result.0[axis].min(point[axis]);
                result.1[axis] = result.1[axis].max(point[axis]);
            }
        }
        result
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self::points([self.0, self.1, other.0, other.1])
    }

    pub(crate) fn expanded(self, radius: f64) -> Self {
        let padding = Vec3::new(radius, radius, radius);
        Self(self.0 - padding, self.1 + padding)
    }

    pub(crate) fn corners(self) -> impl Iterator<Item = Point3> {
        (0..8).map(move |i| {
            Point3::new(
                if i & 1 == 0 { self.0.x } else { self.1.x },
                if i & 2 == 0 { self.0.y } else { self.1.y },
                if i & 4 == 0 { self.0.z } else { self.1.z },
            )
        })
    }
}

fn unavailable() -> KernelError {
    unsupported("cannot establish a finite geometric enclosure")
}

pub(crate) fn curve2(curve: &Curve2, range: ParameterRange) -> KernelResult<Bounds> {
    let lift = |p: crate::Point2| Point3::new(p.x, p.y, 0.0);
    Ok(match curve {
        Curve2::Line(_) => Bounds::points([
            lift(curve.evaluate(range.start)?.point),
            lift(curve.evaluate(range.end)?.point),
        ]),
        Curve2::Circle(c) => Bounds::points([lift(c.center())]).expanded(c.radius()),
        Curve2::Nurbs(c) => Bounds::points(c.control_points().iter().copied().map(lift)),
        Curve2::Offset(c) => curve2(c.base(), range)?.expanded(c.distance().abs()),
        Curve2::Projected(c) => parameter_bounds(&Surface3::Plane(c.plane()), curve3(c.curve(),range)?)?,
        // Derive parameters from the retained spatial curve, never its guide.
        Curve2::SurfaceIntersection(c) => {
            if c.is_contact_rail() { return Err(unavailable()); }
            parameter_bounds(c.surface(), curve3(&c.spatial_curve(), range)?)?
        }
        Curve2::SurfaceProjection(c) => parameter_bounds(c.surface(), curve3(c.curve(), range)?)?,
    })
}

pub(super) fn wire2(wire: &super::Wire2) -> KernelResult<Bounds> {
    let mut bounds: Option<Bounds> = None;
    for segment in wire.segments() {
        let next = curve2(segment.curve(), segment.range())?;
        bounds = Some(bounds.map_or(next, |current| current.union(next)));
    }
    bounds.ok_or_else(unavailable)
}

fn parameter_bounds(s: &Surface3, bounds: Bounds) -> KernelResult<Bounds> {
    match s {
        Surface3::Cylinder(c) => {
            let heights = Bounds::points(bounds.corners().map(|q| {
                Point3::new(
                    0.0,
                    (q - c.origin()).dot(c.axis()) / c.axis().magnitude2(),
                    0.0,
                )
            }));
            // Azimuth may be unwrapped by any number of periods. Cylinder
            // enclosures only need height and include every azimuth anyway.
            Ok(Bounds(
                Point3::new(f64::NEG_INFINITY, heights.0.y, 0.0),
                Point3::new(f64::INFINITY, heights.1.y, 0.0),
            ))
        }
        Surface3::Plane(p) => {
            let u = p.u();
            let v = p.v();
            let uu = u.dot(u);
            let vv = v.dot(v);
            let uv = u.dot(v);
            let determinant = uu * vv - uv * uv;
            Ok(Bounds::points(bounds.corners().map(|q| {
                let d = q - p.origin();
                Point3::new(
                    (d.dot(u) * vv - d.dot(v) * uv) / determinant,
                    (d.dot(v) * uu - d.dot(u) * uv) / determinant,
                    0.0,
                )
            })))
        }
        Surface3::Cone(c) => {
            let heights = Bounds::points(bounds.corners().map(|q| {
                Point3::new(
                    0.0,
                    (q - c.apex()).dot(c.axis()) / c.axis().magnitude2(),
                    0.0,
                )
            }));
            Ok(Bounds(
                Point3::new(f64::NEG_INFINITY, heights.0.y, 0.0),
                Point3::new(f64::INFINITY, heights.1.y, 0.0),
            ))
        }
        Surface3::Offset(offset) => {
            parameter_bounds(offset.base(), bounds.expanded(offset.maximum_distance()))
        }
        _ => Err(unavailable()),
    }
}

pub(super) fn curve3(curve: &Curve3, range: ParameterRange) -> KernelResult<Bounds> {
    Ok(match curve {
        Curve3::Line(_) => Bounds::points([
            curve.evaluate(range.start)?.point,
            curve.evaluate(range.end)?.point,
        ]),
        Curve3::Circle(c) => {
            let x = c.x_radius();
            let y = c.y_radius();
            let extent = Vec3::new(x.x.hypot(y.x), x.y.hypot(y.y), x.z.hypot(y.z));
            Bounds(c.center() - extent, c.center() + extent)
        }
        Curve3::Nurbs(c) => Bounds::points(c.control_points().iter().copied()),
        Curve3::SurfaceCurve(c) => surface_curve(c.surface(), c.pcurve(), range)?,
        Curve3::SurfaceIntersection(c) => {
            if c.constant_width.is_some() { return Err(unavailable()); }
            // A curve lies on both supports. A finite enclosure of either is
            // sufficient, independent of the Newton-corrected parameter guides.
            surface(c.first_surface(), None)
                .or_else(|_| surface(c.second_surface(), None))
                .or_else(|_| intersection(c.first_surface(), c.second_surface(), curve, range))?
        }
    })
}

fn surface_curve(
    support: &Surface3,
    pcurve: &Curve2,
    range: ParameterRange,
) -> KernelResult<Bounds> {
    match pcurve {
        Curve2::SurfaceIntersection(p) if p.is_contact_rail() => Err(unavailable()),
        Curve2::SurfaceIntersection(p) if p.lies_on(support) => {
            curve3(&p.spatial_curve(), range)
        }
        Curve2::SurfaceProjection(p) if p.surface() == support => curve3(p.curve(), range),
        _ => surface(support, None)
            .or_else(|_| surface_patch(support, curve2(pcurve, range)?)),
    }
}

fn intersection(
    first: &Surface3,
    second: &Surface3,
    curve: &Curve3,
    range: ParameterRange,
) -> KernelResult<Bounds> {
    match (first, second) {
        (Surface3::Plane(p), Surface3::Cylinder(c))
        | (Surface3::Cylinder(c), Surface3::Plane(p)) => {
            let axis = c.axis().normalize();
            let normal = p.normal().normalize();
            let divisor = normal.dot(axis);
            if divisor.abs() <= f64::EPSILON {
                return Err(unavailable());
            }
            let radius = c.x_radius().magnitude().max(c.y_radius().magnitude());
            let height = normal.dot(p.origin() - c.origin()) / divisor;
            let span = radius / divisor.abs();
            Ok(Bounds::points([
                c.origin() + axis * (height - span),
                c.origin() + axis * (height + span),
            ])
            .expanded(radius))
        }
        (Surface3::Cylinder(a), Surface3::Cylinder(b)) => {
            let axis = a.axis().normalize();
            let other = b.axis().normalize();
            let projected = axis - other * axis.dot(other);
            let length = projected.magnitude();
            if length <= f64::EPSILON {
                return Err(unavailable());
            }
            let delta = a.origin() - b.origin();
            let delta = delta - other * delta.dot(other);
            let height = -delta.dot(projected) / projected.magnitude2();
            let radius = a.x_radius().magnitude().max(a.y_radius().magnitude());
            let other_radius = b.x_radius().magnitude().max(b.y_radius().magnitude());
            let span = (radius + other_radius) / length;
            Ok(Bounds::points([
                a.origin() + axis * (height - span),
                a.origin() + axis * (height + span),
            ])
            .expanded(radius))
        }
        (Surface3::Plane(_), Surface3::Plane(_)) => Ok(Bounds::points([
            curve.evaluate(range.start)?.point,
            curve.evaluate(range.end)?.point,
        ])),
        _ => trimmed_plane_cone(first, second, curve, range)
            .or_else(|_| cone_intersection(first, second))
            .or_else(|_| cone_intersection(second, first)),
    }
}

fn offset_base(mut surface: &Surface3) -> (&Surface3, f64) {
    let mut distance = 0.0;
    while let Surface3::Offset(offset) = surface {
        distance += offset.maximum_distance();
        surface = offset.base();
    }
    (surface, distance)
}

fn trimmed_plane_cone(
    first: &Surface3,
    second: &Surface3,
    curve: &Curve3,
    range: ParameterRange,
) -> KernelResult<Bounds> {
    let (first, first_offset) = offset_base(first);
    let (second, second_offset) = offset_base(second);
    let (cone, offset, plane) = match (first, second) {
        (Surface3::Cone(cone), Surface3::Plane(plane)) => (cone, first_offset, plane),
        (Surface3::Plane(plane), Surface3::Cone(cone)) => (cone, second_offset, plane),
        _ => return Err(unavailable()),
    };
    let normal = plane.normal().normalize();
    let radial = normal.dot(cone.x_radius()).hypot(normal.dot(cone.y_radius()));
    if normal.dot(cone.axis()).abs() > radial {
        // An elliptic section is bounded by the complete support intersection.
        return Err(unavailable());
    }
    // A constant normal offset of a circular cone translates its apex along
    // the axis by d / sin(half_angle). Nested offsets can reverse its normal,
    // so bound the total translation by the sum of their absolute distances.
    // An offset plane remains parallel and does not change the section type.
    let axis_length = cone.axis().magnitude();
    let radius = cone.x_radius().magnitude().min(cone.y_radius().magnitude());
    let apex_shift = offset * axis_length.hypot(radius) / (radius * axis_length);
    let mut low = -apex_shift;
    let mut high = apex_shift;
    let mut height = 0.0_f64;
    for parameter in [range.start, range.end] {
        let axial = (curve.evaluate(parameter)?.point - cone.apex()).dot(cone.axis())
            / cone.axis().magnitude2();
        low = low.min(axial);
        high = high.max(axial);
        height = height.max(axial.abs());
    }
    // On a continuous hyperbolic or parabolic branch, |height| from the true
    // apex is bounded by the endpoints. Thus axial positions lie between the
    // endpoints and the translated apex; the radial bound allows its shift.
    let axial = Bounds::points([
        cone.apex() + cone.axis() * low,
        cone.apex() + cone.axis() * high,
    ]);
    let x = cone.x_radius();
    let y = cone.y_radius();
    let extent = Vec3::new(x.x.hypot(y.x), x.y.hypot(y.y), x.z.hypot(y.z))
        * (height + apex_shift);
    Ok(Bounds(axial.0 - extent, axial.1 + extent))
}

fn cone_intersection(cone_support: &Surface3, other: &Surface3) -> KernelResult<Bounds> {
    let (base, cone_offset) = offset_base(cone_support);
    let Surface3::Cone(cone) = base else {
        return Err(unavailable());
    };
    let (other, other_offset) = offset_base(other);
    let padding = cone_offset + other_offset;
    let radius = cone.x_radius().magnitude().max(cone.y_radius().magnitude());
    // Bound the cone's axial parameter analytically. Offsets contribute a
    // distance allowance to the supporting plane or cylindrical enclosure.
    let (numerator, denominator) = match other {
        Surface3::Plane(plane) => {
            let normal = plane.normal().normalize();
            let radial = normal
                .dot(cone.x_radius())
                .hypot(normal.dot(cone.y_radius()));
            (
                normal.dot(plane.origin() - cone.apex()).abs() + padding,
                normal.dot(cone.axis()).abs() - radial,
            )
        }
        Surface3::Cylinder(cylinder) => {
            let axis = cylinder.axis().normalize();
            let perpendicular = |v: Vec3| v - axis * v.dot(axis);
            let radial = cone.x_radius().magnitude().min(cone.y_radius().magnitude())
                * cone.axis().normalize().dot(axis).abs();
            (
                perpendicular(cone.apex() - cylinder.origin()).magnitude()
                    + cylinder
                        .x_radius()
                        .magnitude()
                        .max(cylinder.y_radius().magnitude())
                    + padding,
                radial - perpendicular(cone.axis()).magnitude(),
            )
        }
        _ => return Err(unavailable()),
    };
    // A nonpositive denominator permits an unbounded intersection.
    if denominator <= f64::EPSILON {
        return Err(unavailable());
    }
    let height = numerator / denominator;
    Ok(Bounds::points([
        cone.apex() - cone.axis() * height,
        cone.apex() + cone.axis() * height,
    ])
    .expanded(radius * height + cone_offset))
}

fn surface_patch(support: &Surface3, uv: Bounds) -> KernelResult<Bounds> {
    match support {
        Surface3::Plane(p) => Ok(Bounds::points(
            uv.corners().map(|q| p.origin() + p.u() * q.x + p.v() * q.y),
        )),
        Surface3::Cylinder(c) => Ok(Bounds::points([
            c.origin() + c.axis() * uv.0.y,
            c.origin() + c.axis() * uv.1.y,
        ])
        .expanded(c.x_radius().magnitude().max(c.y_radius().magnitude()))),
        Surface3::Cone(c) => {
            Ok(
                Bounds::points([c.apex() + c.axis() * uv.0.y, c.apex() + c.axis() * uv.1.y])
                    .expanded(
                        uv.0.y.abs().max(uv.1.y.abs())
                            * c.x_radius().magnitude().max(c.y_radius().magnitude()),
                    ),
            )
        }
        Surface3::Offset(s) => Ok(surface_patch(s.base(), uv)?.expanded(s.maximum_distance())),
        Surface3::Blend(blend) => {
            let u = blend.center_range().middle();
            let translation = blend.translating_quintic(u)?;
            let rotation = if translation.is_none() { blend.rotating_quintic(u)? } else { None };
            let controls = translation.map(|(controls, _)| controls)
                .or_else(|| rotation.map(|(controls, _)| controls)).ok_or_else(unavailable)?;
            // Restrict the Bernstein polygon to the chart box. The blossom
            // also handles trims on an extension of the nominal section.
            let section = (0..=5).map(|upper_count| {
                let mut points = controls;
                for level in 0..5 {
                    let v = if level < upper_count { uv.1.y } else { uv.0.y };
                    for index in 0..5 - level {
                        points[index] = points[index] + (points[index + 1] - points[index]) * v;
                    }
                }
                points[0]
            });
            if let Some((_, direction)) = translation {
                Ok(Bounds::points(section.flat_map(|point| {
                    [point + direction * (uv.0.x - u), point + direction * (uv.1.x - u)]
                })))
            } else {
                let (_, axis) = rotation.unwrap();
                Ok(section.map(|point| {
                    let axial = axis.origin() + axis.direction() * (point - axis.origin()).dot(axis.direction());
                    let radius = point.distance(axial);
                    let extent = Vec3::new(1.0, 1.0, 1.0) - axis.direction().mul_element_wise(axis.direction());
                    let extent = Vec3::new(extent.x.max(0.0).sqrt(), extent.y.max(0.0).sqrt(), extent.z.max(0.0).sqrt()) * radius;
                    Bounds(axial - extent, axial + extent)
                }).reduce(Bounds::union).unwrap())
            }
        }
        _ => surface(support, None),
    }
}

fn surface(s: &Surface3, boundary: Option<Bounds>) -> KernelResult<Bounds> {
    Ok(match s {
        Surface3::Plane(_) => boundary.ok_or_else(unavailable)?,
        Surface3::Cylinder(c) => {
            // Axial height has no interior extremum on a cylinder. Enclose its
            // boundary heights, then include the full circular cross-sections.
            let b = boundary.ok_or_else(unavailable)?;
            Bounds::points(b.corners().map(|p| {
                c.origin() + c.axis() * ((p - c.origin()).dot(c.axis()) / c.axis().magnitude2())
            }))
            .expanded(c.x_radius().magnitude().max(c.y_radius().magnitude()))
        }
        Surface3::Cone(c) => {
            let b = boundary.ok_or_else(unavailable)?;
            let heights: Vec<_> = b
                .corners()
                .map(|p| (p - c.apex()).dot(c.axis()) / c.axis().magnitude2())
                .collect();
            let radius = heights.iter().copied().map(f64::abs).fold(0.0, f64::max)
                * c.x_radius().magnitude().max(c.y_radius().magnitude());
            Bounds::points(heights.into_iter().map(|h| c.apex() + c.axis() * h)).expanded(radius)
        }
        Surface3::Sphere(s) => {
            let x = s.x_radius();
            let y = s.y_radius();
            let z = s.z_radius();
            let r = Vec3::new(
                (x.x * x.x + y.x * y.x + z.x * z.x).sqrt(),
                (x.y * x.y + y.y * y.y + z.y * z.y).sqrt(),
                (x.z * x.z + y.z * y.z + z.z * z.z).sqrt(),
            );
            Bounds(s.center() - r, s.center() + r)
        }
        Surface3::Torus(t) => Bounds::points([t.center()])
            .expanded(t.x_major().magnitude().max(t.y_major().magnitude()) + t.minor_radius()),
        Surface3::Nurbs(n) => Bounds::points(n.control_points().iter().flatten().copied()),
        Surface3::Offset(o) => {
            // The supplied boundary is on the offset, so first enclose the
            // corresponding base boundary as well.
            let d = o.maximum_distance();
            surface(o.base(), boundary.map(|b| b.expanded(d)))?.expanded(d)
        }
        Surface3::RollingBall(b) => {
            curve3(b.center_curve(), b.center_range())?.expanded(b.radius())
        }
        Surface3::Blend(_) => return Err(unavailable()),
        Surface3::Ruled(b) => {
            // Ruled blends interpolate their contacts, not a circular section.
            let first = surface_curve(
                b.first_surface(),
                b.first_pcurve(), b.center_range(),
            )?;
            let second = surface_curve(
                b.second_surface(),
                b.second_pcurve(), b.center_range(),
            )?;
            first.union(second)
        }
        Surface3::Revolved(r) => {
            let profile = curve2(r.profile(), r.profile_range())?;
            let axis = r.axis();
            Bounds::points([
                axis.origin() + axis.direction() * profile.0.y,
                axis.origin() + axis.direction() * profile.1.y,
            ])
            .expanded(profile.0.x.abs().max(profile.1.x.abs()))
        }
        Surface3::Swept(s) => {
            // The section only turns about the rail, so it stays within its
            // largest reach from the rail start around every rail point.
            let profile = curve2(s.profile().curve(), s.profile().range())?;
            let pp = s.profile_plane();
            let start = s.rail().evaluate(s.rail_range().start)?.point;
            let radius = profile
                .corners()
                .map(|p| (pp.origin() + pp.u() * p.x + pp.v() * p.y - start).magnitude())
                .fold(0.0, f64::max);
            curve3(s.rail(), s.rail_range())?.expanded(radius)
        }
    })
}

pub(crate) fn face(topology: &super::Topology, id: super::FaceId) -> KernelResult<Bounds> {
    let face = topology.face(id)?;
    if matches!(face.surface(), Surface3::Blend(_)) {
        let chart = || -> KernelResult<Bounds> {
            let mut bounds: Option<Bounds> = None;
            for loop_id in face.loops() {
                for use_id in topology.loop_coedges(*loop_id)? {
                    let coedge = topology.coedge(use_id)?;
                    let edge = curve2(coedge.pcurve(), coedge.range())?;
                    bounds = Some(bounds.map_or(edge, |old| old.union(edge)));
                }
            }
            surface_patch(face.surface(), bounds.ok_or_else(unavailable)?)
        };
        if let Ok(bounds) = chart() { return Ok(bounds); }
    }
    surface(face.surface(), None).or_else(|_| {
            let mut boundary: Option<Bounds> = None;
            for id in topology.face_edges(id)? {
                let edge = topology.edge(id)?;
                let b = curve3(edge.curve(), edge.range()).map_err(|error| {
                    KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        format!("bounds for edge {:?}: {}", edge.name(), error),
                    )
                })?;
                boundary = Some(boundary.map_or(b, |old| old.union(b)));
            }
            surface(face.surface(), boundary)
        })
}

pub(crate) fn compound(source: &Compound) -> KernelResult<Bounds> {
    let topology = source.topology();
    let mut result: Option<Bounds> = None;
    for value in source.faces() {
        let b = face(topology, value.topology_id())?;
        result = Some(result.map_or(b, |old| old.union(b)));
    }
    let result = result.ok_or_else(unavailable)?;
    if result
        .corners()
        .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
    {
        return Err(unavailable());
    }
    Ok(result.expanded(source.tolerance().linear * 64.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Cone3, ModelTolerance, NurbsCurve3, OffsetSurface3, Plane3};

    #[test]
    fn plane_offset_cone_bounds_enclose_trimmed_hyperbolas() {
        let tolerance = ModelTolerance::default();
        // Radius, offsets, base sheet, section sheet, plane x, arc parameter.
        // The narrow cone's inward offset turns back toward its translated apex:
        // its interior is much higher than either endpoint in the original frame.
        for (radius, offsets, base_sheet, section_sheet, x, a) in [
            (1.0, vec![], 1.0, 1.0, 1.0, 0.8),
            (1.0, vec![1.0], 1.0, 1.0, 1.0, 0.8),
            (1.0, vec![1.0], -1.0, -1.0, 1.0, 0.8),
            (1.0, vec![0.5, 0.25], 1.0, 1.0, 1.0, 0.8),
            (0.01, vec![-1.0], 1.0, -1.0, 0.001, (989.0_f64 / 991.0).sqrt()),
        ] {
            let mut support: Surface3 = Cone3::new(
                Point3::origin(), Vec3::unit_z(),
                Vec3::unit_x() * radius, Vec3::unit_y() * radius, tolerance,
            ).unwrap().into();
            let distance: f64 = offsets.iter().sum();
            for offset in offsets {
                support = OffsetSurface3::new(support, offset, tolerance).unwrap().into();
            }
            // A circular cone's normal offset translates its apex along the axis.
            let apex = -base_sheet * distance * 1.0_f64.hypot(radius) / radius;
            let height = section_sheet * x / radius;
            let end_weight = 1.0 - a * a;
            let middle_weight = 1.0 + a * a;
            let end_height = apex + height * middle_weight / end_weight;
            // Exact rational quadratic for x = constant,
            // (z - apex)^2 = (x^2 + y^2) / radius^2.
            let curve: Curve3 = NurbsCurve3::new(2, vec![
                Point3::new(x, -2.0 * x * a / end_weight, end_height),
                Point3::new(x, 0.0, apex + height * end_weight / middle_weight),
                Point3::new(x, 2.0 * x * a / end_weight, end_height),
            ], vec![end_weight, middle_weight, end_weight],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap().into();
            let plane: Surface3 = Plane3::new(Point3::new(x, 0.0, 0.0),
                Vec3::unit_y(), Vec3::unit_z(), tolerance).unwrap().into();
            let offset_plane: Surface3 = OffsetSurface3::new(
                Plane3::new(Point3::new(x - 0.1, 0.0, 0.0),
                    Vec3::unit_y(), Vec3::unit_z(), tolerance).unwrap().into(),
                0.1, tolerance,
            ).unwrap().into();
            for range in [
                ParameterRange::new(0.0, 1.0).unwrap(),
                ParameterRange::new(1.0, 0.0).unwrap(),
                ParameterRange::new(0.15, 0.7).unwrap(),
            ] {
                for (first, second) in [(&support, &plane), (&plane, &support),
                    (&support, &offset_plane), (&offset_plane, &support)] {
                    let bounds = intersection(first, second, &curve, range).unwrap();
                    for step in 0..=256 {
                        let point = curve.evaluate(range.start + range.span() * step as f64 / 256.0)
                            .unwrap().point;
                        assert!((0..3).all(|axis|
                            point[axis] >= bounds.0[axis] - tolerance.linear
                                && point[axis] <= bounds.1[axis] + tolerance.linear),
                            "radius {radius}, offset {distance}, point {point:?}");
                    }
                }
            }
        }
    }
}
