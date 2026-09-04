use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2};

use super::euler::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::{
    Compound, Curve2, Curve3, DiagnosticKind, FaceOrientation, IntersectionSet, Kernel,
    KernelError, KernelResult, LoopKind, ModelTolerance, ParameterRange, Surface3,
    TessellationOptions, TriangleMesh,
};

/// Material-set operation requested by the high-level exact BREP Boolean API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Union,
    Intersection,
    Difference,
}

#[derive(Debug, Clone)]
struct FaceCurve {
    curve: Curve3,
    pcurve: Curve2,
    range: ParameterRange,
    loop_end: bool,
    loop_kind: LoopKind,
    parameter_seam: bool,
}

#[derive(Debug, Clone)]
struct DirectedPiece {
    curve: Curve3,
    pcurve: Curve2,
    range: ParameterRange,
    start_node: usize,
    end_node: usize,
    start_uv: Point2,
    end_uv: Point2,
}

impl DirectedPiece {
    fn reversed(&self) -> Self {
        Self {
            curve: self.curve.clone(),
            pcurve: self.pcurve.clone(),
            range: self.range.reversed(),
            start_node: self.end_node,
            end_node: self.start_node,
            start_uv: self.end_uv,
            end_uv: self.start_uv,
        }
    }
}

#[derive(Clone, Copy)]
struct ClassificationMesh<'a> {
    mesh: &'a TriangleMesh,
    faces: &'a [ClassificationFace],
    tolerance: ModelTolerance,
    chord: f64,
}

struct ClassificationFace {
    surface: Surface3,
    orientation: FaceOrientation,
    boundary: Vec<FaceCurve>,
    bounds: [ParameterRange; 2],
}

impl ClassificationMesh<'_> {
    fn contains(self, point: Point3) -> bool {
        // A chord mesh can put a point on the wrong side of a curved support.
        // Within its error envelope, classify against the nearest exact face.
        let mut nearest = None::<(f64, bool)>;
        for face in self.faces {
            if !matches!(
                face.surface,
                Surface3::Plane(_) | Surface3::Cylinder(_) | Surface3::Torus(_)
            ) {
                continue;
            }
            let Ok(projection) = face.surface.project(point, self.tolerance) else {
                continue;
            };
            if projection.distance > self.chord * 2.0
                || nearest.is_some_and(|(d, _)| d <= projection.distance)
            {
                continue;
            }
            let mut uv = Point2::new(projection.parameters.0, projection.parameters.1);
            for axis in 0..2 {
                if let Some(period) = face.surface.domains()[axis].period {
                    let center = (face.bounds[axis].start + face.bounds[axis].end) * 0.5;
                    uv[axis] += ((center - uv[axis]) / period).round() * period;
                }
            }
            if !point_in_trim(uv, &face.boundary).unwrap_or(false) {
                continue;
            }
            let Ok(value) = face.surface.evaluate(uv.x, uv.y) else {
                continue;
            };
            let Ok(normal) = value.normal() else {
                continue;
            };
            let sign = if face.orientation == FaceOrientation::Forward {
                1.0
            } else {
                -1.0
            };
            nearest = Some((
                projection.distance,
                (point - value.point).dot(normal) * sign <= 0.0,
            ));
        }
        if let Some((_, inside)) = nearest {
            return inside;
        }
        let mut solid_angle = 0.0;
        for triangle in &self.mesh.triangles {
            let a = self.mesh.positions[triangle[0]] - point;
            let b = self.mesh.positions[triangle[1]] - point;
            let c = self.mesh.positions[triangle[2]] - point;
            let lengths = [a.magnitude(), b.magnitude(), c.magnitude()];
            if lengths.iter().any(|length| *length <= f64::EPSILON) {
                return true;
            }
            let numerator = a.dot(b.cross(c));
            let denominator = lengths[0] * lengths[1] * lengths[2]
                + a.dot(b) * lengths[2]
                + b.dot(c) * lengths[0]
                + c.dot(a) * lengths[1];
            solid_angle += 2.0 * numerator.atan2(denominator);
        }
        solid_angle.abs() > std::f64::consts::TAU
    }
}

fn occupied(kind: BooleanKind, first: bool, second: bool) -> bool {
    match kind {
        BooleanKind::Union => first || second,
        BooleanKind::Intersection => first && second,
        BooleanKind::Difference => first && !second,
    }
}

