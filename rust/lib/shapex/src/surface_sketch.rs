use cgmath::prelude::*;

use super::extrude::{prepared_wire_segment_sources, prepared_wire_segments};
use super::sew::{publish_with_roles, BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::trim::{face_boundary_curves, face_parameter_bounds, parameter_bounds, sample_pcurve};
use super::{
    invalid, Compound, CreationRole, Curve2, Curve3, FaceId, FaceOrientation, KernelResult, Line2,
    Line3, LoopKind, ModelTolerance, ParameterRange, Plane3, Point2, Point3, Polyline, Profile,
    ProfileBoundary, Surface3, SurfaceCurve3, TessellationOptions, Topology, TriangleMesh,
    TrimmedCurve2, TrimmedCurve3, Vec2, Vec3,
};

/// The unwrapped parameter chart of a face. Sketch units match the lengths of
/// the two parameter directions at the chart origin.
#[derive(Debug, Clone)]
pub struct SurfaceSketch {
    surface: Surface3,
    orientation: FaceOrientation,
    face: FaceId,
    tolerance: ModelTolerance,
    chart: Plane3,
    edit_plane: Plane3,
    origin: Point2,
    boundary: Vec<Vec<Point2>>,
}

impl SurfaceSketch {
    pub(super) fn new(
        topology: &Topology,
        face: FaceId,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let boundary = face_boundary_curves(topology, face, tolerance)?;
        let [u, v] = face_parameter_bounds(&boundary, tolerance)?;
        let origin = Point2::new(u.middle(), v.middle());
        let entity = topology.face(face)?;
        let surface = entity.surface().clone();
        let (point, du, dv) = surface.first_order(origin.x, origin.y)?;
        let x = du.normalize();
        let y = du.cross(dv).normalize().cross(x);
        let edit_plane = Plane3::new(point, x, y, tolerance)?;
        let (sx, sy) = (du.magnitude(), dv.magnitude());
        let chart = Plane3::new(
            Point3::new(-origin.x * sx, -origin.y * sy, 0.0),
            Vec3::unit_x() * sx,
            Vec3::unit_y() * sy,
            tolerance,
        )?;
        let boundary = boundary
            .iter()
            .map(|segment| {
                let count = if matches!(segment.pcurve, Curve2::Line(_)) {
                    1
                } else {
                    64
                };
                sample_pcurve(segment, count).map(|samples| {
                    samples
                        .into_iter()
                        .map(|(_, uv)| {
                            let point = chart.point_at(uv);
                            Point2::new(point.x, point.y)
                        })
                        .collect()
                })
            })
            .collect::<KernelResult<_>>()?;
        Ok(Self {
            surface,
            orientation: entity.orientation(),
            face,
            tolerance,
            chart,
            edit_plane,
            origin,
            boundary,
        })
    }

    pub fn edit_plane(&self) -> Plane3 {
        self.edit_plane
    }

    pub fn boundary(&self) -> &[Vec<Point2>] {
        &self.boundary
    }

    fn uv(&self, point: Point2) -> KernelResult<Point2> {
        self.chart.coordinates(Point3::new(point.x, point.y, 0.0))
    }

    pub fn point_normal(&self, point: Point2) -> KernelResult<(Point3, Vec3)> {
        let uv = self.uv(point)?;
        let (point, _, _) = self.surface.first_order(uv.x, uv.y)?;
        let normal = self.surface.normal(uv.x, uv.y)? * self.orientation.sign();
        Ok((point, normal))
    }

    pub fn project(&self, point: Point3) -> KernelResult<Point2> {
        let projection = self.surface.project(point, self.tolerance)?;
        let uv = super::nearest_uv(
            self.surface.domains(),
            projection.parameters.into(),
            self.origin,
        );
        let point = self.chart.point_at(uv);
        Ok(Point2::new(point.x, point.y))
    }

    fn parameter_curve(&self, segment: &TrimmedCurve2) -> KernelResult<TrimmedCurve2> {
        let tolerance = self.tolerance;
        // Isoparametric edges use the varying surface coordinate directly.
        // This preserves analytic lines, circles and spline sections.
        if let Curve2::Line(line) = segment.curve() {
            if line.direction().y == 0.0 {
                let start = self.uv(segment.start()?)?;
                let end = self.uv(segment.end()?)?;
                return TrimmedCurve2::new(
                    Line2::new(Point2::new(0.0, start.y), Vec2::unit_x())?,
                    ParameterRange::new(start.x, end.x)?,
                );
            }
            if line.direction().x == 0.0 {
                let start = self.uv(segment.start()?)?;
                let end = self.uv(segment.end()?)?;
                return TrimmedCurve2::new(
                    Line2::new(Point2::new(start.x, 0.0), Vec2::unit_y())?,
                    ParameterRange::new(start.y, end.y)?,
                );
            }
        }
        let flat = Plane3::xy();
        let spatial = match segment.curve() {
            Curve2::Line(_) | Curve2::Circle(_) | Curve2::Nurbs(_) => {
                flat.lift_trimmed_curve(segment, tolerance)?
            }
            curve => {
                SurfaceCurve3::new(flat.into(), curve.clone(), segment.range(), tolerance)?.into()
            }
        };
        let curve = self
            .chart
            .parameterized_curve_image(&spatial, segment.range(), tolerance)?;
        TrimmedCurve2::new(curve, segment.range())
    }

    pub fn lift_curve(&self, segment: &TrimmedCurve2) -> KernelResult<TrimmedCurve3> {
        let segment = self.parameter_curve(segment)?;
        let range = segment.range();
        TrimmedCurve3::new(
            self.surface
                .boundary_curve(segment.curve().clone(), range, self.tolerance)?,
            range,
        )
    }

    pub fn tessellate_curve(
        &self,
        segment: &TrimmedCurve2,
        options: TessellationOptions,
    ) -> KernelResult<Polyline> {
        let lifted = self.lift_curve(segment)?;
        super::tessellation::tessellate_curve(lifted.curve(), lifted.range(), options)
    }

    pub fn tessellate_profile(
        &self,
        profile: &Profile,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        let rings = self.parameter_rings(profile)?;
        let mut region = self.cap(self.surface.clone(), &rings)?;
        if self.orientation == FaceOrientation::Reversed {
            region.reverse();
        }
        super::tessellation::tessellate_surface_region(&region, self.face, self.tolerance, options)
    }

    /// Sweep every point of a chart region along the supporting face normal.
    /// The end caps share the chart and the walls interpolate corresponding
    /// points on the two offsets, so each wall ruling follows a face normal.
    pub fn extrude(&self, profile: &Profile, start: f64, end: f64) -> KernelResult<Compound> {
        let tolerance = self.tolerance;
        if !start.is_finite() || !end.is_finite() || (end - start).abs() <= tolerance.linear {
            return Err(invalid("normal extrusion needs a nonzero finite depth"));
        }
        let start = start * self.orientation.sign();
        let end = end * self.orientation.sign();
        let offset = |distance| {
            if distance == 0.0 {
                Ok(self.surface.clone())
            } else {
                self.surface.offset(distance, tolerance)
            }
        };
        let rings = self.parameter_rings(profile)?;
        let mut first = self.cap(offset(start)?, &rings)?;
        let mut last = self.cap(offset(end)?, &rings)?;
        let (_, du, dv) = self.surface.first_order(self.origin.x, self.origin.y)?;
        let normal = self.surface.normal(self.origin.x, self.origin.y)?;
        let forward = du.cross(dv).dot(normal) * (end - start) > 0.0;
        let mut walls = Vec::new();
        let mut roles = vec![CreationRole::ExtrusionStart, CreationRole::ExtrusionEnd];
        for (ring, wire) in std::iter::once(profile.outer())
            .chain(profile.holes())
            .enumerate()
        {
            let boundary = if ring == 0 {
                ProfileBoundary::Outer
            } else {
                ProfileBoundary::Hole(ring - 1)
            };
            for ((a, b), (segment, part)) in first.loops[ring]
                .segments
                .iter()
                .zip(&last.loops[ring].segments)
                .zip(prepared_wire_segment_sources(wire))
            {
                walls.push(self.wall(&first.surface, &last.surface, a, b, forward)?);
                roles.push(CreationRole::ExtrusionSide {
                    boundary,
                    segment: profile.generator(ring, segment),
                    part,
                });
            }
        }
        if forward {
            first.reverse();
        } else {
            last.reverse();
        }
        let mut faces = vec![first, last];
        faces.extend(walls);
        publish_with_roles(faces, roles, tolerance)
    }

    fn parameter_rings(&self, profile: &Profile) -> KernelResult<Vec<Vec<TrimmedCurve2>>> {
        let rings = std::iter::once(profile.outer())
            .chain(profile.holes())
            .map(|wire| {
                prepared_wire_segments(wire)?
                    .iter()
                    .map(|segment| self.parameter_curve(segment))
                    .collect()
            })
            .collect::<KernelResult<Vec<Vec<_>>>>()?;
        let bounds = parameter_bounds(
            rings[0]
                .iter()
                .map(|segment| (segment.curve(), segment.range())),
            self.tolerance,
        )?;
        for (axis, domain) in self.surface.domains().into_iter().enumerate() {
            if let Some(period) = domain.period {
                if bounds[axis].span() >= period - self.tolerance.parameter {
                    return Err(invalid(
                        "a sketch region must span less than one surface period",
                    ));
                }
            } else if !domain.contains_range(bounds[axis], self.tolerance.parameter * 64.0) {
                return Err(invalid(
                    "the sketch region extends outside the surface parameter domain",
                ));
            }
        }
        Ok(rings)
    }

    fn cap(&self, surface: Surface3, rings: &[Vec<TrimmedCurve2>]) -> KernelResult<BrepPatch> {
        let loops = rings
            .iter()
            .enumerate()
            .map(|(index, ring)| {
                let segments = ring
                    .iter()
                    .map(|section| {
                        let range = section.range();
                        let pcurve = section.curve().clone();
                        let curve =
                            surface.boundary_curve(pcurve.clone(), range, self.tolerance)?;
                        Ok(BrepPatchSegment {
                            provenance: Default::default(),
                            curve,
                            pcurve,
                            range,
                        })
                    })
                    .collect::<KernelResult<_>>()?;
                Ok(BrepPatchLoop {
                    kind: if index == 0 {
                        LoopKind::Outer
                    } else {
                        LoopKind::Inner
                    },
                    segments,
                })
            })
            .collect::<KernelResult<_>>()?;
        Ok(BrepPatch {
            sources: Vec::new(),
            preserved: Vec::new(),
            surface,
            orientation: FaceOrientation::Forward,
            loops,
        })
    }

    fn wall(
        &self,
        first: &Surface3,
        last: &Surface3,
        a: &BrepPatchSegment,
        b: &BrepPatchSegment,
        forward: bool,
    ) -> KernelResult<BrepPatch> {
        let tolerance = self.tolerance;
        let range = a.range;
        let rail = |u| -> KernelResult<Curve3> {
            let start = a.curve.first_order(u)?.0;
            let end = b.curve.first_order(u)?.0;
            Ok(Line3::new(start, end - start)?.into())
        };
        let mut segments = [
            BrepPatchSegment {
                curve: a.curve.clone(),
                range,
                provenance: Default::default(),
                pcurve: Line2::new(Point2::new(0.0, 0.0), Vec2::unit_x())?.into(),
            },
            BrepPatchSegment {
                curve: rail(range.end)?,
                range: ParameterRange::new(0.0, 1.0)?,
                provenance: Default::default(),
                pcurve: Line2::new(Point2::new(range.end, 0.0), Vec2::unit_y())?.into(),
            },
            BrepPatchSegment {
                curve: b.curve.clone(),
                range: range.reversed(),
                provenance: Default::default(),
                pcurve: Line2::new(Point2::new(0.0, 1.0), Vec2::unit_x())?.into(),
            },
            BrepPatchSegment {
                curve: rail(range.start)?,
                range: ParameterRange::new(1.0, 0.0)?,
                provenance: Default::default(),
                pcurve: Line2::new(Point2::new(range.start, 0.0), Vec2::unit_y())?.into(),
            },
        ];
        let mut orientation = if range.span() > 0.0 {
            FaceOrientation::Forward
        } else {
            FaceOrientation::Reversed
        };
        // The walls of a rectangle on a cylinder lie in axial or radial planes.
        // Retain those planes for analytic Boolean intersections with the body.
        let plane = if matches!(first, Surface3::Cylinder(_)) {
            match (&a.curve, &segments[3].curve) {
                (Curve3::Line(line), Curve3::Line(rail)) => Some(Plane3::new(
                    line.origin(),
                    line.direction().normalize(),
                    rail.direction().normalize(),
                    tolerance,
                )?),
                (Curve3::Circle(circle), _) => Some(Plane3::new(
                    circle.center(),
                    circle.x_radius().normalize(),
                    circle.y_radius().normalize(),
                    tolerance,
                )?),
                _ => None,
            }
        } else {
            None
        };
        let surface = if let Some(plane) = plane {
            let tangent = a.curve.first_order(range.middle())?.1;
            let rise =
                b.curve.first_order(range.middle())?.0 - a.curve.first_order(range.middle())?.0;
            if plane.normal().dot(tangent.cross(rise)) < 0.0 {
                orientation = orientation.reversed();
            }
            for segment in &mut segments {
                segment.pcurve =
                    plane.parameterized_curve_image(&segment.curve, segment.range, tolerance)?;
            }
            plane.into()
        } else {
            let depth = (b.curve.first_order(range.start)?.0 - a.curve.first_order(range.start)?.0)
                .magnitude();
            super::EdgeBlendSurface3::with_section(
                a.curve.clone(),
                first.clone(),
                last.clone(),
                a.pcurve.clone(),
                b.pcurve.clone(),
                range,
                depth,
                tolerance,
                true,
            )?
            .with_center_period(None)
            .into()
        };
        let mut wall = BrepPatch {
            sources: Vec::new(),
            preserved: Vec::new(),
            surface,
            orientation,
            loops: vec![BrepPatchLoop {
                kind: LoopKind::Outer,
                segments: segments.into(),
            }],
        };
        if !forward {
            wall.reverse();
        }
        Ok(wall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Circle2, Wire2};

    fn circle(center: Point2, radius: f64, tolerance: ModelTolerance) -> KernelResult<Profile> {
        let curve = TrimmedCurve2::new(
            Circle2::new(center, radius)?,
            ParameterRange::new(0.0, std::f64::consts::TAU)?,
        )?;
        Profile::new(Wire2::new([curve], tolerance)?, [], tolerance)
    }

    #[test]
    fn cuts_a_constant_depth_pocket_from_a_cylinder() -> KernelResult<()> {
        let tolerance = ModelTolerance::default();
        let cylinder = circle(Point2::origin(), 10.0, tolerance)?
            .extrude(Plane3::xy(), Vec3::unit_z() * 20.0)?;
        let face = cylinder
            .faces()
            .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .unwrap();
        let sketch = face.surface_sketch()?;
        let points = [(-3.0, -4.0), (3.0, -4.0), (3.0, 4.0), (-3.0, 4.0)].map(Point2::from);
        let profile = Profile::new(Wire2::polygon(points, tolerance)?, [], tolerance)?;
        let mesh = sketch.tessellate_profile(&profile, TessellationOptions::default())?;
        assert!(mesh.triangles.len() > 2);
        assert!(mesh
            .positions
            .iter()
            .all(|point| (point.x.hypot(point.y) - 10.0).abs() < tolerance.linear));

        let tool = sketch.extrude(&profile, 0.0, -2.0)?;
        let result = cylinder.difference(&tool)?;
        let expected = 0.5 * (100.0 - 64.0) * 0.6 * 8.0;
        assert!((cylinder.volume() - result.volume() - expected).abs() < 1e-6);
        assert!(result.faces().any(|face| matches!(face.surface(), Surface3::Cylinder(s) if (s.radius() - 8.0).abs() < tolerance.linear)));
        Ok(())
    }

    #[test]
    fn adds_a_circular_boss_to_a_cylinder() -> KernelResult<()> {
        let tolerance = ModelTolerance::default();
        let cylinder = circle(Point2::origin(), 10.0, tolerance)?
            .extrude(Plane3::xy(), Vec3::unit_z() * 20.0)?;
        let face = cylinder
            .faces()
            .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .unwrap();
        let sketch = face.surface_sketch()?;
        let profile = circle(Point2::origin(), 3.0, tolerance)?;
        let tool = sketch.extrude(&profile, 0.0, 2.0)?;
        let result = cylinder.union(&tool)?;
        let expected = std::f64::consts::PI * 9.0 * 2.2;
        assert!((result.volume() - cylinder.volume() - expected).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn cuts_a_rectangular_chart_region_from_a_sphere() -> KernelResult<()> {
        let tolerance = ModelTolerance::default();
        let mut kernel = super::super::Kernel::new(tolerance)?;
        let mut transaction = kernel.begin_operation();
        transaction.sphere(
            Point3::origin(),
            Vec3::unit_x() * 10.0,
            Vec3::unit_y() * 10.0,
            Vec3::unit_z() * 10.0,
        )?;
        let committed = transaction.finish()?;
        let sphere = Compound::from_operation(kernel, committed, [])?;
        let sketch = sphere.faces().next().unwrap().surface_sketch()?;
        let points = [(-3.0, -4.0), (3.0, -4.0), (3.0, 4.0), (-3.0, 4.0)].map(Point2::from);
        let profile = Profile::new(Wire2::polygon(points, tolerance)?, [], tolerance)?;
        let tool = sketch
            .extrude(&profile, 0.0, -2.0)
            .expect("sew spherical extrusion");
        let result = sphere.difference(&tool)?;
        let expected = (1000.0 - 512.0) / 3.0 * 0.6 * 2.0 * 0.4_f64.sin();
        assert!((sphere.volume() - result.volume() - expected).abs() < 1e-6);
        Ok(())
    }
}
