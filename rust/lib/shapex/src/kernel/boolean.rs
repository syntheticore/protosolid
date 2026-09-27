use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2};

use super::euler::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
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
    provenance: SegmentProvenance,
    curve: Curve3,
    pcurve: Curve2,
    range: ParameterRange,
    loop_end: bool,
    loop_kind: LoopKind,
    parameter_seam: bool,
}

#[derive(Debug, Clone)]
struct DirectedPiece {
    provenance: SegmentProvenance,
    curve: Curve3,
    pcurve: Curve2,
    range: ParameterRange,
    start_node: usize,
    end_node: usize,
    start_uv: Point2,
    end_uv: Point2,
    parameter_seam: bool,
}

impl DirectedPiece {
    fn reversed(&self) -> Self {
        Self {
            provenance: self.provenance.reversed(),
            curve: self.curve.clone(),
            pcurve: self.pcurve.clone(),
            range: self.range.reversed(),
            start_node: self.end_node,
            end_node: self.start_node,
            start_uv: self.end_uv,
            end_uv: self.start_uv,
            parameter_seam: self.parameter_seam,
        }
    }
}

#[derive(Clone, Copy)]
struct OperandClassifier<'a> {
    compound: &'a Compound,
    mesh: &'a OnceCell<KernelResult<TriangleMesh>>,
    options: TessellationOptions,
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