/// Feature-neutral regularized Boolean evaluation over two arbitrary BREP
/// boundaries.  Every retained face keeps its original analytical support;
/// newly introduced edges retain both intersected surfaces through an implicit
/// [`Curve3::SurfaceIntersection`].
pub(crate) fn boolean_brep(
    first: &Compound,
    second: &Compound,
    kind: BooleanKind,
) -> KernelResult<Compound> {
    let tolerance = first.tolerance();
    let scale = first
        .vertices()
        .chain(second.vertices())
        .map(|vertex| vertex.point().to_vec().magnitude())
        .fold(1.0_f64, f64::max);
    // Tessellation is used only for point classification, never as output
    // geometry.  Keeping it appreciably coarser than modeling tolerance avoids
    // turning analytical circles and blends into enormous classification
    // meshes while the eight-chord offset stays clear of its error envelope.
    let chord = (tolerance.linear_at_scale(scale) * 32.0).max(scale * 5.0e-4);
    let options = TessellationOptions::new(chord, 0.12, 12)?;
    let first_mesh = first.tessellate(options)?;
    let second_mesh = second.tessellate(options)?;
    let classification_offset = chord * 8.0;

    let first_faces = first.faces().cloned().collect::<Vec<_>>();
    let second_faces = second.faces().cloned().collect::<Vec<_>>();
    let mut face_curves = BTreeMap::<(usize, usize), Vec<FaceCurve>>::new();
    for (operand, faces, compound) in [
        (0_usize, first_faces.as_slice(), first),
        (1_usize, second_faces.as_slice(), second),
    ] {
        for (index, face) in faces.iter().enumerate() {
            face_curves.insert(
                (operand, index),
                boundary_curves(compound, face.topology_id())?,
            );
        }
    }
    let original_curves = face_curves.clone();
    let classification_faces = [(first, &first_faces), (second, &second_faces)]
        .into_iter()
        .enumerate()
        .map(|(operand, (compound, faces))| {
            faces
                .iter()
                .enumerate()
                .map(|(index, face)| {
                    let boundary = original_curves[&(operand, index)].clone();
                    Ok(ClassificationFace {
                        surface: face.surface().clone(),
                        orientation: compound.topology().face(face.topology_id())?.orientation(),
                        bounds: face_parameter_bounds(&boundary, tolerance)?,
                        boundary,
                    })
                })
                .collect::<KernelResult<Vec<_>>>()
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let classifiers = [
        ClassificationMesh {
            mesh: &first_mesh,
            faces: &classification_faces[0],
            tolerance,
            chord,
        },
        ClassificationMesh {
            mesh: &second_mesh,
            faces: &classification_faces[1],
            tolerance,
            chord,
        },
    ];

    for (first_index, first_face) in first_faces.iter().enumerate() {
        for (second_index, second_face) in second_faces.iter().enumerate() {
            let first_boundary = &original_curves[&(0, first_index)];
            let second_boundary = &original_curves[&(1, second_index)];
            if face_bounds_separated(first_boundary, second_boundary, tolerance)? {
                continue;
            }
            let first_bounds = face_parameter_bounds(first_boundary, tolerance)?;
            let second_bounds = face_parameter_bounds(second_boundary, tolerance)?;
            let intersections = first_face.surface().intersect_surface_in_bounds(
                second_face.surface(),
                first_bounds,
                second_bounds,
                tolerance,
            )?;
            let IntersectionSet::Isolated(intersections) = intersections else {
                continue;
            };
            for intersection in intersections {
                let first_pcurve = align_pcurve_to_face_chart(
                    &intersection.first_pcurve,
                    first_face.surface(),
                    first_boundary,
                    tolerance,
                )?;
                let second_pcurve = align_pcurve_to_face_chart(
                    &intersection.second_pcurve,
                    second_face.surface(),
                    second_boundary,
                    tolerance,
                )?;
                let trimmed = trim_to_faces(
                    &intersection.curve,
                    &first_pcurve,
                    first_boundary,
                    &second_pcurve,
                    second_boundary,
                    tolerance,
                )?;
                for range in trimmed {
                    let first_curve = boundary_curve_for_intersection(
                        &intersection.curve,
                        range,
                        first_boundary,
                        tolerance,
                    )?
                    .unwrap_or_else(|| FaceCurve {
                        curve: intersection.curve.clone(),
                        pcurve: first_pcurve.clone(),
                        range,
                        loop_end: false,
                        loop_kind: LoopKind::Outer,
                        parameter_seam: false,
                    });
                    let second_curve = boundary_curve_for_intersection(
                        &intersection.curve,
                        range,
                        second_boundary,
                        tolerance,
                    )?
                    .unwrap_or_else(|| FaceCurve {
                        curve: intersection.curve.clone(),
                        pcurve: second_pcurve.clone(),
                        range,
                        loop_end: false,
                        loop_kind: LoopKind::Outer,
                        parameter_seam: false,
                    });
                    face_curves
                        .get_mut(&(0, first_index))
                        .expect("face curve map is complete")
                        .push(first_curve);
                    face_curves
                        .get_mut(&(1, second_index))
                        .expect("face curve map is complete")
                        .push(second_curve);
                }
            }
        }
    }

    // Coincident supports form one arrangement even when their source faces
    // have different trim loops.  Overlay both complete boundary graphs on
    // each support; otherwise a union of coplanar faces would leave each
    // operand holding only an open fragment of the result outline.
    for (first_index, first_face) in first_faces.iter().enumerate() {
        for (second_index, second_face) in second_faces.iter().enumerate() {
            let coincident = first_face
                .surface()
                .intersect_surface(second_face.surface(), tolerance)
                .is_ok_and(|intersection| matches!(intersection, IntersectionSet::Coincident));
            if !coincident {
                continue;
            }
            let first_boundary = &original_curves[&(0, first_index)];
            let second_boundary = &original_curves[&(1, second_index)];
            for source in second_boundary {
                let projected: Curve2 = if first_face.surface() == second_face.surface() {
                    source.pcurve.clone()
                } else {
                    super::SurfaceProjectionPcurve2::new(
                        source.curve.clone(),
                        first_face.surface().clone(),
                        source.pcurve.clone(),
                        source.range,
                        tolerance,
                    )?
                    .into()
                };
                let projected = align_pcurve_to_face_chart(
                    &projected,
                    first_face.surface(),
                    first_boundary,
                    tolerance,
                )?;
                face_curves
                    .get_mut(&(0, first_index))
                    .expect("face curve map is complete")
                    .push(FaceCurve {
                        curve: source.curve.clone(),
                        pcurve: projected,
                        range: source.range,
                        loop_end: false,
                        loop_kind: source.loop_kind,
                        parameter_seam: source.parameter_seam,
                    });
            }
            for source in first_boundary {
                let projected: Curve2 = if first_face.surface() == second_face.surface() {
                    source.pcurve.clone()
                } else {
                    super::SurfaceProjectionPcurve2::new(
                        source.curve.clone(),
                        second_face.surface().clone(),
                        source.pcurve.clone(),
                        source.range,
                        tolerance,
                    )?
                    .into()
                };
                let projected = align_pcurve_to_face_chart(
                    &projected,
                    second_face.surface(),
                    second_boundary,
                    tolerance,
                )?;
                face_curves
                    .get_mut(&(1, second_index))
                    .expect("face curve map is complete")
                    .push(FaceCurve {
                        curve: source.curve.clone(),
                        pcurve: projected,
                        range: source.range,
                        loop_end: false,
                        loop_kind: source.loop_kind,
                        parameter_seam: source.parameter_seam,
                    });
            }
        }
    }

    let mut patches = Vec::new();
    for (operand, faces, compound) in [
        (0_usize, first_faces.as_slice(), first),
        (1_usize, second_faces.as_slice(), second),
    ] {
        let opposite_faces = if operand == 0 {
            second_faces.as_slice()
        } else {
            first_faces.as_slice()
        };
        for (index, face) in faces.iter().enumerate() {
            let original = boundary_curves(compound, face.topology_id())?;
            let has_coincident_support = opposite_faces.iter().any(|opposite| {
                face.surface()
                    .intersect_surface(opposite.surface(), tolerance)
                    .is_ok_and(|intersection| matches!(intersection, IntersectionSet::Coincident))
            });
            // Both operands' complete trim graphs were overlaid above.  Emit
            // the shared-support arrangement once, using the first operand as
            // the canonical parameterization, instead of publishing duplicate
            // coplanar faces from both source shells.
            if operand == 1 && has_coincident_support {
                continue;
            }
            patches.extend(arrange_face(
                operand,
                face.surface(),
                compound.topology().face(face.topology_id())?.orientation(),
                &original,
                &face_curves[&(operand, index)],
                has_coincident_support,
                classifiers,
                kind,
                classification_offset,
                tolerance,
            )?);
        }
    }
    deduplicate_patches(&mut patches, tolerance)?;
    // Let the Euler weld choose the simplest exact representation when more
    // than two supports meet on one edge (for example a planar line tangent to
    // a NURBS extrusion).  This is geometry canonicalization, independent of
    // operand or feature order.
    patches.sort_by_key(patch_complexity_key);
    let mut components = patch_components(patches, tolerance)?;
    for component in &mut components {
        component.sort_by_key(patch_complexity_key);
    }

    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::with_capacity(components.len());
    for component in &components {
        bodies.push(transaction.brep_shell(component)?);
    }
    transaction.nest_brep_shells(&bodies, options)?;
    let committed = transaction.finish()?;
    let role = match kind {
        BooleanKind::Union => "boolean.union",
        BooleanKind::Intersection => "boolean.intersection",
        BooleanKind::Difference => "boolean.difference",
    };
    Compound::from_operation(kernel, committed, [first, second])?
        .with_boolean_parents(first, second, role)
}

fn curve_complexity(curve: &Curve3) -> u8 {
    match curve {
        Curve3::Line(_) => 0,
        Curve3::Circle(_) => 1,
        Curve3::Nurbs(_) => 2,
        Curve3::SurfaceCurve(_) => 3,
        Curve3::SurfaceIntersection(_) => 4,
    }
}

fn patch_complexity_key(patch: &BrepPatch) -> (u8, u8) {
    let boundary_complexity = patch
        .loops
        .iter()
        .flat_map(|loop_| &loop_.segments)
        .map(|segment| curve_complexity(&segment.curve))
        .filter(|complexity| *complexity > 0)
        .min()
        .unwrap_or(0);
    let surface_complexity = match patch.surface {
        Surface3::Plane(_) => 0,
        Surface3::Cylinder(_) | Surface3::Cone(_) | Surface3::Sphere(_) | Surface3::Torus(_) => 1,
        Surface3::Revolved(_) => 2,
        Surface3::Nurbs(_) => 3,
        Surface3::Offset(_) | Surface3::RollingBall(_) => 4,
    };
    (boundary_complexity, surface_complexity)
}

fn align_pcurve_to_face_chart(
    pcurve: &Curve2,
    surface: &Surface3,
    boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<Curve2> {
    let mut bounds = [
        Point2::new(f64::INFINITY, f64::INFINITY),
        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    ];
    for segment in boundary {
        for (_, point) in sample_pcurve(segment, 8)? {
            bounds[0].x = bounds[0].x.min(point.x);
            bounds[0].y = bounds[0].y.min(point.y);
            bounds[1].x = bounds[1].x.max(point.x);
            bounds[1].y = bounds[1].y.max(point.y);
        }
    }
    let parameter = match pcurve.domain().bounds {
        super::DomainBounds::Bounded(range) => (range.start + range.end) * 0.5,
        _ => 0.0,
    };
    let point = pcurve.evaluate(parameter)?.point;
    let domains = surface.domains();
    let coordinates = [point.x, point.y];
    let centers = [
        (bounds[0].x + bounds[1].x) * 0.5,
        (bounds[0].y + bounds[1].y) * 0.5,
    ];
    let mut offset = Vec2::new(0.0, 0.0);
    for axis in 0..2 {
        if let Some(period) = domains[axis].period {
            let shift = ((centers[axis] - coordinates[axis]) / period).round() * period;
            if axis == 0 {
                offset.x = shift;
            } else {
                offset.y = shift;
            }
        }
    }
    if offset.magnitude2() <= tolerance.parameter.powi(2) {
        return Ok(pcurve.clone());
    }
    translate_pcurve(pcurve, offset, surface, tolerance)
}

fn translate_pcurve(
    pcurve: &Curve2,
    offset: Vec2,
    surface: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<Curve2> {
    match pcurve {
        Curve2::Line(line) => {
            Ok(super::Line2::new(line.origin() + offset, line.direction())?.into())
        }
        Curve2::Circle(circle) => {
            Ok(super::Circle2::new(circle.center() + offset, circle.radius())?.into())
        }
        Curve2::Nurbs(curve) => Ok(super::NurbsCurve2::new(
            curve.degree(),
            curve
                .control_points()
                .iter()
                .map(|point| *point + offset)
                .collect(),
            curve.weights().to_vec(),
            curve.knots().to_vec(),
        )?
        .into()),
        Curve2::SurfaceProjection(projection) => {
            let guide = translate_pcurve(projection.guide(), offset, surface, tolerance)?;
            Ok(super::SurfaceProjectionPcurve2::new(
                projection.curve().clone(),
                projection.surface().clone(),
                guide,
                projection.range(),
                tolerance,
            )?
            .into())
        }
        Curve2::SurfaceIntersection(intersection) => {
            let spatial = intersection.spatial_curve();
            let Curve3::SurfaceIntersection(curve) = &spatial else {
                unreachable!()
            };
            let guide = curve.pcurve_for_surface(surface).ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "intersection p-curve belongs to another support",
                )
            })?;
            let guide = translate_pcurve(guide, offset, surface, tolerance)?;
            let range = curve.range();
            Ok(super::SurfaceProjectionPcurve2::new(
                spatial,
                surface.clone(),
                guide,
                range,
                tolerance,
            )?
            .into())
        }
    }
}

fn face_parameter_bounds(
    boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for curve in boundary {
        for (_, point) in sample_pcurve(curve, 16)? {
            minimum.x = minimum.x.min(point.x);
            minimum.y = minimum.y.min(point.y);
            maximum.x = maximum.x.max(point.x);
            maximum.y = maximum.y.max(point.y);
        }
    }
    let expand = |minimum: f64, maximum: f64| {
        let padding = tolerance.parameter * 64.0;
        if maximum - minimum <= padding {
            (minimum - padding, maximum + padding)
        } else {
            (minimum, maximum)
        }
    };
    let u = expand(minimum.x, maximum.x);
    let v = expand(minimum.y, maximum.y);
    Ok([
        ParameterRange::new(u.0, u.1)?,
        ParameterRange::new(v.0, v.1)?,
    ])
}

fn face_bounds_separated(
    first: &[FaceCurve],
    second: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let bounds = |curves: &[FaceCurve]| -> KernelResult<[Point3; 2]> {
        let mut minimum = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut maximum = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for curve in curves {
            for index in 0..=16 {
                let parameter = curve.range.start + curve.range.span() * index as f64 / 16.0;
                let point = curve.curve.evaluate(parameter)?.point;
                minimum.x = minimum.x.min(point.x);
                minimum.y = minimum.y.min(point.y);
                minimum.z = minimum.z.min(point.z);
                maximum.x = maximum.x.max(point.x);
                maximum.y = maximum.y.max(point.y);
                maximum.z = maximum.z.max(point.z);
            }
        }
        Ok([minimum, maximum])
    };
    let first = bounds(first)?;
    let second = bounds(second)?;
    let scale = first
        .iter()
        .chain(&second)
        .map(|point| point.to_vec().magnitude())
        .fold(1.0_f64, f64::max);
    let linear = tolerance.linear_at_scale(scale) * 16.0;
    Ok(first[1].x < second[0].x - linear
        || second[1].x < first[0].x - linear
        || first[1].y < second[0].y - linear
        || second[1].y < first[0].y - linear
        || first[1].z < second[0].z - linear
        || second[1].z < first[0].z - linear)
}

fn boundary_curves(compound: &Compound, face: super::FaceId) -> KernelResult<Vec<FaceCurve>> {
    let topology = compound.topology();
    let surface = topology.face(face)?.surface();
    let domains = surface.domains();
    let mut curves = Vec::new();
    for loop_id in topology.face(face)?.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let coedges = topology.loop_coedges(*loop_id)?;
        let last = coedges.len().saturating_sub(1);
        // Keep the complete loop in one continuous periodic chart. Individual
        // coedges may use different principal copies of the same seam.
        let mut previous = None::<Point2>;
        for (index, coedge_id) in coedges.into_iter().enumerate() {
            let coedge = topology.coedge(coedge_id)?;
            let edge = topology.edge(coedge.edge())?;
            let mut pcurve = coedge.pcurve().clone();
            if let Some(previous) = previous {
                let start = pcurve.evaluate(coedge.range().start)?.point;
                let delta = previous - start;
                let offset = Vec2::new(
                    domains[0].period.map_or(0.0, |p| (delta.x / p).round() * p),
                    domains[1].period.map_or(0.0, |p| (delta.y / p).round() * p),
                );
                if offset.magnitude2() > compound.tolerance().parameter.powi(2) {
                    pcurve = translate_pcurve(&pcurve, offset, surface, compound.tolerance())?;
                }
            }
            previous = Some(pcurve.evaluate(coedge.range().end)?.point);
            curves.push(FaceCurve {
                curve: edge.curve().clone(),
                pcurve,
                range: coedge.range(),
                loop_end: index == last,
                loop_kind: loop_value.kind(),
                parameter_seam: topology.edge_faces(coedge.edge())?.len() == 1,
            });
        }
    }
    Ok(curves)
}

