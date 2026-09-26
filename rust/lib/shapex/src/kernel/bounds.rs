//! Conservative enclosures for sizing finite construction tools. No sampling.
use cgmath::prelude::*;

use super::{
    Compound, Curve2, Curve3, DiagnosticKind, KernelError, KernelResult, ParameterRange, Surface3,
};
use crate::{Point3, Vec3};

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

    fn union(self, other: Self) -> Self {
        Self::points([self.0, self.1, other.0, other.1])
    }

    fn expanded(self, radius: f64) -> Self {
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
    KernelError::new(
        DiagnosticKind::UnsupportedGeometry,
        "cannot establish a finite geometric enclosure",
    )
}

fn curve2(curve: &Curve2, range: ParameterRange) -> KernelResult<Bounds> {
    let lift = |p: crate::Point2| Point3::new(p.x, p.y, 0.0);
    Ok(match curve {
        Curve2::Line(_) => Bounds::points([
            lift(curve.evaluate(range.start)?.point),
            lift(curve.evaluate(range.end)?.point),
        ]),
        Curve2::Circle(c) => Bounds::points([lift(c.center())]).expanded(c.radius()),
        Curve2::Nurbs(c) => Bounds::points(c.control_points().iter().copied().map(lift)),
        // Derive parameters from the retained spatial curve, never its guide.
        Curve2::SurfaceIntersection(c) => {
            parameter_bounds(c.surface(), curve3(&c.spatial_curve(), range)?)?
        }
        Curve2::SurfaceProjection(c) => parameter_bounds(c.surface(), curve3(c.curve(), range)?)?,
    })
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
            parameter_bounds(offset.base(), bounds.expanded(offset.distance().abs()))
        }
        _ => Err(unavailable()),
    }
}

fn curve3(curve: &Curve3, range: ParameterRange) -> KernelResult<Bounds> {
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
        Curve3::SurfaceCurve(c) => match c.pcurve() {
            Curve2::SurfaceIntersection(p) if p.lies_on(c.surface()) => {
                curve3(&p.spatial_curve(), range)?
            }
            Curve2::SurfaceProjection(p) if p.surface() == c.surface() => curve3(p.curve(), range)?,
            _ => surface(c.surface(), None)
                .or_else(|_| surface_patch(c.surface(), curve2(c.pcurve(), range)?))?,
        },
        Curve3::SurfaceIntersection(c) => {
            // A curve lies on both supports. A finite enclosure of either is
            // sufficient, independent of the Newton-corrected parameter guides.
            surface(c.first_surface(), None)
                .or_else(|_| surface(c.second_surface(), None))
                .or_else(|_| intersection(c.first_surface(), c.second_surface(), curve, range))?
        }
    })
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
        _ => cone_intersection(first, second).or_else(|_| cone_intersection(second, first)),
    }
}

fn offset_base(mut surface: &Surface3) -> (&Surface3, f64) {
    let mut distance = 0.0;
    while let Surface3::Offset(offset) = surface {
        distance += offset.distance().abs();
        surface = offset.base();
    }
    (surface, distance)
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
        Surface3::Offset(s) => Ok(surface_patch(s.base(), uv)?.expanded(s.distance().abs())),
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
            let d = o.distance().abs();
            surface(o.base(), boundary.map(|b| b.expanded(d)))?.expanded(d)
        }
        Surface3::RollingBall(b) => {
            curve3(b.center_curve(), b.center_range())?.expanded(b.radius())
        }
        Surface3::Ruled(b) => {
            // Ruled blends interpolate their contacts, not a circular section.
            let first = surface_patch(
                b.first_surface(),
                curve2(b.first_pcurve(), b.center_range())?,
            )?;
            let second = surface_patch(
                b.second_surface(),
                curve2(b.second_pcurve(), b.center_range())?,
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
            let rail = curve2(s.rail().curve(), s.rail().range())?;
            let profile = curve2(s.profile().curve(), s.profile().range())?;
            let rp = s.rail_plane();
            let pp = s.profile_plane();
            let start = s.rail().curve().evaluate(s.rail().range().start)?.point;
            let start = rp.origin() + rp.u() * start.x + rp.v() * start.y;
            let radius = profile
                .corners()
                .map(|p| (pp.origin() + pp.u() * p.x + pp.v() * p.y - start).magnitude())
                .fold(0.0, f64::max);
            Bounds::points(
                rail.corners()
                    .map(|p| rp.origin() + rp.u() * p.x + rp.v() * p.y),
            )
            .expanded(radius)
        }
    })
}

pub(crate) fn compound(source: &Compound) -> KernelResult<Bounds> {
    let topology = source.topology();
    let mut result: Option<Bounds> = None;
    for face in source.faces() {
        let b = surface(face.surface(), None).or_else(|_| {
            let mut boundary: Option<Bounds> = None;
            for id in topology.face_edges(face.topology_id())? {
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
        })?;
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