impl OperandClassifier<'_> {
    fn sides_on_support(
        self,
        support: &Surface3,
        point: Point3,
        normal: crate::Vec3,
    ) -> KernelResult<(bool, bool)> {
        let mut covered = false;
        let mut sides = (false, false);
        for face in self.faces {
            if face.surface != *support
                && !face
                    .surface
                    .intersect_surface(support, self.tolerance)
                    .is_ok_and(|set| matches!(set, IntersectionSet::Coincident))
            {
                continue;
            }
            let projection = face.surface.project(point, self.tolerance)?;
            let mut uv = Point2::new(projection.parameters.0, projection.parameters.1);
            for axis in 0..2 {
                if let Some(period) = face.surface.domains()[axis].period {
                    let center = (face.bounds[axis].start + face.bounds[axis].end) * 0.5;
                    uv[axis] += ((center - uv[axis]) / period).round() * period;
                }
            }
            if !point_in_trim(uv, &face.boundary)? {
                continue;
            }
            let orientation = if face.orientation == FaceOrientation::Forward {
                1.0
            } else {
                -1.0
            };
            let forward = face.surface.normal(uv.x, uv.y)?.dot(normal) * orientation > 0.0;
            sides.0 |= forward;
            sides.1 |= !forward;
            covered = true;
        }
        if covered {
            Ok(sides)
        } else {
            let inside = self.contains(point)?;
            Ok((inside, inside))
        }
    }
    fn contains(self, point: Point3) -> KernelResult<bool> {
        // A chord mesh can put a point on the wrong side of a curved support.
        // Within its error envelope, classify against the nearest exact face.
        let mut nearest = None::<(f64, bool)>;
        for face in self.faces {
            if !matches!(
                face.surface,
                Surface3::Plane(_)
                    | Surface3::Cylinder(_)
                    | Surface3::Cone(_)
                    | Surface3::Torus(_)
                    | Surface3::Nurbs(_)
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
            return Ok(inside);
        }
        // Geometry queries above need no mesh. Build the fallback at most once
        // per operand and operation, only when those queries are inconclusive.
        let mesh = self
            .mesh
            .get_or_init(|| self.compound.tessellate(self.options))
            .as_ref()
            .map_err(Clone::clone)?;
        let mut solid_angle = 0.0;
        for triangle in &mesh.triangles {
            let a = mesh.positions[triangle[0]] - point;
            let b = mesh.positions[triangle[1]] - point;
            let c = mesh.positions[triangle[2]] - point;
            let lengths = [a.magnitude(), b.magnitude(), c.magnitude()];
            if lengths.iter().any(|length| *length <= f64::EPSILON) {
                return Ok(true);
            }
            let numerator = a.dot(b.cross(c));
            let denominator = lengths[0] * lengths[1] * lengths[2]
                + a.dot(b) * lengths[2]
                + b.dot(c) * lengths[0]
                + c.dot(a) * lengths[1];
            solid_angle += 2.0 * numerator.atan2(denominator);
        }
        Ok(solid_angle.abs() > std::f64::consts::TAU)
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
    boolean_brep_with_face_boundaries(first, second, kind, false)
}

// Sections need the boundary between an existing coincident face and a new
// cut cap, even when both regions have the same support and orientation.
pub(crate) fn boolean_brep_with_face_boundaries(
    first: &Compound,
    second: &Compound,
    kind: BooleanKind,
    preserve_first_faces: bool,
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
    let first_mesh = OnceCell::new();
    let second_mesh = OnceCell::new();
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
        OperandClassifier {
            compound: first,
            mesh: &first_mesh,
            options,
            faces: &classification_faces[0],
            tolerance,
            chord,
        },
        OperandClassifier {
            compound: second,
            mesh: &second_mesh,
            options,
            faces: &classification_faces[1],
            tolerance,
            chord,
        },
    ];

    for (first_index, first_face) in first_faces.iter().enumerate() {
        for (second_index, second_face) in second_faces.iter().enumerate() {
            let first_boundary = &original_curves[&(0, first_index)];
            let second_boundary = &original_curves[&(1, second_index)];
            if face_bounds_separated(
                first_face.surface(),
                first_boundary,
                second_face.surface(),
                second_boundary,
                tolerance,
            )? {
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
                    None,
                    first_face.surface(),
                    first_boundary,
                    tolerance,
                )?;
                let second_pcurve = align_pcurve_to_face_chart(
                    &intersection.second_pcurve,
                    None,
                    second_face.surface(),
                    second_boundary,
                    tolerance,
                )?;
                let first_copies = intersection_chart_copies(
                    &intersection.curve,
                    &first_pcurve,
                    first_face.surface(),
                    second_face.surface(),
                    first_boundary,
                    tolerance,
                )?;
                let second_copies = intersection_chart_copies(
                    &intersection.curve,
                    &second_pcurve,
                    second_face.surface(),
                    first_face.surface(),
                    second_boundary,
                    tolerance,
                )?;
                for first_pcurve in &first_copies {
                    for second_pcurve in &second_copies {
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
                                provenance: Default::default(),
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
                                provenance: Default::default(),
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
        }
    }

    // Coincident supports form one arrangement even when their source faces
    // have different trim loops.  Overlay both complete boundary graphs on
    // each support; otherwise a union of coplanar faces would leave each
    // operand holding only an open fragment of the result outline.
    // Include intersections imprinted by other faces, not just original trim
    // edges. The opposite shared-support face is skipped during publication,
    // so omitting its intersections loses material boundaries (for example
    // where a torus crosses its start plane again after half a turn).
    // Intersections already represented by either trim boundary are excluded:
    // projecting them again can introduce differently subdivided duplicates.
    let imprinted_curves = face_curves.clone();
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
            for (index, source) in imprinted_curves[&(1, second_index)].iter().enumerate() {
                if index >= second_boundary.len()
                    && (boundary_curve_for_intersection(
                        &source.curve,
                        source.range,
                        first_boundary,
                        tolerance,
                    )?
                    .is_some()
                        || boundary_curve_for_intersection(
                            &source.curve,
                            source.range,
                            second_boundary,
                            tolerance,
                        )?
                        .is_some())
                {
                    continue;
                }
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
                    Some(source.range),
                    first_face.surface(),
                    first_boundary,
                    tolerance,
                )?;
                face_curves
                    .get_mut(&(0, first_index))
                    .expect("face curve map is complete")
                    .push(FaceCurve {
                        provenance: source.provenance.clone(),
                        curve: source.curve.clone(),
                        pcurve: projected,
                        range: source.range,
                        loop_end: false,
                        loop_kind: source.loop_kind,
                        parameter_seam: source.parameter_seam,
                    });
            }
            for (index, source) in imprinted_curves[&(0, first_index)].iter().enumerate() {
                if index >= first_boundary.len()
                    && (boundary_curve_for_intersection(
                        &source.curve,
                        source.range,
                        first_boundary,
                        tolerance,
                    )?
                    .is_some()
                        || boundary_curve_for_intersection(
                            &source.curve,
                            source.range,
                            second_boundary,
                            tolerance,
                        )?
                        .is_some())
                {
                    continue;
                }
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
                    Some(source.range),
                    second_face.surface(),
                    second_boundary,
                    tolerance,
                )?;
                face_curves
                    .get_mut(&(1, second_index))
                    .expect("face curve map is complete")
                    .push(FaceCurve {
                        provenance: source.provenance.clone(),
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
            let mut combined_curves = Vec::new();
            if has_coincident_support {
                // A shell can have several disjoint faces on this support.
                // Arrange their combined boundaries once, or each face can
                // independently publish the opposite operand's end cap.
                let same_support = |other: &super::FaceHandle| {
                    face.surface()
                        .intersect_surface(other.surface(), tolerance)
                        .is_ok_and(|set| matches!(set, IntersectionSet::Coincident))
                };
                if faces[..index].iter().any(same_support) {
                    continue;
                }
                for (other_index, other) in faces.iter().enumerate() {
                    if same_support(other) {
                        for curve in &face_curves[&(operand, other_index)] {
                            let mut curve = curve.clone();
                            if other.surface() != face.surface() {
                                curve.pcurve = super::SurfaceProjectionPcurve2::new(
                                    curve.curve.clone(),
                                    face.surface().clone(),
                                    curve.pcurve.clone(),
                                    curve.range,
                                    tolerance,
                                )?
                                .into();
                            }
                            if other_index != index {
                                curve.pcurve = align_pcurve_to_face_chart(
                                    &curve.pcurve,
                                    Some(curve.range),
                                    face.surface(),
                                    &original,
                                    tolerance,
                                )?;
                            }
                            combined_curves.push(curve);
                        }
                    }
                }
            }
            let exclude_existing_faces = preserve_first_faces && has_coincident_support;
            let mut arranged = Vec::new();
            if exclude_existing_faces {
                // A coincident half-space retains each existing face boundary.
                // Arrange the remaining cut cap separately.
                for (index, original_face) in first_faces.iter().enumerate() {
                    if !coincident_supports(face.surface(), original_face.surface(), tolerance)? {
                        continue;
                    }
                    let boundary = &original_curves[&(0, index)];
                    arranged.extend(arrange_face(
                        0,
                        original_face.surface(),
                        first
                            .topology()
                            .face(original_face.topology_id())?
                            .orientation(),
                        boundary,
                        boundary,
                        true,
                        false,
                        classifiers,
                        kind,
                        classification_offset,
                        tolerance,
                    )?);
                }
            }
            arranged.extend(arrange_face(
                operand,
                face.surface(),
                compound.topology().face(face.topology_id())?.orientation(),
                &original,
                if has_coincident_support {
                    &combined_curves
                } else {
                    &face_curves[&(operand, index)]
                },
                has_coincident_support,
                exclude_existing_faces,
                classifiers,
                kind,
                classification_offset,
                tolerance,
            )?);
            for patch in &mut arranged {
                if has_coincident_support {
                    // A shared-support arrangement can contain several disjoint
                    // source faces. Attribute only the trimmed regions it uses.
                    for parent in [first, second] {
                        for candidate in parent.faces() {
                            if coincident_supports(&patch.surface, candidate.surface(), tolerance)?
                                && patch_uses_face(patch, parent, candidate, tolerance)?
                            {
                                patch.sources.push(candidate.persistent_id());
                                if patch
                                    .retains_boundary(parent.topology(), candidate.topology_id())?
                                {
                                    patch.preserved.push(candidate.persistent_id());
                                }
                            }
                        }
                    }
                } else {
                    patch.sources.push(face.persistent_id());
                    if patch.retains_boundary(compound.topology(), face.topology_id())? {
                        patch.preserved.push(face.persistent_id());
                    }
                }
            }
            patches.extend(arranged);
        }
    }
    if preserve_first_faces {
        for patch in &mut patches {
            let Surface3::Plane(plane) = &patch.surface else {
                continue;
            };
            for segment in patch.loops.iter_mut().flat_map(|loop_| &mut loop_.segments) {
                if let Curve3::Line(line) = &segment.curve {
                    let origin = plane.project(line.origin())?.parameters;
                    let end = plane.project(line.origin() + line.direction())?.parameters;
                    segment.pcurve = super::Line2::new(
                        Point2::new(origin.0, origin.1),
                        Vec2::new(end.0 - origin.0, end.1 - origin.1),
                    )?
                    .into();
                }
            }
        }
    }
    super::euler::conform_brep_interfaces(&mut patches, tolerance)?;
    // Section regions partition each shared support exactly once. Boundary-only
    // containment would mistake a cap for a duplicate of its surrounding face.
    if !preserve_first_faces {
        deduplicate_patches(&mut patches, tolerance)?;
    }
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
    transaction.unify_same_domain()?;
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
        Surface3::Swept(_)
        | Surface3::Offset(_)
        | Surface3::RollingBall(_)
        | Surface3::Ruled(_) => 4,
    };
    (boundary_complexity, surface_complexity)
}