fn point_in_trim(point: Point2, boundary: &[FaceCurve]) -> KernelResult<bool> {
    let mut crossings = 0_u32;
    for curve in boundary {
        let samples = sample_pcurve(curve, 32)?;
        for pair in samples.windows(2) {
            let first = pair[0].1;
            let second = pair[1].1;
            if point_segment_distance(point, first, second) <= 1.0e-10 {
                return Ok(true);
            }
            if (first.y > point.y) != (second.y > point.y) {
                // The chords only bracket a ray crossing. Classifying against
                // their x coordinate can exclude an exact intersection lying
                // on a curved trim, leaving its neighboring face open.
                let mut low = pair[0].0;
                let mut high = pair[1].0;
                let mut crossing = first;
                for _ in 0..48 {
                    let middle = (low + high) * 0.5;
                    crossing = curve.pcurve.evaluate(middle)?.point;
                    if (crossing.y - point.y).abs() <= 1.0e-12 {
                        break;
                    }
                    if (crossing.y > point.y) == (first.y > point.y) {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                let x = crossing.x;
                if (x - point.x).abs() <= 1.0e-9 {
                    return Ok(true);
                }
                if x > point.x {
                    crossings += 1;
                }
            }
        }
    }
    Ok(crossings % 2 == 1)
}

fn sample_pcurve(curve: &FaceCurve, count: usize) -> KernelResult<Vec<(f64, Point2)>> {
    (0..=count)
        .map(|index| {
            let parameter =
                curve.range.start + curve.range.span() * index as f64 / count.max(1) as f64;
            Ok((parameter, curve.pcurve.evaluate(parameter)?.point))
        })
        .collect()
}

fn point_segment_distance(point: Point2, start: Point2, end: Point2) -> f64 {
    let direction = end - start;
    if direction.magnitude2() <= f64::EPSILON {
        return point.distance(start);
    }
    let parameter = (point - start).dot(direction) / direction.magnitude2();
    point.distance(start + direction * parameter.clamp(0.0, 1.0))
}

/// Reuse an existing exact face-boundary curve when a surface/surface
/// intersection runs along it. This is a representation simplification, not a
/// feature case: the boundary remains the same geometric set, while the
/// simpler curve avoids introducing a redundant implicit parameterization at
/// a three-surface junction.
fn boundary_curve_for_intersection(
    intersection: &Curve3,
    range: ParameterRange,
    boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<Option<FaceCurve>> {
    for candidate in boundary {
        let mut parameters = Vec::with_capacity(9);
        let mut matches = true;
        for index in 0..=8 {
            let parameter = range.start + range.span() * index as f64 / 8.0;
            let point = intersection.evaluate(parameter)?.point;
            let Ok(projection) = candidate.curve.project(point, tolerance) else {
                matches = false;
                break;
            };
            let mut projected = projection.parameter;
            if let Some(period) = candidate.curve.domain().period {
                let reference = parameters
                    .last()
                    .copied()
                    .unwrap_or((candidate.range.start + candidate.range.end) * 0.5);
                projected += ((reference - projected) / period).round() * period;
            }
            let allowed = tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0;
            if projection.distance > allowed || !candidate.range.contains(projected, tolerance) {
                matches = false;
                break;
            }
            parameters.push(projected);
        }
        if matches {
            let replacement_range = ParameterRange::new(
                *parameters
                    .first()
                    .expect("intersection samples are nonempty"),
                *parameters
                    .last()
                    .expect("intersection samples are nonempty"),
            )?;
            if replacement_range.span().abs() > tolerance.parameter {
                return Ok(Some(FaceCurve {
                    curve: candidate.curve.clone(),
                    pcurve: candidate.pcurve.clone(),
                    range: replacement_range,
                    loop_end: false,
                    loop_kind: candidate.loop_kind,
                    parameter_seam: candidate.parameter_seam,
                }));
            }
        }
    }
    Ok(None)
}

fn trim_to_faces(
    curve: &Curve3,
    first: &Curve2,
    first_boundary: &[FaceCurve],
    second: &Curve2,
    second_boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<ParameterRange>> {
    let finite_domain = |curve: &Curve2, boundary: &[FaceCurve]| -> KernelResult<ParameterRange> {
        if let super::DomainBounds::Bounded(range) = curve.domain().bounds {
            return Ok(range);
        }
        let Curve2::Line(line) = curve else {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "an unbounded surface-intersection p-curve must be linear",
            ));
        };
        let denominator = line.direction().magnitude2();
        let mut parameters = Vec::new();
        for segment in boundary {
            parameters.extend(
                sample_pcurve(segment, 8)?
                    .into_iter()
                    .map(|(_, point)| (point - line.origin()).dot(line.direction()) / denominator),
            );
        }
        let minimum = parameters.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = parameters.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let padding = (maximum - minimum).abs() * 0.05 + tolerance.parameter * 64.0;
        ParameterRange::new(minimum - padding, maximum + padding)
    };
    let first_natural = match first.domain().bounds {
        super::DomainBounds::Bounded(range) => Some(range),
        _ => None,
    };
    let second_natural = match second.domain().bounds {
        super::DomainBounds::Bounded(range) => Some(range),
        _ => None,
    };
    // The two p-curves share the spatial curve parameter.  An unbounded
    // p-curve (most commonly `u = t` on a periodic cylinder) must not shorten
    // the bounded domain supplied by its mate merely because the face's UV
    // chart has a different seam.
    let domain = match (first_natural, second_natural) {
        (Some(first), Some(second)) => {
            let start = first.start.min(first.end).max(second.start.min(second.end));
            let end = first.start.max(first.end).min(second.start.max(second.end));
            if end - start <= tolerance.parameter * 16.0 {
                return Ok(Vec::new());
            }
            ParameterRange::new(start, end)?
        }
        (Some(range), None) | (None, Some(range)) => range,
        (None, None) => {
            let first_domain = finite_domain(first, first_boundary)?;
            let second_domain = finite_domain(second, second_boundary)?;
            let start = first_domain
                .start
                .min(first_domain.end)
                .max(second_domain.start.min(second_domain.end));
            let end = first_domain
                .start
                .max(first_domain.end)
                .min(second_domain.start.max(second_domain.end));
            if end - start <= tolerance.parameter * 16.0 {
                return Ok(Vec::new());
            }
            ParameterRange::new(start, end)?
        }
    };
    let inside = |parameter: f64| -> KernelResult<bool> {
        Ok(
            point_in_trim(first.evaluate(parameter)?.point, first_boundary)?
                && point_in_trim(second.evaluate(parameter)?.point, second_boundary)?,
        )
    };
    let first_curve = FaceCurve {
        curve: curve.clone(),
        pcurve: first.clone(),
        range: domain,
        loop_end: false,
        loop_kind: LoopKind::Outer,
        parameter_seam: false,
    };
    let second_curve = FaceCurve {
        curve: curve.clone(),
        pcurve: second.clone(),
        range: domain,
        loop_end: false,
        loop_kind: LoopKind::Outer,
        parameter_seam: false,
    };
    let mut parameters = vec![domain.start, domain.end];
    for boundary in first_boundary {
        parameters.extend(
            curve_intersections(&first_curve, boundary, tolerance)?
                .into_iter()
                .map(|hit| hit.0),
        );
    }
    for boundary in second_boundary {
        parameters.extend(
            curve_intersections(&second_curve, boundary, tolerance)?
                .into_iter()
                .map(|hit| hit.0),
        );
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 64.0);
    let mut ranges = Vec::<ParameterRange>::new();
    for pair in parameters.windows(2) {
        let middle = (pair[0] + pair[1]) * 0.5;
        if pair[1] - pair[0] <= tolerance.parameter * 16.0 {
            continue;
        }
        let shifts: Vec<f64> = match curve.domain().period {
            Some(period) => vec![0.0, -period, period, -2.0 * period, 2.0 * period],
            None => vec![0.0],
        };
        // Select the equivalent parameter branch that lies directly in both
        // faces' UV charts.  This keeps later arrangement endpoints on the
        // same side of a periodic seam instead of relying on a wrapped
        // point-in-polygon test that cannot preserve edge connectivity.
        for shift in shifts {
            if inside(middle + shift)? {
                ranges.push(ParameterRange::new(pair[0] + shift, pair[1] + shift)?);
                break;
            }
        }
    }
    ranges.sort_by(|first, second| first.start.total_cmp(&second.start));
    let mut merged = Vec::<ParameterRange>::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut() {
            if (previous.end - range.start).abs() <= tolerance.parameter * 64.0 {
                previous.end = range.end;
                continue;
            }
        }
        merged.push(range);
    }
    Ok(merged)
}

fn curve_intersections(
    first: &FaceCurve,
    second: &FaceCurve,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<(f64, f64)>> {
    let endpoint_hits = || -> KernelResult<Vec<(f64, f64)>> {
        let fit = |curve: &Curve3, range: ParameterRange, point: Point3| -> Option<f64> {
            let projection = curve.project(point, tolerance).ok()?;
            let mut parameter = projection.parameter;
            if let Some(period) = curve.domain().period {
                let center = (range.start + range.end) * 0.5;
                parameter += ((center - parameter) / period).round() * period;
            }
            let allowed = tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0;
            (projection.distance <= allowed && range.contains(parameter, tolerance))
                .then_some(parameter.clamp(range.start.min(range.end), range.start.max(range.end)))
        };
        let mut hits = Vec::<(f64, f64)>::new();
        for first_parameter in [first.range.start, first.range.end] {
            let point = first.curve.evaluate(first_parameter)?.point;
            if let Some(second_parameter) = fit(&second.curve, second.range, point) {
                hits.push((first_parameter, second_parameter));
            }
        }
        for second_parameter in [second.range.start, second.range.end] {
            let point = second.curve.evaluate(second_parameter)?.point;
            if let Some(first_parameter) = fit(&first.curve, first.range, point) {
                hits.push((first_parameter, second_parameter));
            }
        }
        Ok(hits)
    };
    let deduplicate = |mut hits: Vec<(f64, f64)>| {
        hits.sort_by(|first, second| {
            first
                .0
                .total_cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1))
        });
        hits.dedup_by(|first, second| {
            (first.0 - second.0).abs() <= tolerance.parameter * 64.0
                && (first.1 - second.1).abs() <= tolerance.parameter * 64.0
        });
        hits
    };
    if matches!(first.curve, Curve3::Line(_) | Curve3::Circle(_))
        && matches!(second.curve, Curve3::Line(_) | Curve3::Circle(_))
    {
        match first.curve.intersect_curve(&second.curve, tolerance)? {
            IntersectionSet::Isolated(hits) => {
                let mut hits = hits
                    .into_iter()
                    .filter_map(|hit| {
                        let fit = |mut parameter: f64, curve: &Curve3, range: ParameterRange| {
                            if let Some(period) = curve.domain().period {
                                let center = (range.start + range.end) * 0.5;
                                parameter += ((center - parameter) / period).round() * period;
                            }
                            range.contains(parameter, tolerance).then_some(parameter)
                        };
                        fit(hit.first_parameter, &first.curve, first.range).zip(fit(
                            hit.second_parameter,
                            &second.curve,
                            second.range,
                        ))
                    })
                    .collect::<Vec<_>>();
                hits.extend(endpoint_hits()?);
                return Ok(deduplicate(hits));
            }
            IntersectionSet::Empty | IntersectionSet::Coincident => {
                // Coincident trims can cover different portions of the same
                // basis curve. Their overlap endpoints are arrangement
                // vertices just as much as transverse intersections are. If
                // they are not transferred to both parameter ranges, a long
                // edge and a shorter duplicate survive with different node
                // pairs and leave the selected boundary graph non-manifold.
                return Ok(deduplicate(endpoint_hits()?));
            }
        }
    }
    let first_samples = sample_pcurve(first, 32)?;
    let second_samples = sample_pcurve(second, 32)?;
    let mut hits = Vec::<(f64, f64)>::new();
    for first_pair in first_samples.windows(2) {
        for second_pair in second_samples.windows(2) {
            let Some((first_fraction, second_fraction)) = segment_intersection_parameters(
                first_pair[0].1,
                first_pair[1].1,
                second_pair[0].1,
                second_pair[1].1,
            ) else {
                continue;
            };
            let mut parameters = [
                first_pair[0].0
                    + (first_pair[1].0 - first_pair[0].0) * first_fraction.clamp(0.0, 1.0),
                second_pair[0].0
                    + (second_pair[1].0 - second_pair[0].0) * second_fraction.clamp(0.0, 1.0),
            ];
            for _ in 0..24 {
                let first_value = first.pcurve.first_order(parameters[0])?;
                let second_value = second.pcurve.first_order(parameters[1])?;
                let residual = first_value.0 - second_value.0;
                if residual.magnitude() <= tolerance.parameter * 16.0 {
                    break;
                }
                let determinant =
                    first_value.1.x * -second_value.1.y - first_value.1.y * -second_value.1.x;
                if determinant.abs() <= f64::EPSILON * 256.0 {
                    break;
                }
                let first_delta =
                    (-residual.x * -second_value.1.y - second_value.1.x * residual.y) / determinant;
                let second_delta =
                    (first_value.1.x * -residual.y + residual.x * first_value.1.y) / determinant;
                parameters[0] = (parameters[0] + first_delta).clamp(
                    first.range.start.min(first.range.end),
                    first.range.start.max(first.range.end),
                );
                parameters[1] = (parameters[1] + second_delta).clamp(
                    second.range.start.min(second.range.end),
                    second.range.start.max(second.range.end),
                );
            }
            let first_point = first.pcurve.evaluate(parameters[0])?.point;
            let second_point = second.pcurve.evaluate(parameters[1])?.point;
            if first_point.distance(second_point) <= tolerance.parameter * 64.0
                && !hits.iter().any(|(a, b)| {
                    (parameters[0] - *a).abs() <= tolerance.parameter * 64.0
                        && (parameters[1] - *b).abs() <= tolerance.parameter * 64.0
                })
            {
                hits.push((parameters[0], parameters[1]));
            }
        }
    }
    hits.extend(endpoint_hits()?);
    Ok(deduplicate(hits))
}

fn segment_intersection_parameters(
    a: Point2,
    b: Point2,
    c: Point2,
    d: Point2,
) -> Option<(f64, f64)> {
    let first = b - a;
    let second = d - c;
    let denominator = first.x * second.y - first.y * second.x;
    if denominator.abs() <= f64::EPSILON * 256.0 {
        return None;
    }
    let delta = c - a;
    let first_parameter = (delta.x * second.y - delta.y * second.x) / denominator;
    let second_parameter = (delta.x * first.y - delta.y * first.x) / denominator;
    ((-1.0e-9..=1.0 + 1.0e-9).contains(&first_parameter)
        && (-1.0e-9..=1.0 + 1.0e-9).contains(&second_parameter))
    .then_some((first_parameter, second_parameter))
}

fn arrange_face(
    operand: usize,
    surface: &Surface3,
    source_orientation: FaceOrientation,
    original_boundary: &[FaceCurve],
    curves: &[FaceCurve],
    has_coincident_support: bool,
    classifiers: [ClassificationMesh<'_>; 2],
    kind: BooleanKind,
    classification_offset: f64,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<BrepPatch>> {
    let mut uv_bounds = [
        Point2::new(f64::INFINITY, f64::INFINITY),
        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    ];
    for curve in original_boundary {
        for (_, point) in sample_pcurve(curve, 8)? {
            uv_bounds[0].x = uv_bounds[0].x.min(point.x);
            uv_bounds[0].y = uv_bounds[0].y.min(point.y);
            uv_bounds[1].x = uv_bounds[1].x.max(point.x);
            uv_bounds[1].y = uv_bounds[1].y.max(point.y);
        }
    }
    let uv_probe_limit = (uv_bounds[1].x - uv_bounds[0].x)
        .abs()
        .min((uv_bounds[1].y - uv_bounds[0].y).abs())
        * 5.0e-2;
    let classify_orientation_at = |uv: Point2| -> KernelResult<Option<FaceOrientation>> {
        let value = surface.evaluate(uv.x, uv.y)?;
        let parameter_normal = value.normal()?;
        let normal_probe = classification_offset;
        let below = value.point - parameter_normal * normal_probe;
        let above = value.point + parameter_normal * normal_probe;
        if has_coincident_support {
            let below_occupied = occupied(
                kind,
                classifiers[0].contains(below),
                classifiers[1].contains(below),
            );
            let above_occupied = occupied(
                kind,
                classifiers[0].contains(above),
                classifiers[1].contains(above),
            );
            let coincident_orientation = match (below_occupied, above_occupied) {
                (true, false) => Some(FaceOrientation::Forward),
                (false, true) => Some(FaceOrientation::Reversed),
                _ => None,
            };
            return Ok(coincident_orientation);
        }
        let other_contains = classifiers[1 - operand].contains(value.point);
        let direct = match (kind, operand) {
            (BooleanKind::Union, _) => (!other_contains).then_some(source_orientation),
            (BooleanKind::Intersection, _) => other_contains.then_some(source_orientation),
            (BooleanKind::Difference, 0) => (!other_contains).then_some(source_orientation),
            (BooleanKind::Difference, 1) => other_contains.then_some(source_orientation.reversed()),
            _ => unreachable!(),
        };
        Ok(direct)
    };
    let orientation_at = |uv: Point2| -> KernelResult<Option<FaceOrientation>> {
        if !has_coincident_support && !point_in_trim(uv, original_boundary)? {
            return Ok(None);
        }
        classify_orientation_at(uv)
    };

    if curves.len() == original_boundary.len() {
        // A periodic face can retain a same-face parameter seam in its loop.
        // That seam is not a material boundary, and its first listed use need
        // not have a meaningful "inside" side in the wrapped UV chart. Search
        // all boundary uses for a valid interior probe instead of allowing the
        // arbitrary loop start to decide whether an unchanged face survives.
        let mut desired = None;
        for boundary in original_boundary
            .iter()
            .filter(|boundary| !boundary.parameter_seam)
        {
            let parameter = boundary.range.start + boundary.range.span() * 0.5;
            let (boundary_uv, tangent) = boundary.pcurve.first_order(parameter)?;
            let tangent = tangent * boundary.range.span().signum();
            if tangent.magnitude2() <= tolerance.parameter.powi(2) {
                continue;
            }
            let mut inward = Vec2::new(-tangent.y, tangent.x).normalize();
            if source_orientation == FaceOrientation::Reversed {
                inward = -inward;
            }
            let surface_value = surface.evaluate(boundary_uv.x, boundary_uv.y)?;
            let world_inward = surface_value.du * inward.x + surface_value.dv * inward.y;
            if world_inward.magnitude2() <= tolerance.linear.powi(2) {
                continue;
            }
            let uv_step = (classification_offset / world_inward.magnitude()).min(uv_probe_limit);
            // This point is constructed from the oriented trim itself, so it
            // is the local interior by definition. Periodic seam-spiked loops
            // are not necessarily representable as one planar polygon and
            // therefore must not be rejected by `point_in_trim` here.
            if let Some(orientation) = classify_orientation_at(boundary_uv + inward * uv_step)? {
                desired = Some(orientation);
                break;
            }
        }
        return match desired {
            None => Ok(Vec::new()),
            Some(orientation) => Ok(vec![unchanged_face_patch(
                surface,
                original_boundary,
                source_orientation,
                orientation,
                tolerance,
            )?]),
        };
    }

    let mut split_parameters = curves
        .iter()
        .map(|curve| vec![curve.range.start, curve.range.end])
        .collect::<Vec<_>>();
    for first in 0..curves.len() {
        for second in first + 1..curves.len() {
            for (first_parameter, second_parameter) in
                curve_intersections(&curves[first], &curves[second], tolerance)?
            {
                split_parameters[first].push(first_parameter);
                split_parameters[second].push(second_parameter);
            }
        }
    }

    let mut nodes = Vec::<Point2>::new();
    let mut pieces = Vec::<DirectedPiece>::new();
    let weld_node = |nodes: &mut Vec<Point2>, point: Point2| -> KernelResult<usize> {
        let value = surface.evaluate(point.x, point.y)?;
        let linear = tolerance.linear_at_scale(value.point.to_vec().magnitude());
        if let Some(index) = nodes.iter().position(|candidate| {
            let delta = *candidate - point;
            // Intersection solvers converge in model units, not UV units.
            // Use the local surface metric while keeping distinct periodic
            // chart endpoints (whose raw UV separation is a full period).
            candidate.distance(point) <= tolerance.parameter * 64.0
                || (value.du * delta.x + value.dv * delta.y).magnitude() <= linear
        }) {
            Ok(index)
        } else {
            nodes.push(point);
            Ok(nodes.len() - 1)
        }
    };
    for (curve, mut parameters) in curves.iter().zip(split_parameters) {
        for parameter in &mut parameters {
            *parameter = parameter.clamp(
                curve.range.start.min(curve.range.end),
                curve.range.start.max(curve.range.end),
            );
        }
        parameters.sort_by(f64::total_cmp);
        parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 32.0);
        if curve.range.span() < 0.0 {
            parameters.reverse();
        }
        for pair in parameters.windows(2) {
            if (pair[1] - pair[0]).abs() <= tolerance.parameter * 32.0 {
                continue;
            }
            let range = ParameterRange::new(pair[0], pair[1])?;
            let start = curve.curve.evaluate(range.start)?.point;
            let end = curve.curve.evaluate(range.end)?.point;
            let start_uv = curve.pcurve.evaluate(range.start)?.point;
            let end_uv = curve.pcurve.evaluate(range.end)?.point;
            if start.distance(end) <= tolerance.linear * 8.0
                && range.span().abs() < curve.curve.domain().period.unwrap_or(f64::INFINITY) * 0.5
            {
                continue;
            }
            let start_node = weld_node(&mut nodes, start_uv)?;
            let end_node = weld_node(&mut nodes, end_uv)?;
            pieces.push(DirectedPiece {
                curve: curve.curve.clone(),
                pcurve: curve.pcurve.clone(),
                range,
                start_node,
                end_node,
                start_uv,
                end_uv,
            });
        }
    }

    let mut selected = [Vec::<DirectedPiece>::new(), Vec::<DirectedPiece>::new()];
    for piece in &pieces {
        let parameter = piece.range.start + piece.range.span() * 0.5;
        let (mut uv, tangent) = piece.pcurve.first_order(parameter)?;
        if matches!(piece.curve, Curve3::SurfaceIntersection(_)) {
            // A tracing spline selects the intersection branch but is not the
            // intersection itself. Center side probes on the corrected curve.
            let point = piece.curve.evaluate(parameter)?.point;
            let projection = surface.project(point, tolerance)?;
            let mut exact = Point2::new(projection.parameters.0, projection.parameters.1);
            for axis in 0..2 {
                if let Some(period) = surface.domains()[axis].period {
                    exact[axis] += ((uv[axis] - exact[axis]) / period).round() * period;
                }
            }
            uv = exact;
        }
        let tangent = tangent * piece.range.span().signum();
        if tangent.magnitude2() <= tolerance.parameter.powi(2) {
            continue;
        }
        let left = Vec2::new(-tangent.y, tangent.x).normalize();
        let surface_value = surface.evaluate(uv.x, uv.y)?;
        let world_left = surface_value.du * left.x + surface_value.dv * left.y;
        if world_left.magnitude2() <= tolerance.linear.powi(2) {
            continue;
        }
        let mut probe = classification_offset;
        for curve in curves {
            let Ok(projection) = curve.curve.project(surface_value.point, tolerance) else {
                continue;
            };
            let mut parameter = projection.parameter;
            if let Some(period) = curve.curve.domain().period {
                parameter += (((curve.range.start + curve.range.end) * 0.5 - parameter) / period)
                    .round()
                    * period;
            }
            if projection.distance > tolerance.linear * 16.0
                && curve.range.contains(parameter, tolerance)
            {
                probe = probe.min(projection.distance * 0.25);
            }
        }
        let uv_step = (probe / world_left.magnitude()).min(uv_probe_limit);
        let left_orientation = orientation_at(uv + left * uv_step)?;
        let right_orientation = orientation_at(uv - left * uv_step)?;
        for (slot, orientation) in [FaceOrientation::Forward, FaceOrientation::Reversed]
            .into_iter()
            .enumerate()
        {
            match orientation {
                FaceOrientation::Forward => {
                    if left_orientation == Some(orientation)
                        && right_orientation != Some(orientation)
                    {
                        selected[slot].push(piece.clone());
                    } else if right_orientation == Some(orientation)
                        && left_orientation != Some(orientation)
                    {
                        selected[slot].push(piece.reversed());
                    }
                }
                FaceOrientation::Reversed => {
                    if right_orientation == Some(orientation)
                        && left_orientation != Some(orientation)
                    {
                        selected[slot].push(piece.clone());
                    } else if left_orientation == Some(orientation)
                        && right_orientation != Some(orientation)
                    {
                        selected[slot].push(piece.reversed());
                    }
                }
            }
        }
    }

    let mut patches = Vec::new();
    for (slot, orientation) in [FaceOrientation::Forward, FaceOrientation::Reversed]
        .into_iter()
        .enumerate()
    {
        let mut unique = Vec::<DirectedPiece>::new();
        for piece in std::mem::take(&mut selected[slot]) {
            let middle = piece
                .curve
                .evaluate(piece.range.start + piece.range.span() * 0.5)?
                .point;
            let duplicate = unique.iter().any(|candidate| {
                let same_nodes = (candidate.start_node == piece.start_node
                    && candidate.end_node == piece.end_node)
                    || (candidate.start_node == piece.end_node
                        && candidate.end_node == piece.start_node);
                let projected_match =
                    candidate
                        .curve
                        .project(middle, tolerance)
                        .is_ok_and(|projection| {
                            let mut parameter = projection.parameter;
                            if let Some(period) = candidate.curve.domain().period {
                                let center = (candidate.range.start + candidate.range.end) * 0.5;
                                parameter += ((center - parameter) / period).round() * period;
                            }
                            projection.distance <= tolerance.linear * 16.0
                                && candidate.range.contains(parameter, tolerance)
                        });
                let candidate_middle = candidate
                    .curve
                    .evaluate(candidate.range.start + candidate.range.span() * 0.5);
                let piece_middle = piece
                    .curve
                    .evaluate(piece.range.start + piece.range.span() * 0.5);
                let sampled_match = match (candidate_middle, piece_middle) {
                    (Ok(first), Ok(second)) => {
                        first.point.distance(second.point) <= classification_offset * 0.25
                            && first
                                .first_derivative
                                .normalize()
                                .dot(second.first_derivative.normalize())
                                .abs()
                                >= 1.0 - tolerance.angular * 16.0
                    }
                    _ => false,
                };
                same_nodes && (projected_match || sampled_match)
            });
            if !duplicate {
                unique.push(piece);
            }
        }
        add_periodic_seams(&mut unique, &pieces, surface, tolerance)?;
        let loops = connect_loops(unique, tolerance, surface, orientation)?;
        if loops.is_empty() {
            continue;
        }
        let orientation_sign: f64 = if orientation == FaceOrientation::Forward {
            1.0
        } else {
            -1.0
        };
        let mut classified = Vec::<(Vec<DirectedPiece>, Vec<Point2>, f64, bool)>::new();
        for mut loop_ in loops {
            let area = loop_.iter().try_fold(0.0, |sum, piece| {
                Ok::<_, KernelError>(
                    sum + piece
                        .pcurve
                        .signed_area_contribution(piece.range, tolerance.parameter)?,
                )
            })?;
            if area.abs() <= tolerance.parameter.powi(2) {
                continue;
            }
            let polygon = loop_polyline(&loop_)?;
            let sample = Point2::from_vec(
                polygon
                    .iter()
                    .fold(Vec2::new(0.0, 0.0), |sum, point| sum + point.to_vec())
                    / polygon.len() as f64,
            );
            let depth = classified
                .iter()
                .filter(|(_, enclosing, enclosing_area, _)| {
                    enclosing_area.abs() > area.abs() && point_in_polygon(sample, enclosing)
                })
                .count();
            let outer = depth % 2 == 0;
            let desired_sign = orientation_sign * if outer { 1.0 } else { -1.0 };
            if area.signum() != desired_sign.signum() {
                loop_.reverse();
                loop_ = loop_.into_iter().map(|piece| piece.reversed()).collect();
            }
            classified.push((loop_, polygon, area, outer));
        }
        classified.sort_by(|first, second| second.2.abs().total_cmp(&first.2.abs()));
        let mut outers = classified
            .iter()
            .enumerate()
            .filter(|(_, value)| value.3)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for outer_index in outers.drain(..) {
            let outer = &classified[outer_index].0;
            let outer_polygon = &classified[outer_index].1;
            let mut patch_loops = vec![BrepPatchLoop {
                kind: LoopKind::Outer,
                segments: patch_segments(outer, surface),
            }];
            for (hole, _, _, is_outer) in &classified {
                if *is_outer {
                    continue;
                }
                let sample = hole[0].pcurve.evaluate(hole[0].range.start)?.point;
                if point_in_polygon(sample, outer_polygon) {
                    patch_loops.push(BrepPatchLoop {
                        kind: LoopKind::Inner,
                        segments: patch_segments(hole, surface),
                    });
                }
            }
            patches.push(BrepPatch {
                surface: surface.clone(),
                orientation,
                loops: patch_loops,
            });
        }
    }
    Ok(patches)
}

/// A face that spans a complete period needs a chart seam even though that
/// seam is not a material boundary. Reuse unselected arrangement pieces only
/// when they pair the dangling ends of the selected boundary graph. The two
/// parameter-space copies weld to one spatial Euler edge with opposite uses.
fn add_periodic_seams(
    selected: &mut Vec<DirectedPiece>,
    arrangement: &[DirectedPiece],
    surface: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let domains = surface.domains();
    if !domains.iter().any(|domain| domain.period.is_some()) {
        return Ok(());
    }
    let mut degree = BTreeMap::<usize, usize>::new();
    for piece in selected.iter() {
        *degree.entry(piece.start_node).or_default() += 1;
        *degree.entry(piece.end_node).or_default() += 1;
    }
    for candidate in arrangement {
        if degree.get(&candidate.start_node).copied().unwrap_or(0) % 2 == 0
            || degree.get(&candidate.end_node).copied().unwrap_or(0) % 2 == 0
            || selected.iter().any(|piece| {
                (piece.start_node == candidate.start_node && piece.end_node == candidate.end_node)
                    || (piece.start_node == candidate.end_node
                        && piece.end_node == candidate.start_node)
            })
        {
            continue;
        }
        selected.push(candidate.clone());
        *degree.entry(candidate.start_node).or_default() += 1;
        *degree.entry(candidate.end_node).or_default() += 1;
    }
    let dangling = degree
        .iter()
        .filter_map(|(node, degree)| (*degree % 2 == 1).then_some(*node))
        .collect::<Vec<_>>();
    for first in 0..dangling.len() {
        if degree[&dangling[first]] % 2 == 0 {
            continue;
        }
        for second in first + 1..dangling.len() {
            if degree[&dangling[second]] % 2 == 0 {
                continue;
            }
            let node_uv = |node| {
                selected.iter().find_map(|piece| {
                    if piece.start_node == node {
                        Some(piece.start_uv)
                    } else if piece.end_node == node {
                        Some(piece.end_uv)
                    } else {
                        None
                    }
                })
            };
            let Some(first_uv) = node_uv(dangling[first]) else {
                continue;
            };
            let Some(second_uv) = node_uv(dangling[second]) else {
                continue;
            };
            let shifted = arrangement.iter().find_map(|candidate| {
                let offset = periodic_chart_offset(
                    candidate.start_uv,
                    first_uv,
                    domains,
                    tolerance.parameter * 64.0,
                )?;
                let end_offset = periodic_chart_offset(
                    candidate.end_uv,
                    second_uv,
                    domains,
                    tolerance.parameter * 64.0,
                )?;
                ((end_offset - offset).magnitude() <= tolerance.parameter * 64.0)
                    .then_some((candidate, offset))
            });
            let Some((candidate, offset)) = shifted else {
                continue;
            };
            let Curve2::Line(line) = candidate.pcurve else {
                continue;
            };
            let mut seam = candidate.clone();
            seam.pcurve = super::Line2::new(line.origin() + offset, line.direction())?.into();
            seam.start_node = dangling[first];
            seam.end_node = dangling[second];
            seam.start_uv = first_uv;
            seam.end_uv = second_uv;
            selected.push(seam);
            *degree.entry(dangling[first]).or_default() += 1;
            *degree.entry(dangling[second]).or_default() += 1;
            break;
        }
    }
    Ok(())
}

fn periodic_chart_offset(
    from: Point2,
    to: Point2,
    domains: [super::ParameterDomain; 2],
    tolerance: f64,
) -> Option<Vec2> {
    let delta = to - from;
    let mut offset = Vec2::new(0.0, 0.0);
    for (axis, value) in [delta.x, delta.y].into_iter().enumerate() {
        let shifted = if let Some(period) = domains[axis].period {
            let multiple = (value / period).round();
            ((value - multiple * period).abs() <= tolerance).then_some(multiple * period)?
        } else {
            (value.abs() <= tolerance).then_some(0.0)?
        };
        if axis == 0 {
            offset.x = shifted;
        } else {
            offset.y = shifted;
        }
    }
    Some(offset)
}

fn unchanged_face_patch(
    surface: &Surface3,
    boundary: &[FaceCurve],
    source_orientation: FaceOrientation,
    output_orientation: FaceOrientation,
    _tolerance: ModelTolerance,
) -> KernelResult<BrepPatch> {
    let mut loops = Vec::<(LoopKind, Vec<BrepPatchSegment>)>::new();
    let mut current = Vec::new();
    let mut current_kind = None;
    for segment in boundary {
        current_kind.get_or_insert(segment.loop_kind);
        current.push(BrepPatchSegment {
            curve: segment.curve.clone(),
            pcurve: exact_pcurve_on_surface(&segment.curve, surface, &segment.pcurve),
            range: segment.range,
        });
        if segment.loop_end {
            loops.push((
                current_kind.take().expect("a BREP loop has a kind"),
                std::mem::take(&mut current),
            ));
        }
    }
    if !current.is_empty() {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "an unchanged BREP face contains an open trim loop",
        ));
    }
    let reverse = output_orientation != source_orientation;
    let mut patch_loops = Vec::new();
    for (kind, mut segments) in loops {
        if reverse {
            segments.reverse();
            for segment in &mut segments {
                segment.range = segment.range.reversed();
            }
        }
        patch_loops.push(BrepPatchLoop { kind, segments });
    }
    Ok(BrepPatch {
        surface: surface.clone(),
        orientation: output_orientation,
        loops: patch_loops,
    })
}