/// A bounded intersection may straddle a periodic face's seam. Translate its
/// UV image, not its spatial parameter, so both clipped halves enter the same
/// arrangement even when the spatial curve has no periodic evaluation API.
fn intersection_chart_copies(
    curve: &Curve3,
    pcurve: &Curve2,
    surface: &Surface3,
    other: &Surface3,
    boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<Curve2>> {
    // Closed sections can cross the chart seam independently of the face's
    // edge range. This includes circles: their cylinder p-curve is unbounded
    // even though the spatial curve and its mate have one bounded revolution.
    let conical_section = matches!(
        (surface, other),
        (
            Surface3::Cone(_),
            Surface3::Plane(_) | Surface3::Cylinder(_)
        ) | (
            Surface3::Plane(_) | Surface3::Cylinder(_),
            Surface3::Cone(_)
        )
    );
    let oblique_section = matches!(curve, Curve3::SurfaceIntersection(_))
        && matches!(
            (surface, other),
            (Surface3::Plane(_), Surface3::Cylinder(_))
                | (Surface3::Cylinder(_), Surface3::Plane(_))
        );
    let circular_section = matches!(curve, Curve3::Circle(_));
    if !(conical_section || oblique_section || circular_section)
        || !surface.domains().iter().any(|d| d.period.is_some())
    {
        return Ok(vec![pcurve.clone()]);
    }
    let super::DomainBounds::Bounded(range) = curve.domain().bounds else {
        return Ok(vec![pcurve.clone()]);
    };
    if curve
        .evaluate(range.start)?
        .point
        .distance(curve.evaluate(range.end)?.point)
        > tolerance.linear
    {
        return Ok(vec![pcurve.clone()]);
    }
    let bounds = face_parameter_bounds(boundary, tolerance)?;
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for i in 0..=32 {
        let p = pcurve
            .evaluate(range.start + range.span() * i as f64 / 32.0)?
            .point;
        minimum.x = minimum.x.min(p.x);
        minimum.y = minimum.y.min(p.y);
        maximum.x = maximum.x.max(p.x);
        maximum.y = maximum.y.max(p.y);
    }
    let offsets = |axis: usize| {
        let candidates = surface.domains()[axis]
            .period
            .map_or(vec![0.0], |p| vec![-p, 0.0, p]);
        candidates
            .into_iter()
            .filter(|offset| {
                minimum[axis] + offset < bounds[axis].end + tolerance.parameter
                    && maximum[axis] + offset > bounds[axis].start - tolerance.parameter
            })
            .collect::<Vec<_>>()
    };
    let mut result = Vec::new();
    for u in offsets(0) {
        for v in offsets(1) {
            result.push(if u == 0.0 && v == 0.0 {
                pcurve.clone()
            } else {
                translate_pcurve(pcurve, Vec2::new(u, v), tolerance)?
            });
        }
    }
    Ok(result)
}

fn align_pcurve_to_face_chart(
    pcurve: &Curve2,
    trim: Option<ParameterRange>,
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
    let parameter = match trim
        .map(super::DomainBounds::Bounded)
        .unwrap_or(pcurve.domain().bounds)
    {
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
    translate_pcurve(pcurve, offset, tolerance)
}

pub(super) fn translate_pcurve(
    pcurve: &Curve2,
    offset: Vec2,
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
            let guide = translate_pcurve(projection.guide(), offset, tolerance)?;
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
            // A projection can retain an intersection on a different support as
            // its guide. Translate that guide on its own selected side, not on
            // the outer projection's destination surface.
            let guide = translate_pcurve(intersection.guide(), offset, tolerance)?;
            let range = curve.range();
            Ok(super::SurfaceProjectionPcurve2::new(
                spatial,
                intersection.surface().clone(),
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
    first_surface: &Surface3,
    first: &[FaceCurve],
    second_surface: &Surface3,
    second: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let bounds = |surface: &Surface3, curves: &[FaceCurve]| -> KernelResult<Option<[Point3; 2]>> {
        // Boundary samples do not enclose a curved face's interior. Use a
        // support enclosure, or decline the shortcut when none is certified.
        if let Surface3::Torus(torus) = surface {
            let radius = torus.major_radius() + torus.minor_radius();
            let extent = crate::Vec3::new(radius, radius, radius);
            return Ok(Some([torus.center() - extent, torus.center() + extent]));
        }
        if let Surface3::Sphere(sphere) = surface {
            let r = sphere.radius();
            let extent = crate::Vec3::new(r, r, r);
            return Ok(Some([sphere.center() - extent, sphere.center() + extent]));
        }
        if !matches!(surface, Surface3::Plane(_) | Surface3::Cylinder(_)) {
            return Ok(None);
        }
        let mut minimum = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut maximum = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut include = |point: Point3| {
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(point[axis]);
                maximum[axis] = maximum[axis].max(point[axis]);
            }
        };
        for curve in curves {
            match &curve.curve {
                Curve3::Line(_) => {
                    include(curve.curve.evaluate(curve.range.start)?.point);
                    include(curve.curve.evaluate(curve.range.end)?.point);
                }
                Curve3::Circle(circle) => {
                    let x = circle.x_radius();
                    let y = circle.y_radius();
                    let extent = crate::Vec3::new(x.x.hypot(y.x), x.y.hypot(y.y), x.z.hypot(y.z));
                    include(circle.center() - extent);
                    include(circle.center() + extent);
                }
                Curve3::Nurbs(curve) => {
                    for point in curve.control_points() {
                        include(*point);
                    }
                }
                _ => return Ok(None),
            }
        }
        if let Surface3::Cylinder(cylinder) = surface {
            let axis = cylinder.axis().normalize();
            let mut low = f64::INFINITY;
            let mut high = f64::NEG_INFINITY;
            for mask in 0..8 {
                let point = Point3::new(
                    if mask & 1 == 0 { minimum.x } else { maximum.x },
                    if mask & 2 == 0 { minimum.y } else { maximum.y },
                    if mask & 4 == 0 { minimum.z } else { maximum.z },
                );
                let height = (point - cylinder.origin()).dot(axis);
                low = low.min(height);
                high = high.max(height);
            }
            let start = cylinder.origin() + axis * low;
            let end = cylinder.origin() + axis * high;
            let x = cylinder.x_radius();
            let y = cylinder.y_radius();
            for coordinate in 0..3 {
                let radius = x[coordinate].hypot(y[coordinate]);
                minimum[coordinate] = start[coordinate].min(end[coordinate]) - radius;
                maximum[coordinate] = start[coordinate].max(end[coordinate]) + radius;
            }
        }
        Ok(Some([minimum, maximum]))
    };
    let (Some(first), Some(second)) = (
        bounds(first_surface, first)?,
        bounds(second_surface, second)?,
    ) else {
        return Ok(false);
    };
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
                    pcurve = translate_pcurve(&pcurve, offset, compound.tolerance())?;
                }
            }
            previous = Some(pcurve.evaluate(coedge.range().end)?.point);
            curves.push(FaceCurve {
                provenance: SegmentProvenance::from_coedge(topology, coedge_id)?,
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
            let mut first = pair[0].1;
            let mut second = pair[1].1;
            // Adjacent exact p-curves can round their common endpoint to
            // opposite sides of the ray. Give that endpoint one half-open
            // crossing decision, particularly at a tangent line/arc junction.
            for endpoint in [&mut first, &mut second] {
                if (endpoint.y - point.y).abs() <= 1.0e-12 {
                    endpoint.y = point.y;
                }
            }
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
                    if first.y == point.y {
                        crossing = first;
                        break;
                    }
                    if second.y == point.y {
                        crossing = second;
                        break;
                    }
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
            let Ok(projection) = candidate
                .curve
                .project_on_trim(point, candidate.range, tolerance)
            else {
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
            let start = parameters[0];
            let end = parameters[parameters.len() - 1];
            if (end - start).abs() <= tolerance.parameter {
                continue;
            }
            return Ok(Some(FaceCurve {
                provenance: candidate
                    .provenance
                    .trimmed(candidate.range, ParameterRange::new(start, end)?),
                curve: candidate.curve.clone(),
                pcurve: candidate.pcurve.clone(),
                range: ParameterRange::new(start, end)?,
                loop_end: false,
                loop_kind: candidate.loop_kind,
                parameter_seam: candidate.parameter_seam,
            }));
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
        if let Curve2::SurfaceProjection(projection) = curve {
            return Ok(projection.range());
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
        provenance: Default::default(),
        curve: curve.clone(),
        pcurve: first.clone(),
        range: domain,
        loop_end: false,
        loop_kind: LoopKind::Outer,
        parameter_seam: false,
    };
    let second_curve = FaceCurve {
        provenance: Default::default(),
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
            // Spatial periodicity does not make a bounded UV chart periodic.
            // Other surface sheets are supplied by intersection_chart_copies;
            // only shift parameters when both p-curves support that branch.
            if !first.domain().contains(middle + shift, tolerance.parameter)
                || !second.domain().contains(middle + shift, tolerance.parameter)
            {
                continue;
            }
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
            let projection = curve.project_on_trim(point, range, tolerance).ok()?;
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
    if let (Curve3::SurfaceIntersection(a), Curve3::SurfaceIntersection(b)) =
        (&first.curve, &second.curve)
    {
        if (coincident_supports(a.first_surface(), b.first_surface(), tolerance)?
            && coincident_supports(a.second_surface(), b.second_surface(), tolerance)?)
            || (coincident_supports(a.first_surface(), b.second_surface(), tolerance)?
                && coincident_supports(a.second_surface(), b.first_surface(), tolerance)?)
        {
            // Two guides of the same regular support intersection can overlap.
            // Intersecting their approximation splines invents crossings along
            // that overlap; only trim endpoints subdivide the common curve.
            return Ok(deduplicate(endpoint_hits()?));
        }
    }
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
    exclude_existing_faces: bool,
    classifiers: [OperandClassifier<'_>; 2],
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
        if exclude_existing_faces {
            for face in classifiers[0].faces {
                if !coincident_supports(surface, &face.surface, tolerance)? {
                    continue;
                }
                let projection = face.surface.project(value.point, tolerance)?;
                let uv = Point2::new(projection.parameters.0, projection.parameters.1);
                if point_in_trim(uv, &face.boundary)? {
                    return Ok(None);
                }
            }
        }
        let parameter_normal = value.normal()?;
        if has_coincident_support {
            // On a shared support, exact oriented trims determine the local
            // material sides. A fixed normal probe can cross a nearby blend
            // or thin wall and invent an interface between tangent cutters.
            let first = classifiers[0].sides_on_support(surface, value.point, parameter_normal)?;
            let second = classifiers[1].sides_on_support(surface, value.point, parameter_normal)?;
            let below_occupied = occupied(kind, first.0, second.0);
            let above_occupied = occupied(kind, first.1, second.1);
            let coincident_orientation = match (below_occupied, above_occupied) {
                (true, false) => Some(FaceOrientation::Forward),
                (false, true) => Some(FaceOrientation::Reversed),
                _ => None,
            };
            return Ok(coincident_orientation);
        }
        let other_contains = classifiers[1 - operand].contains(value.point)?;
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
        if surface
            .domains()
            .iter()
            .enumerate()
            .any(|(axis, domain)| domain.period.is_none() && !domain.contains(uv[axis], 0.0))
        {
            return Ok(None);
        }
        if !has_coincident_support && !point_in_trim(uv, original_boundary)? {
            return Ok(None);
        }
        classify_orientation_at(uv)
    };

    if !exclude_existing_faces && curves.len() == original_boundary.len() {
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
                // Bounded intersection guides can describe complete closed
                // curves without a periodic evaluation domain. Coincident
                // endpoints alone do not make such a boundary degenerate.
                let mut collapsed = true;
                for fraction in [0.25, 0.5, 0.75] {
                    let point = curve
                        .curve
                        .evaluate(range.start + range.span() * fraction)?
                        .point;
                    if start.distance(point) > tolerance.linear * 8.0 {
                        collapsed = false;
                        break;
                    }
                }
                if collapsed {
                    continue;
                }
            }
            let start_node = weld_node(&mut nodes, start_uv)?;
            let end_node = weld_node(&mut nodes, end_uv)?;
            pieces.push(DirectedPiece {
                provenance: curve.provenance.trimmed(curve.range, range),
                curve: curve.curve.clone(),
                pcurve: curve.pcurve.clone(),
                range,
                start_node,
                end_node,
                start_uv,
                end_uv,
                parameter_seam: curve.parameter_seam,
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
            let Ok(projection) =
                curve
                    .curve
                    .project_on_trim(surface_value.point, curve.range, tolerance)
            else {
                continue;
            };
            if projection.distance > tolerance.linear * 16.0 {
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
            let duplicate = unique.iter().position(|candidate| {
                let same_nodes = (candidate.start_node == piece.start_node
                    && candidate.end_node == piece.end_node)
                    || (candidate.start_node == piece.end_node
                        && candidate.end_node == piece.start_node);
                if !same_nodes {
                    return false;
                }
                let projected_match = candidate
                    .curve
                    .project_on_trim(middle, candidate.range, tolerance)
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
            if let Some(index) = duplicate {
                let candidate = &mut unique[index];
                let a = candidate
                    .curve
                    .evaluate(candidate.range.start + candidate.range.span() * 0.5)?
                    .first_derivative
                    * candidate.range.span();
                let b = piece
                    .curve
                    .evaluate(piece.range.start + piece.range.span() * 0.5)?
                    .first_derivative
                    * piece.range.span();
                candidate.provenance.weld(&if a.dot(b) >= 0.0 {
                    piece.provenance.clone()
                } else {
                    piece.provenance.reversed()
                });
            } else {
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
        let mut loops = loops
            .into_iter()
            .map(|loop_| {
                let area = loop_.iter().try_fold(0.0, |sum, piece| {
                    Ok::<_, KernelError>(
                        sum + piece
                            .pcurve
                            .signed_area_contribution(piece.range, tolerance.parameter)?,
                    )
                })?;
                Ok::<_, KernelError>((loop_, area))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        // Enclosing loops must be classified before their holes, regardless
        // of which source face supplied the first arrangement edge.
        loops.sort_by(|first, second| second.1.abs().total_cmp(&first.1.abs()));
        let mut classified = Vec::<(Vec<DirectedPiece>, Vec<Point2>, f64, bool)>::new();
        for (mut loop_, area) in loops {
            if area.abs() <= tolerance.parameter.powi(2) {
                continue;
            }
            let polygon = loop_polyline(&loop_)?;
            // A concave loop's centroid may lie outside it, even inside a
            // disjoint larger loop. Test nesting from the boundary itself:
            // arrangement loops cannot cross, so an interior point of any
            // boundary segment lies inside every enclosing loop.
            let segment = &loop_[0];
            let sample = segment
                .pcurve
                .evaluate(segment.range.start + segment.range.span() * 0.5)?
                .point;
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
        for outer_index in classified
            .iter()
            .enumerate()
            .filter(|(_, value)| value.3)
            .map(|(index, _)| index)
        {
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
                preserved: Vec::new(),
                sources: Vec::new(),
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
    // Imprinting a touching face can subdivide a chart seam while leaving
    // the entire cylindrical wall unchanged. Reconnect the complete seam
    // chain: its intermediate nodes have degree zero, so looking only for a
    // single edge between odd-degree endpoints would discard the wall.
    let endpoints = degree
        .iter()
        .filter_map(|(node, degree)| (degree % 2 == 1).then_some(*node))
        .collect::<Vec<_>>();
    for start in endpoints {
        if degree[&start] % 2 == 0 {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        let mut previous = BTreeMap::new();
        let mut visited = BTreeSet::from([start]);
        let mut end = None;
        while let Some(node) = queue.pop_front() {
            if node != start && degree.get(&node).copied().unwrap_or(0) % 2 == 1 {
                end = Some(node);
                break;
            }
            for (index, seam) in arrangement
                .iter()
                .enumerate()
                .filter(|(_, p)| p.parameter_seam)
            {
                if selected.iter().any(|piece| {
                    (piece.start_node == seam.start_node && piece.end_node == seam.end_node)
                        || (piece.start_node == seam.end_node && piece.end_node == seam.start_node)
                }) {
                    continue;
                }
                let next = if seam.start_node == node {
                    seam.end_node
                } else if seam.end_node == node {
                    seam.start_node
                } else {
                    continue;
                };
                let next_degree = degree.get(&next).copied().unwrap_or(0);
                if next_degree > 0 && next_degree % 2 == 0 {
                    continue;
                }
                if visited.insert(next) {
                    previous.insert(next, (node, index));
                    queue.push_back(next);
                }
            }
        }
        if let Some(mut node) = end {
            while node != start {
                let (parent, index) = previous[&node];
                let seam = &arrangement[index];
                selected.push(seam.clone());
                *degree.entry(seam.start_node).or_default() += 1;
                *degree.entry(seam.end_node).or_default() += 1;
                node = parent;
            }
        }
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
            provenance: segment.provenance.clone(),
            curve: segment.curve.clone(),
            // An unchanged face already has exact, chart-aligned coedges.
            // The edge's own p-curve may describe the other side of its seam.
            pcurve: segment.pcurve.clone(),
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
                segment.provenance = segment.provenance.reversed();
            }
        }
        patch_loops.push(BrepPatchLoop { kind, segments });
    }
    Ok(BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
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
            provenance: piece.provenance.clone(),
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

pub(crate) fn patch_edge_samples(patch: &BrepPatch) -> KernelResult<Vec<[Point3; 3]>> {
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

pub(crate) fn same_unoriented_edge(first: [Point3; 3], second: [Point3; 3], linear: f64) -> bool {
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
                let sources = patches[second].sources.clone();
                patches[first].sources.extend(sources);
                duplicate.insert(second);
            } else if patch_contains_patch(&patches[second], &samples[first], tolerance)? {
                let sources = patches[first].sources.clone();
                patches[second].sources.extend(sources);
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

pub(crate) fn coincident_supports(
    first: &Surface3,
    second: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    if first == second {
        return Ok(true);
    }
    match (first, second) {
        (Surface3::Torus(_), Surface3::Torus(_)) => Ok(matches!(
            first.intersect_surface(second, tolerance),
            Ok(IntersectionSet::Coincident)
        )),
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

/// Attribute a coincident arrangement by overlap of trimmed interiors, including
/// containment. Split probes at boundary intersections so a short overlap cannot
/// be missed by testing only the midpoint of an entire source edge.
fn patch_uses_face(
    patch: &BrepPatch,
    parent: &Compound,
    face: &super::FaceHandle,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let boundary = patch
        .loops
        .iter()
        .flat_map(|loop_| {
            loop_
                .segments
                .iter()
                .enumerate()
                .map(move |(index, segment)| FaceCurve {
                    provenance: Default::default(),
                    curve: segment.curve.clone(),
                    pcurve: segment.pcurve.clone(),
                    range: segment.range,
                    loop_end: index + 1 == loop_.segments.len(),
                    loop_kind: loop_.kind,
                    parameter_seam: false,
                })
        })
        .collect::<Vec<_>>();
    let mut original = boundary_curves(parent, face.topology_id())?;
    for curve in &mut original {
        if face.surface() != &patch.surface {
            curve.pcurve = super::SurfaceProjectionPcurve2::new(
                curve.curve.clone(),
                patch.surface.clone(),
                curve.pcurve.clone(),
                curve.range,
                tolerance,
            )?
            .into();
        }
    }
    let bounds = face_parameter_bounds(&boundary, tolerance)?;
    let original_bounds = face_parameter_bounds(&original, tolerance)?;
    let domains = patch.surface.domains();
    let mut offset = Vec2::new(0.0, 0.0);
    for axis in 0..2 {
        if let Some(period) = domains[axis].period {
            let center = (bounds[axis].start + bounds[axis].end) * 0.5;
            let original_center = (original_bounds[axis].start + original_bounds[axis].end) * 0.5;
            offset[axis] = ((center - original_center) / period).round() * period;
        }
    }
    // Move the complete chart together. Shifting individual seam uses can
    // collapse a full-period region into a zero-width boundary.
    if offset.magnitude2() != 0.0 {
        for curve in &mut original {
            curve.pcurve = translate_pcurve(&curve.pcurve, offset, tolerance)?;
        }
    }
    let original_bounds = face_parameter_bounds(&original, tolerance)?;
    let inside = |mut point: Point2, curves: &[FaceCurve], bounds: [ParameterRange; 2]| {
        for axis in 0..2 {
            if let Some(period) = domains[axis].period {
                let center = (bounds[axis].start + bounds[axis].end) * 0.5;
                point[axis] += ((center - point[axis]) / period).round() * period;
            }
        }
        point_in_trim(point, curves)
    };
    for (a, ab, b, bb) in [
        (&boundary, bounds, &original, original_bounds),
        (&original, original_bounds, &boundary, bounds),
    ] {
        for curve in a.iter().filter(|curve| !curve.parameter_seam) {
            let mut parameters = vec![curve.range.start, curve.range.end];
            for other in b {
                parameters.extend(
                    curve_intersections(curve, other, tolerance)?
                        .into_iter()
                        .map(|(parameter, _)| parameter),
                );
            }
            parameters.sort_by(f64::total_cmp);
            parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
            for interval in parameters.windows(2) {
                let (point, tangent) = curve
                    .pcurve
                    .first_order((interval[0] + interval[1]) * 0.5)?;
                if tangent.magnitude2() == 0.0 {
                    continue;
                }
                let normal = Vec2::new(-tangent.y, tangent.x).normalize();
                let value = patch.surface.evaluate(point.x, point.y)?;
                let scale = (value.du * normal.x + value.dv * normal.y).magnitude();
                if scale == 0.0 {
                    continue;
                }
                let step =
                    tolerance.linear_at_scale(value.point.to_vec().magnitude()) * 8.0 / scale;
                for sign in [-1.0, 1.0] {
                    let probe = point + normal * step * sign;
                    if inside(probe, a, ab)? && inside(probe, b, bb)? {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
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
            provenance: Default::default(),
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
    // Equivalent exact intersection curves can have different parameterizations.
    // Their parameter midpoints need not coincide in space; test containment on
    // both trimmed curves before treating the faces as separate components.
    let segments = patches
        .iter()
        .map(|patch| {
            patch
                .loops
                .iter()
                .flat_map(|l| &l.segments)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
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
                    samples[current].iter().enumerate().any(|(i, first)| {
                        samples[*candidate].iter().enumerate().any(|(j, second)| {
                            same_unoriented_edge(*first, *second, linear)
                                || (((first[0].distance(second[0]) <= linear
                                    && first[2].distance(second[2]) <= linear)
                                    || (first[0].distance(second[2]) <= linear
                                        && first[2].distance(second[0]) <= linear))
                                    && segments[current][i]
                                        .curve
                                        .project_on_trim(
                                            second[1],
                                            segments[current][i].range,
                                            tolerance,
                                        )
                                        .is_ok_and(|p| p.distance <= linear)
                                    && segments[*candidate][j]
                                        .curve
                                        .project_on_trim(
                                            first[1],
                                            segments[*candidate][j].range,
                                            tolerance,
                                        )
                                        .is_ok_and(|p| p.distance <= linear))
                        })
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

#[cfg(test)]
mod tests {
    use super::super::{Plane3, Profile, TrimmedCurve2, Wire2};
    use super::*;
    use crate::Vec3;

    #[test]
    fn periodic_projection_translates_an_intersection_guide_on_its_own_support() {
        use super::super::{
            Circle2, Cylinder3, Line2, SurfaceIntersectionCurve, SurfaceProjectionPcurve2,
        };

        let tolerance = ModelTolerance::default();
        let cylinder = |height| -> Surface3 {
            Cylinder3::new(
                Point3::new(0.0, 0.0, height),
                Vec3::unit_z(),
                Vec3::unit_x(),
                Vec3::unit_y(),
                tolerance,
            )
            .unwrap()
            .into()
        };
        let range = ParameterRange::new(0.0, std::f64::consts::TAU).unwrap();
        let intersection = SurfaceIntersectionCurve::new(
            Plane3::xy().into(),
            cylinder(0.0),
            Circle2::new(Point2::new(0.0, 0.0), 1.0).unwrap().into(),
            Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))
                .unwrap()
                .into(),
            range,
            tolerance,
            0.0,
        )
        .unwrap();
        // Same cylindrical support, but a different UV chart. The retained
        // guide selects the intersection's second side on the original chart.
        let surface = cylinder(1.0);
        let projected: Curve2 = SurfaceProjectionPcurve2::new(
            intersection.clone().into(),
            surface.clone(),
            intersection.exact_pcurves()[1].clone(),
            range,
            tolerance,
        )
        .unwrap()
        .into();
        for shift in [-std::f64::consts::TAU, std::f64::consts::TAU] {
            let translated = translate_pcurve(&projected, Vec2::new(shift, 0.0), tolerance)
                .expect("nested intersection guide keeps its own support");
            for parameter in [0.0, 0.7, 3.0, range.end] {
                let uv = translated.evaluate(parameter).unwrap().point;
                let original = projected.evaluate(parameter).unwrap().point;
                assert!((uv - original - Vec2::new(shift, 0.0)).magnitude() < tolerance.linear);
                let spatial = Curve3::from(intersection.clone())
                    .evaluate(parameter)
                    .unwrap()
                    .point;
                assert!(
                    surface
                        .evaluate(uv.x, uv.y)
                        .unwrap()
                        .point
                        .distance(spatial)
                        < tolerance.linear
                );
            }
        }
    }

    #[test]
    fn classification_mesh_is_lazy_reused_and_fallible() {
        let tolerance = ModelTolerance::default();
        let compound = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 10.0).unwrap()],
                tolerance,
            )
            .unwrap(),
            [],
            tolerance,
        )
        .unwrap()
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .unwrap();
        let faces = compound
            .faces()
            .map(|face| {
                let boundary = boundary_curves(&compound, face.topology_id()).unwrap();
                ClassificationFace {
                    surface: face.surface().clone(),
                    orientation: compound
                        .topology()
                        .face(face.topology_id())
                        .unwrap()
                        .orientation(),
                    bounds: face_parameter_bounds(&boundary, tolerance).unwrap(),
                    boundary,
                }
            })
            .collect::<Vec<_>>();
        let mesh = OnceCell::new();
        let classifier = OperandClassifier {
            compound: &compound,
            mesh: &mesh,
            options: TessellationOptions::new(0.1, 0.12, 12).unwrap(),
            faces: &faces,
            tolerance,
            chord: 0.1,
        };
        assert!(classifier.contains(Point3::new(9.95, 0.0, 10.0)).unwrap());
        assert!(!classifier.contains(Point3::new(10.05, 0.0, 10.0)).unwrap());
        assert!(
            mesh.get().is_none(),
            "exact face queries must not mesh the operand"
        );
        assert!(classifier.contains(Point3::new(0.0, 0.0, 10.0)).unwrap());
        let cached = mesh.get().unwrap() as *const _;
        assert!(!classifier.contains(Point3::new(20.0, 0.0, 10.0)).unwrap());
        assert_eq!(
            cached,
            mesh.get().unwrap() as *const _,
            "reuse the fallback mesh"
        );

        let failing_mesh = OnceCell::new();
        let failing = OperandClassifier {
            mesh: &failing_mesh,
            options: TessellationOptions {
                maximum_depth: 0,
                ..classifier.options
            },
            ..classifier
        };
        assert!(failing.contains(Point3::new(9.95, 0.0, 10.0)).unwrap());
        assert!(failing_mesh.get().is_none());
        let error = failing.contains(Point3::new(0.0, 0.0, 10.0)).unwrap_err();
        assert_eq!(
            failing.contains(Point3::new(0.0, 0.0, 10.0)).unwrap_err(),
            error
        );
        assert!(
            failing_mesh.get().unwrap().is_err(),
            "retain fallback errors too"
        );
    }
}