fn connect_loops(
    pieces: Vec<DirectedPiece>,
    tolerance: ModelTolerance,
    surface: &Surface3,
    orientation: FaceOrientation,
) -> KernelResult<Vec<Vec<DirectedPiece>>> {
    let domains = surface.domains();
    let equivalent = |first: Point2, second: Point2| {
        domains[0].equivalent(first.x, second.x, tolerance.parameter * 64.0)
            && domains[1].equivalent(first.y, second.y, tolerance.parameter * 64.0)
    };
    let mut unused = (0..pieces.len()).collect::<BTreeSet<_>>();
    let mut loops = Vec::new();
    while let Some(start_index) = unused.iter().next().copied() {
        unused.remove(&start_index);
        let start_node = pieces[start_index].start_node;
        let start_uv = pieces[start_index].start_uv;
        let mut end_node = pieces[start_index].end_node;
        let mut end_uv = pieces[start_index].end_uv;
        let mut loop_ = vec![pieces[start_index].clone()];
        while end_node != start_node {
            let candidates = unused
                .iter()
                .copied()
                .filter(|index| {
                    pieces[*index].start_node == end_node || pieces[*index].end_node == end_node
                })
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                if equivalent(end_uv, start_uv) {
                    break;
                }
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    format!(
                        "imprinted Boolean face boundary is open on {surface:?} ({orientation:?}) at node {end_node} ({} pieces, {} unused; edges {:?})",
                        pieces.len(),
                        unused.len(),
                        pieces
                            .iter()
                            .map(|piece| {
                                (
                                    piece.start_node,
                                    piece.end_node,
                                    piece.start_uv,
                                    piece.end_uv,
                                )
                            })
                            .collect::<Vec<_>>()
                    ),
                ));
            }
            let previous = loop_.last().expect("loop has a first piece");
            let previous_tangent = previous
                .pcurve
                .first_order(previous.range.end - previous.range.span() * 1.0e-7)?
                .1
                * previous.range.span().signum();
            let previous_tangent = previous_tangent.normalize();
            let next_index = candidates
                .into_iter()
                .min_by(|first, second| {
                    let turn = |index: usize| {
                        let candidate = if pieces[index].start_node == end_node {
                            pieces[index].clone()
                        } else {
                            pieces[index].reversed()
                        };
                        let tangent = candidate
                            .pcurve
                            .first_order(candidate.range.start + candidate.range.span() * 1.0e-7)
                            .map(|value| (value.1 * candidate.range.span().signum()).normalize())
                            .unwrap_or(Vec2::unit_x());
                        previous_tangent
                            .x
                            .mul_add(tangent.y, -previous_tangent.y * tangent.x)
                            .atan2(previous_tangent.dot(tangent))
                            .rem_euclid(std::f64::consts::TAU)
                    };
                    turn(*first).total_cmp(&turn(*second))
                })
                .expect("nonempty candidate set");
            unused.remove(&next_index);
            let next = if pieces[next_index].start_node == end_node {
                pieces[next_index].clone()
            } else {
                pieces[next_index].reversed()
            };
            end_node = next.end_node;
            end_uv = next.end_uv;
            loop_.push(next);
            if loop_.len() > pieces.len() {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "imprinted Boolean face boundary did not close",
                ));
            }
        }
        loops.push(loop_);
    }
    Ok(loops)
}

fn patch_segments(loop_: &[DirectedPiece], surface: &Surface3) -> Vec<BrepPatchSegment> {
    loop_
        .iter()
        .map(|piece| BrepPatchSegment {
            curve: piece.curve.clone(),
            pcurve: exact_pcurve_on_surface(&piece.curve, surface, &piece.pcurve),
            range: piece.range,
        })
        .collect()
}

fn exact_pcurve_on_surface(curve: &Curve3, surface: &Surface3, fallback: &Curve2) -> Curve2 {
    match curve {
        Curve3::SurfaceIntersection(intersection) => {
            let exact = intersection.exact_pcurves();
            if intersection.first_surface() == surface {
                exact[0].clone()
            } else if intersection.second_surface() == surface {
                exact[1].clone()
            } else {
                fallback.clone()
            }
        }
        Curve3::SurfaceCurve(curve) if curve.surface() == surface => curve.pcurve().clone(),
        _ => fallback.clone(),
    }
}

fn loop_polyline(loop_: &[DirectedPiece]) -> KernelResult<Vec<Point2>> {
    let mut polygon = Vec::new();
    for piece in loop_ {
        for index in 0..8 {
            let parameter = piece.range.start + piece.range.span() * index as f64 / 8.0;
            polygon.push(piece.pcurve.evaluate(parameter)?.point);
        }
    }
    Ok(polygon)
}

fn point_in_polygon(point: Point2, polygon: &[Point2]) -> bool {
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        if (first.y > point.y) != (second.y > point.y)
            && point.x < (second.x - first.x) * (point.y - first.y) / (second.y - first.y) + first.x
        {
            inside = !inside;
        }
    }
    inside
}

fn patch_edge_samples(patch: &BrepPatch) -> KernelResult<Vec<[Point3; 3]>> {
    patch
        .loops
        .iter()
        .flat_map(|loop_| &loop_.segments)
        .map(|segment| {
            Ok([
                segment.curve.evaluate(segment.range.start)?.point,
                segment
                    .curve
                    .evaluate(segment.range.start + segment.range.span() * 0.5)?
                    .point,
                segment.curve.evaluate(segment.range.end)?.point,
            ])
        })
        .collect()
}

fn same_unoriented_edge(first: [Point3; 3], second: [Point3; 3], linear: f64) -> bool {
    let first_closed = first[0].distance(first[2]) <= linear;
    let second_closed = second[0].distance(second[2]) <= linear;
    if first_closed && second_closed {
        let first_center = Point3::from_vec((first[0].to_vec() + first[1].to_vec()) * 0.5);
        let second_center = Point3::from_vec((second[0].to_vec() + second[1].to_vec()) * 0.5);
        let first_radius = first[0].distance(first_center);
        let second_radius = second[0].distance(second_center);
        if first_center.distance(second_center) <= linear
            && (first_radius - second_radius).abs() <= linear
        {
            return true;
        }
    }
    first[1].distance(second[1]) <= linear
        && ((first[0].distance(second[0]) <= linear && first[2].distance(second[2]) <= linear)
            || (first[0].distance(second[2]) <= linear && first[2].distance(second[0]) <= linear))
}

fn deduplicate_patches(
    patches: &mut Vec<BrepPatch>,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let samples = patches
        .iter()
        .map(patch_edge_samples)
        .collect::<KernelResult<Vec<_>>>()?;
    let mut duplicate = BTreeSet::new();
    for first in 0..patches.len() {
        if duplicate.contains(&first) {
            continue;
        }
        for second in first + 1..patches.len() {
            if patches[first].orientation != patches[second].orientation {
                continue;
            }
            if !coincident_supports(&patches[first].surface, &patches[second].surface, tolerance)? {
                continue;
            }
            if patch_contains_patch(&patches[first], &samples[second], tolerance)? {
                duplicate.insert(second);
            } else if patch_contains_patch(&patches[second], &samples[first], tolerance)? {
                duplicate.insert(first);
                break;
            }
        }
    }
    let mut index = 0_usize;
    patches.retain(|_| {
        let retain = !duplicate.contains(&index);
        index += 1;
        retain
    });
    Ok(())
}

fn coincident_supports(
    first: &Surface3,
    second: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    if first == second {
        return Ok(true);
    }
    match (first, second) {
        (Surface3::Plane(first), Surface3::Plane(second)) => {
            Ok(
                first.normal().dot(second.normal()).abs() >= 1.0 - tolerance.angular * 8.0
                    && first.project(second.origin())?.distance
                        <= tolerance.linear_at_scale(second.origin().to_vec().magnitude()) * 16.0,
            )
        }
        (Surface3::Cylinder(first), Surface3::Cylinder(second)) => {
            let axes_parallel = first
                .axis()
                .normalize()
                .dot(second.axis().normalize())
                .abs()
                >= 1.0 - tolerance.angular * 8.0;
            let radial_offset = (second.origin() - first.origin())
                .cross(first.axis().normalize())
                .magnitude();
            Ok(axes_parallel
                && radial_offset <= tolerance.linear * 16.0
                && (first.radius() - second.radius()).abs() <= tolerance.linear * 16.0)
        }
        _ => Ok(false),
    }
}

fn patch_contains_patch(
    container: &BrepPatch,
    candidate_edges: &[[Point3; 3]],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let boundary = container
        .loops
        .iter()
        .flat_map(|loop_| &loop_.segments)
        .map(|segment| FaceCurve {
            curve: segment.curve.clone(),
            pcurve: segment.pcurve.clone(),
            range: segment.range,
            loop_end: false,
            loop_kind: LoopKind::Outer,
            parameter_seam: false,
        })
        .collect::<Vec<_>>();
    for point in candidate_edges.iter().flatten() {
        let projection = container.surface.project(*point, tolerance)?;
        if projection.distance > tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0
            || !point_in_trim(
                Point2::new(projection.parameters.0, projection.parameters.1),
                &boundary,
            )?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn patch_components(
    patches: Vec<BrepPatch>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<Vec<BrepPatch>>> {
    if patches.is_empty() {
        return Ok(Vec::new());
    }
    let samples = patches
        .iter()
        .map(patch_edge_samples)
        .collect::<KernelResult<Vec<_>>>()?;
    let linear = tolerance.linear * 32.0;
    let mut remaining = (0..patches.len()).collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(seed) = remaining.iter().next().copied() {
        remaining.remove(&seed);
        let mut stack = vec![seed];
        let mut component = vec![seed];
        while let Some(current) = stack.pop() {
            let connected = remaining
                .iter()
                .copied()
                .filter(|candidate| {
                    samples[current].iter().any(|first| {
                        samples[*candidate]
                            .iter()
                            .any(|second| same_unoriented_edge(*first, *second, linear))
                    })
                })
                .collect::<Vec<_>>();
            for candidate in connected {
                remaining.remove(&candidate);
                component.push(candidate);
                stack.push(candidate);
            }
        }
        components.push(
            component
                .into_iter()
                .map(|index| patches[index].clone())
                .collect(),
        );
    }
    Ok(components)
}
