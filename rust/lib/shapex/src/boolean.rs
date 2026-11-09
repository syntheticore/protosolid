use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2};

use super::brep_classification::{BrepClassifier, PointClassification};
use super::bounds::{bounds_separated, Bounds};
use super::trim::{align_pcurve_to_face_chart, coincident_supports,
    face_boundary_curves, face_parameter_bounds, intersection_chart_copies,
    point_in_sampled_trim, point_in_sampled_periodic_trim, sample_pcurve,
    translate_pcurve, trim_samples, trim_to_faces_sampled, curve_intersections_sampled, TrimSamples};
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::polygon::point_in_polygon;
use super::{
    box_center, nearest_uv, whole_periods, Compound, Curve2, Curve3, DiagnosticKind, FaceHandle,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, LoopKind, ModelTolerance,
    ParameterRange, Surface3,
};
use crate::{geometry_error, topology_error};

/// Material-set operation requested by the high-level exact BREP Boolean API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Union,
    Intersection,
    Difference,
}

type FaceCurve = super::trim::FaceCurve<SegmentProvenance>;

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
    let mut arrangement = Arrangement::new([first, second], kind)?;
    arrangement.imprint_intersections()?;
    arrangement.overlay_coincident_boundaries()?;
    let mut patches = arrangement.face_patches()?;
    super::sew::conform_brep_interfaces(&mut patches, tolerance)?;
    remove_retraced_trim_spurs(&mut patches, tolerance)?;
    deduplicate_patches(&mut patches, tolerance)?;
    // Let the Euler weld choose the simplest exact representation when more
    // than two supports meet on one edge (for example a planar line tangent to
    // a NURBS extrusion).  This is geometry canonicalization, independent of
    // operand or feature order.
    patches.sort_by_key(patch_complexity_key);
    match publish_boolean(first, second, kind, &patches) {
        Err(error) if error.diagnostics().iter().any(|diagnostic| diagnostic.kind == DiagnosticKind::GeometryInvariant) => {
            if let Some(mut partitioned) = partition_tangent_trims(&patches, tolerance)? {
                super::sew::conform_brep_interfaces(&mut partitioned, tolerance)?;
                publish_boolean(first, second, kind, &partitioned)
            } else {
                Err(error)
            }
        }
        result => result,
    }
}

/// Independently imprinted trims can retrace a support's own boundary at a
/// tangential junction. Once the endpoints conform, cancel the backtrack in
/// that chart. A backtrack between two outer regions separates their loops.
/// A periodic seam has distinct chart copies and must be kept.
fn remove_retraced_trim_spurs(patches: &mut Vec<BrepPatch>, tolerance: ModelTolerance) -> KernelResult<()> {
    let mut split_patches = Vec::new();
    for patch in patches.iter_mut() {
        let simple = patch.loops.len() == 1;
        let mut split_loops = Vec::new();
        for loop_ in &mut patch.loops {
            let mut index = 0;
            while loop_.segments.len() > 1 && index < loop_.segments.len() {
                let first = &loop_.segments[index];
                let uv = |segment: &BrepPatchSegment, t: f64| {
                    segment.pcurve.first_order(t).map(|value| value.0)
                };
                let same_uv = |a: Point2, b: Point2| -> KernelResult<bool> {
                    let domains = patch.surface.domains();
                    if (0..2).any(|axis| domains[axis].period.is_some_and(|period|
                        (a[axis] - b[axis]).abs() >= period * 0.5)) {
                        return Ok(false);
                    }
                    let first = patch.surface.first_order(a.x, a.y)?.0;
                    let second = patch.surface.first_order(b.x, b.y)?.0;
                    Ok(first.distance(second) <= tolerance.linear_at_scale(first.to_vec().magnitude()) * 4.0)
                };
                let mut retraced = None;
                for (other, second) in loop_.segments.iter().enumerate().skip(index + 1) {
                    if !simple && other != index + 1 && (index != 0 || other + 1 != loop_.segments.len()) {
                        continue;
                    }
                    if !same_uv(uv(first, first.range.start)?, uv(second, second.range.end)?)?
                        || !same_uv(uv(first, first.range.end)?, uv(second, second.range.start)?)?
                    {
                        continue;
                    }
                    let mut retraces = true;
                    for fraction in [0.25, 0.5, 0.75] {
                        let t = first.range.start + first.range.span() * fraction;
                        let point = first.curve.first_order(t)?.0;
                        let projection = second.curve.project_on_trim(point, second.range, tolerance)?;
                        let parameter = second.curve.domain().nearest(projection.parameter, second.range.middle());
                        if projection.distance > tolerance.linear_at_scale(point.to_vec().magnitude()) * 4.0
                            || !same_uv(uv(first, t)?, uv(second, parameter)?)?
                        {
                            retraces = false;
                            break;
                        }
                    }
                    if retraces {
                        let separate_loops = other != index + 1
                            && (index != 0 || other + 1 != loop_.segments.len());
                        if separate_loops {
                            let inside = trim_loop_area(
                                loop_.segments[index + 1..other].iter(), tolerance,
                            )?;
                            let outside = trim_loop_area(
                                loop_.segments[..index].iter().chain(&loop_.segments[other + 1..]),
                                tolerance,
                            )?;
                            let sign = patch.orientation.sign();
                            if inside * sign <= 0.0 || outside * sign <= 0.0 {
                                continue;
                            }
                        }
                        retraced = Some(other);
                        break;
                    }
                }
                if let Some(other) = retraced {
                    let segments = loop_.segments.drain(index..=other).skip(1)
                        .take(other - index - 1).collect::<Vec<_>>();
                    if !segments.is_empty() && !loop_.segments.is_empty() {
                        split_loops.push(BrepPatchLoop { kind: loop_.kind, segments });
                    } else if loop_.segments.is_empty() {
                        loop_.segments = segments;
                    }
                    index = index.saturating_sub(1);
                } else {
                    index += 1;
                }
            }
        }
        patch.loops.retain(|loop_| !loop_.segments.is_empty());
        for loop_ in split_loops {
            split_patches.push(BrepPatch {
                preserved: Vec::new(), sources: patch.sources.clone(), surface: patch.surface.clone(),
                orientation: patch.orientation, loops: vec![loop_],
            });
        }
    }
    patches.retain(|patch| patch.loops.iter().any(|loop_| loop_.kind == LoopKind::Outer));
    patches.extend(split_patches);
    Ok(())
}

fn trim_loop_area<'a>(
    mut segments: impl Iterator<Item = &'a BrepPatchSegment>,
    tolerance: ModelTolerance,
) -> KernelResult<f64> {
    segments.try_fold(0.0, |area, edge| {
        edge.pcurve.signed_area_contribution(edge.range, tolerance.parameter).map(|value| area + value)
    })
}

fn publish_boolean(first: &Compound, second: &Compound, kind: BooleanKind, patches: &[BrepPatch]) -> KernelResult<Compound> {
    let tolerance = first.tolerance();
    let mut components = patch_components(patches.to_vec(), tolerance)?;
    for component in &mut components {
        component.sort_by_key(patch_complexity_key);
    }

    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::with_capacity(components.len());
    for component in &components {
        bodies.push(transaction.brep_shell(component)?);
    }
    transaction.nest_brep_shells(&bodies)?;
    transaction.unify_same_domain()?;
    let committed = transaction.finish()?;
    Compound::from_operation_with_parents(kernel, committed, &[first, second], super::DerivationRole::Boolean(kind))
}

/// A face of either operand, addressed as `(operand, index)`.
type FaceKey = (usize, usize);

/// How a face being arranged shares its support with the other operand.
#[derive(Debug, Clone, Copy)]
enum Support {
    /// No face of the other operand lies on this support.
    Own,
    /// Faces of both operands lie on this support. With an `owner`, each of
    /// that operand's trims is one chart cell.
    Shared {
        owner: Option<usize>,
    },
}

/// The faces of both operands with their trims, as intersections are imprinted
/// and coincident boundaries overlaid before regions are classified.
struct Arrangement<'a> {
    operands: [&'a Compound; 2],
    faces: [Vec<FaceHandle>; 2],
    kind: BooleanKind,
    tolerance: ModelTolerance,
    /// Probe distance used to classify the material side of a region.
    classification_offset: f64,
    /// Each face's own trim, recharted against the other operand.
    original: BTreeMap<FaceKey, Vec<FaceCurve>>,
    /// Parameter bounds of each original trim. Implicit p-curves make these
    /// costly, and every face pair and coincident overlay needs them.
    original_bounds: BTreeMap<FaceKey, [ParameterRange; 2]>,
    /// Certified spatial enclosures survive recharting and are reused by every
    /// face-pair query, including faces with implicit intersection edges.
    spatial_bounds: BTreeMap<FaceKey, Option<Bounds>>,
    /// The trim plus every curve imprinted on the face.
    imprinted: BTreeMap<FaceKey, Vec<FaceCurve>>,
    supports: RefCell<BTreeMap<(FaceKey, FaceKey), bool>>,
}

impl<'a> Arrangement<'a> {
    fn new(
        operands: [&'a Compound; 2],
        kind: BooleanKind,
    ) -> KernelResult<Self> {
        let tolerance = operands[0].tolerance();
        let scale = operands
            .iter()
            .flat_map(|operand| operand.vertices())
            .map(|vertex| vertex.point().to_vec().magnitude())
            .fold(1.0_f64, f64::max);
        let faces = operands.map(|operand| operand.faces().cloned().collect::<Vec<_>>());
        let mut curves = BTreeMap::new();
        let mut spatial_bounds = BTreeMap::new();
        for operand in 0..2 {
            for (index, face) in faces[operand].iter().enumerate() {
                spatial_bounds.insert((operand, index),
                    super::bounds::face(operands[operand].topology(), face.topology_id()).ok());
                curves.insert(
                    (operand, index),
                    boundary_curves(operands[operand], face.topology_id())?,
                );
            }
        }
        let source_bounds = curves.iter()
            .map(|(key, boundary)| Ok((*key, face_parameter_bounds(boundary, tolerance)?)))
            .collect::<KernelResult<BTreeMap<_, _>>>()?;
        let mut original_bounds = source_bounds.clone();
        for operand in 0..2 {
            for (index, face) in faces[operand].iter().enumerate() {
                let key = (operand, index);
                let boundary = curves.get_mut(&key).expect("every face has a trim");
                if rechart_periodic_band(
                    face.surface(),
                    boundary,
                    source_bounds[&key],
                    &faces[1 - operand],
                    &source_bounds,
                    1 - operand,
                    spatial_bounds[&key],
                    &spatial_bounds,
                    tolerance,
                )? {
                    original_bounds.insert(key, face_parameter_bounds(boundary, tolerance)?);
                }
            }
        }
        Ok(Self {
            operands,
            faces,
            kind,
            tolerance,
            classification_offset: (tolerance.linear_at_scale(scale) * 32.0).max(scale * 5.0e-4)
                * 8.0,
            original: curves.clone(),
            original_bounds,
            spatial_bounds,
            imprinted: curves,
            supports: RefCell::default(),
        })
    }

    fn face(&self, key: FaceKey) -> &FaceHandle {
        &self.faces[key.0][key.1]
    }

    fn keys(&self, operand: usize) -> impl Iterator<Item = FaceKey> {
        (0..self.faces[operand].len()).map(move |index| (operand, index))
    }

    /// Whether two faces lie on one support, computed once per unordered pair.
    fn same_support(&self, first: FaceKey, second: FaceKey) -> KernelResult<bool> {
        // Coincidence is symmetric; same-operand support grouping and opposite
        // operand attribution must reuse the same expensive certification.
        let (first, second) = if first <= second { (first, second) } else { (second, first) };
        if let Some(value) = self.supports.borrow().get(&(first, second)) {
            return Ok(*value);
        }
        let value = coincident_supports(
            self.face(first).surface(),
            self.face(second).surface(),
            self.tolerance,
        )?;
        self.supports.borrow_mut().insert((first, second), value);
        Ok(value)
    }

    /// Imprint every trimmed intersection of two operand faces on both faces.
    fn imprint_intersections(&mut self) -> KernelResult<()> {
        let tolerance = self.tolerance;
        let faces = &self.faces;
        let original = &self.original;
        let original_bounds = &self.original_bounds;
        let imprinted = &mut self.imprinted;
        let samples = original.keys().map(|key| (*key, OnceCell::<KernelResult<TrimSamples>>::new()))
            .collect::<BTreeMap<_, _>>();
        for (first_index, first_face) in faces[0].iter().enumerate() {
            for (second_index, second_face) in faces[1].iter().enumerate() {
                let first_boundary = &original[&(0, first_index)];
                let second_boundary = &original[&(1, second_index)];
                let spatial_bounds = [
                    self.spatial_bounds[&(0, first_index)],
                    self.spatial_bounds[&(1, second_index)],
                ];
                if bounds_separated(spatial_bounds[0], spatial_bounds[1], tolerance) {
                    continue;
                }
                let first_bounds = original_bounds[&(0, first_index)];
                let second_bounds = original_bounds[&(1, second_index)];
                let IntersectionSet::Isolated(intersections) = first_face
                    .surface()
                    .intersect_surface_in_bounds(
                        second_face.surface(),
                        first_bounds,
                        second_bounds,
                        tolerance,
                    )?
                else {
                    continue;
                };
                for intersection in intersections {
                    let chart_copies = |pcurve: &Curve2,
                                        face: &FaceHandle,
                                        boundary: &[FaceCurve],
                                        bounds: [ParameterRange; 2]| {
                        let aligned = align_pcurve_to_face_chart(
                            pcurve,
                            None,
                            face.surface(),
                            boundary,
                            tolerance,
                        )?;
                        intersection_chart_copies(
                            intersection.curve.domain().bounds,
                            &aligned,
                            face.surface(),
                            bounds,
                            tolerance,
                        )
                    };
                    let first_copies = chart_copies(
                        &intersection.first_pcurve,
                        first_face,
                        first_boundary,
                        first_bounds,
                    )?;
                    let second_copies = chart_copies(
                        &intersection.second_pcurve,
                        second_face,
                        second_boundary,
                        second_bounds,
                    )?;
                    for first_pcurve in &first_copies {
                        for second_pcurve in &second_copies {
                            let first_samples = samples[&(0, first_index)]
                                .get_or_init(|| trim_samples(first_boundary)).as_ref().map_err(Clone::clone)?;
                            let second_samples = samples[&(1, second_index)]
                                .get_or_init(|| trim_samples(second_boundary)).as_ref().map_err(Clone::clone)?;
                            let trimmed = trim_to_faces_sampled(
                                &intersection.curve,
                                first_pcurve,
                                first_boundary,
                                second_pcurve,
                                second_boundary,
                                Some([first_samples, second_samples]),
                                spatial_bounds,
                                tolerance,
                            )?;
                            for range in trimmed {
                                // The original trim already participates in this
                                // arrangement. Reimprinting a coincident
                                // intersection would subdivide it using
                                // independently solved parameters, separating
                                // shared contact vertices.
                                for (key, pcurve, boundary) in [
                                    ((0, first_index), first_pcurve, first_boundary),
                                    ((1, second_index), second_pcurve, second_boundary),
                                ] {
                                    if boundary_curve_for_intersection(
                                        &intersection.curve,
                                        range,
                                        boundary,
                                        tolerance,
                                    )?
                                    .is_none()
                                    {
                                        imprinted
                                            .get_mut(&key)
                                            .expect("every face has a trim")
                                            .push(FaceCurve {
                                                provenance: Default::default(),
                                                curve: intersection.curve.clone(),
                                                pcurve: pcurve.clone(),
                                                range,
                                                loop_end: false,
                                                loop_kind: LoopKind::Outer,
                                                parameter_seam: false,
                                            });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Coincident supports form one arrangement even when their source faces
    /// have different trim loops. Overlay both complete boundary graphs on each
    /// support; otherwise a union of coplanar faces would leave each operand
    /// holding only an open fragment of the result outline.
    ///
    /// Intersections imprinted by other faces are included, not just original
    /// trim edges: the opposite shared-support face is skipped during
    /// publication, so omitting its intersections loses material boundaries.
    /// Intersections already represented by either trim are excluded, since
    /// projecting them again can introduce differently subdivided duplicates.
    fn overlay_coincident_boundaries(&mut self) -> KernelResult<()> {
        let tolerance = self.tolerance;
        let mut overlays = Vec::new();
        for first_key in self.keys(0) {
            for second_key in self.keys(1) {
                if !self.same_support(first_key, second_key)? {
                    continue;
                }
                let first_boundary = &self.original[&first_key];
                let second_boundary = &self.original[&second_key];
                let same_surface =
                    self.face(first_key).surface() == self.face(second_key).surface();
                for (target_key, source_key) in [(first_key, second_key), (second_key, first_key)] {
                    let target = self.face(target_key).surface();
                    let target_boundary = &self.original[&target_key];
                    // A union can enlarge a partial source face. Overlay in one
                    // complete chart, rather than clipping to the source trim.
                    let mut chart = self.original_bounds[&target_key];
                    for (axis, domain) in target.domains().iter().enumerate() {
                        if let Some(period) = domain.period {
                            chart[axis].end = chart[axis].start + period;
                        }
                    }
                    let source_boundary = &self.original[&source_key];
                    for (index, source) in self.imprinted[&source_key].iter().enumerate() {
                        if index >= source_boundary.len()
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
                        let projected: Curve2 = if same_surface {
                            source.pcurve.clone()
                        } else {
                            super::SurfaceProjectionPcurve2::new(
                                source.curve.clone(),
                                target.clone(),
                                source.pcurve.clone(),
                                source.range,
                                tolerance,
                            )?
                            .into()
                        };
                        let projected = align_pcurve_to_face_chart(
                            &projected,
                            Some(source.range),
                            target,
                            target_boundary,
                            tolerance,
                        )?;
                        for pcurve in intersection_chart_copies(
                            super::DomainBounds::Bounded(source.range),
                            &projected,
                            target,
                            chart,
                            tolerance,
                        )? {
                            overlays.push((
                                target_key,
                                FaceCurve {
                                    provenance: source.provenance.clone(),
                                    curve: source.curve.clone(),
                                    pcurve,
                                    range: source.range,
                                    loop_end: false,
                                    loop_kind: source.loop_kind,
                                    parameter_seam: source.parameter_seam,
                                },
                            ));
                        }
                    }
                }
            }
        }
        for (key, curve) in overlays {
            self.imprinted
                .get_mut(&key)
                .expect("every face has a trim")
                .push(curve);
        }
        Ok(())
    }

    /// Split every face into regions and keep those bounding the result,
    /// each attributed to the source faces whose trims it occupies.
    fn face_patches(&self) -> KernelResult<Vec<BrepPatch>> {
        let tolerance = self.tolerance;
        let first_classifier = BrepClassifier::for_compound(self.operands[0])?;
        let second_classifier = BrepClassifier::for_compound(self.operands[1])?;
        let classifiers = [&first_classifier, &second_classifier];
        let mut patches = Vec::new();
        for operand in 0..2 {
            let compound = self.operands[operand];
            for key in self.keys(operand) {
                let face = self.face(key);
                let mut original = self.original[&key].clone();
                let mut has_coincident_support = false;
                for opposite in self.keys(1 - operand) {
                    has_coincident_support |= self.same_support(key, opposite)?;
                }
                // Prefer an existing complete periodic band over a small contact
                // patch whose arbitrary chart cut can split that band. Retain the
                // first operand's chart when it already covers a full period.
                let mut shared_owner = None;
                if has_coincident_support {
                    let mut chart_bounds = [[
                        Point2::new(f64::INFINITY, f64::INFINITY),
                        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
                    ]; 2];
                    let mut singular = false;
                    for side in 0..2 {
                        for candidate_key in self.keys(side) {
                            if !self.same_support(key, candidate_key)? {
                                continue;
                            }
                            let candidate_boundary = &self.original[&candidate_key];
                            let bounds = self.original_bounds[&candidate_key];
                            for axis in 0..2 {
                                chart_bounds[side][0][axis] =
                                    chart_bounds[side][0][axis].min(bounds[axis].start);
                                chart_bounds[side][1][axis] =
                                    chart_bounds[side][1][axis].max(bounds[axis].end);
                            }
                            let candidate = self.face(candidate_key).surface();
                            for curve in candidate_boundary {
                                for parameter in [curve.range.start, curve.range.end] {
                                    let uv = curve.pcurve.evaluate(parameter)?.point;
                                    let (_, du, dv) = candidate.first_order(uv.x, uv.y)?;
                                    singular |= du.cross(dv).magnitude()
                                        <= tolerance.linear * du.magnitude().max(dv.magnitude());
                                }
                            }
                        }
                    }
                    let full_band = |side: usize| {
                        face.surface().domains().iter().enumerate().any(|(axis, domain)| {
                            domain.period.is_some_and(|period| {
                                chart_bounds[side][1][axis] - chart_bounds[side][0][axis]
                                    >= period - tolerance.parameter * 64.0
                            })
                        })
                    };
                    let canonical = usize::from(!full_band(0) && full_band(1));
                    if singular {
                        shared_owner = Some(canonical);
                    } else if operand != canonical {
                        continue;
                    }
                }
                let mut combined_curves = Vec::new();
                if has_coincident_support && shared_owner.is_none() {
                    // A shell can have several disjoint faces on this support.
                    // Arrange their combined boundaries once, or each face can
                    // independently publish the opposite operand's end cap.
                    let mut matching = Vec::new();
                    for other_key in self.keys(operand) {
                        if self.same_support(key, other_key)? {
                            matching.push(other_key);
                        }
                    }
                    if matching[0] != key {
                        continue;
                    }
                    for other_key in matching {
                        let other = self.face(other_key);
                        for (curve_index, curve) in self.imprinted[&other_key].iter().enumerate() {
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
                                curve.pcurve = align_pcurve_to_face_chart(
                                    &curve.pcurve,
                                    Some(curve.range),
                                    face.surface(),
                                    &original,
                                    tolerance,
                                )?;
                            }
                            if other_key != key && curve_index < self.original[&other_key].len() {
                                original.push(curve.clone());
                            }
                            combined_curves.push(curve);
                        }
                    }
                }
                let support = if has_coincident_support {
                    Support::Shared {
                        owner: shared_owner,
                    }
                } else {
                    Support::Own
                };
                let curves = if has_coincident_support && shared_owner.is_none() {
                    &combined_curves
                } else {
                    &self.imprinted[&key]
                };
                for mut patch in self.arrange_face(classifiers, key, &original, curves, support)? {
                    if has_coincident_support {
                        // A shared-support arrangement can contain several
                        // disjoint source faces. Attribute only the trimmed
                        // regions it uses.
                        for parent_operand in 0..2 {
                            let parent = self.operands[parent_operand];
                            for candidate_key in self.keys(parent_operand) {
                                let candidate = self.face(candidate_key);
                                if self.same_support(key, candidate_key)?
                                    && patch_uses_face(&patch, parent, candidate, tolerance)?
                                {
                                    patch.sources.push(candidate.persistent_id());
                                    if patch.retains_boundary(
                                        parent.topology(),
                                        candidate.topology_id(),
                                    )? {
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
                    patches.push(patch);
                }
            }
        }
        Ok(patches)
    }
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

pub(super) fn patch_complexity_key(patch: &BrepPatch) -> (u8, u8) {
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
        | Surface3::Blend(_)
        | Surface3::Ruled(_) => 4,
    };
    (boundary_complexity, surface_complexity)
}

fn boundary_curves(compound: &Compound, face: super::FaceId) -> KernelResult<Vec<FaceCurve>> {
    let topology = compound.topology();
    face_boundary_curves(topology, face, compound.tolerance())?.into_iter()
        .map(|curve| {
            let provenance = SegmentProvenance::from_coedge(topology, curve.provenance)?;
            Ok(curve.with_provenance(provenance))
        }).collect()
}

// A full periodic band can choose its chart cut freely. Keep that cut out of
// localized intersections so a closed material rim is not split by the chart.
fn rechart_periodic_band(
    surface: &Surface3,
    boundary: &mut [FaceCurve],
    bounds: [ParameterRange; 2],
    opposite: &[super::FaceHandle],
    source_bounds: &BTreeMap<FaceKey, [ParameterRange; 2]>,
    operand: usize,
    bounds3: Option<Bounds>,
    spatial_bounds: &BTreeMap<FaceKey, Option<Bounds>>,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    if !matches!(surface, Surface3::Cylinder(_)) || boundary.len() != 4
        || boundary.iter().filter(|curve| curve.parameter_seam).count() != 2
        || boundary.iter().any(|curve| !matches!(curve.pcurve, Curve2::Line(_)))
        || boundary.iter().any(|curve| !curve.parameter_seam && !matches!(curve.curve, Curve3::Circle(_)))
    {
        return Ok(false);
    }
    let period = std::f64::consts::TAU;
    if (bounds[0].span() - period).abs() > tolerance.parameter * 64.0 {
        return Ok(false);
    }
    let mut occupied = Vec::new();
    for (index, face) in opposite.iter().enumerate() {
        if bounds_separated(bounds3, spatial_bounds[&(operand, index)], tolerance) { continue; }
        let intersections = surface.intersect_surface_in_bounds(face.surface(), bounds,
            source_bounds[&(operand, index)], tolerance)?;
        let IntersectionSet::Isolated(intersections) = intersections else { continue; };
        for intersection in intersections {
            let super::DomainBounds::Bounded(range) = intersection.curve.domain().bounds else { continue; };
            let mut minimum = f64::INFINITY;
            let mut maximum = f64::NEG_INFINITY;
            for index in 0..=64 {
                let u = intersection.first_pcurve.evaluate(range.start + range.span() * index as f64 / 64.0)?.point.x;
                minimum = minimum.min(u);
                maximum = maximum.max(u);
            }
            if maximum - minimum >= period - tolerance.parameter * 64.0 { return Ok(false); }
            let start = (minimum - bounds[0].start).rem_euclid(period);
            let end = start + maximum - minimum;
            occupied.push((start, end.min(period)));
            if end > period { occupied.push((0.0, end - period)); }
        }
    }
    if occupied.is_empty() { return Ok(false); }
    occupied.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = occupied[0].1;
    let mut gap = (0.0, 0.0);
    for interval in &occupied[1..] {
        if interval.0 - end > gap.1 - gap.0 { gap = (end, interval.0); }
        end = end.max(interval.1);
    }
    if occupied[0].0 + period - end > gap.1 - gap.0 { gap = (end, occupied[0].0 + period); }
    if gap.1 - gap.0 <= tolerance.parameter * 64.0 { return Ok(false); }
    // Preserve a seam that already lies in the free interval.
    if occupied[0].0 > tolerance.parameter * 64.0 && end < period - tolerance.parameter * 64.0 {
        return Ok(false);
    }
    let seam = bounds[0].start + (gap.0 + gap.1) * 0.5;
    for curve in boundary {
        let Curve2::Line(line) = curve.pcurve else { unreachable!() };
        if curve.parameter_seam {
            let old_u = line.origin().x;
            let u = seam + if (old_u - bounds[0].end).abs() < tolerance.parameter * 64.0 { period } else { 0.0 };
            let origin = Point2::new(u, line.origin().y);
            curve.pcurve = super::Line2::new(origin, line.direction())?.into();
            let point = surface.evaluate(origin.x, origin.y)?.point;
            let next = surface.evaluate(origin.x + line.direction().x, origin.y + line.direction().y)?.point;
            curve.curve = super::Line3::new(point, next - point)?.into();
        } else {
            let shift = (seam - bounds[0].start) / line.direction().x;
            curve.range = ParameterRange::new(curve.range.start + shift, curve.range.end + shift)?;
        }
        curve.provenance = curve.provenance.modified();
        curve.provenance.vertices = Default::default();
    }
    Ok(true)
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
    let mut points = Vec::with_capacity(9);
    for candidate in boundary {
        let mut parameters = Vec::with_capacity(9);
        let mut matches = true;
        for index in 0..=8 {
            if index == points.len() {
                let parameter = range.start + range.span() * index as f64 / 8.0;
                let point = intersection.evaluate(parameter)?.point;
                let allowed = tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0;
                points.push((point, allowed));
            }
            let (point, allowed) = points[index];
            if index == 0 && super::bounds::curve3(&candidate.curve, candidate.range)
                .is_ok_and(|bounds| !bounds.expanded(allowed).contains(point))
            {
                matches = false;
                break;
            }
            let Ok(projection) = candidate
                .curve
                .project_on_trim(point, candidate.range, tolerance)
            else {
                matches = false;
                break;
            };
            let reference = parameters
                .last()
                .copied()
                .unwrap_or(candidate.range.middle());
            let projected = candidate.curve.domain().nearest(projection.parameter, reference);
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

impl Arrangement<'_> {
/// Split the face `key`, bounded by `original_boundary`, along `curves` and
/// keep the regions on the result boundary with their material orientation.
fn arrange_face(
    &self,
    classifiers: [&BrepClassifier<'_>; 2],
    key: FaceKey,
    original_boundary: &[FaceCurve],
    curves: &[FaceCurve],
    support: Support,
) -> KernelResult<Vec<BrepPatch>> {
    let operand = key.0;
    let face = self.face(key);
    let surface = face.surface();
    let source_orientation = self.operands[operand]
        .topology()
        .face(face.topology_id())?
        .orientation();
    let (has_coincident_support, shared_owner) = match support {
        Support::Own => (false, None),
        Support::Shared { owner } => (true, owner),
    };
    let kind = self.kind;
    let classification_offset = self.classification_offset;
    let tolerance = self.tolerance;
    if !has_coincident_support && curves.len() == original_boundary.len()
        && classifiers[1 - operand].separated_from(self.spatial_bounds[&key])
    {
        return if kind == BooleanKind::Union || (kind == BooleanKind::Difference && operand == 0) {
            Ok(vec![unchanged_face_patch(surface, original_boundary, source_orientation, source_orientation, tolerance)?])
        } else {
            Ok(Vec::new())
        };
    }
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
    let domains = surface.domains();
    let stepped_chart = domains.iter().enumerate().any(|(axis, domain)| {
        domain.period.is_some_and(|period| {
            uv_bounds[1][axis] - uv_bounds[0][axis] > period + tolerance.parameter * 64.0
        })
    });
    let chart_bounds = face_parameter_bounds(original_boundary, tolerance)?;
    let original_samples = trim_samples(original_boundary)?;
    let classify_orientation_at = |uv: Point2| -> KernelResult<Option<FaceOrientation>> {
        let value = surface.evaluate(uv.x, uv.y)?;
        if let Some(owner) = shared_owner {
            // Arrange each source trim as one chart cell. This retains pole
            // meridians and periodic cuts instead of collapsing an entire
            // closed support into a singular one-face boundary. The other
            // operand contributes only regions outside the owner's trims.
            if !point_in_sampled_trim(uv, original_boundary, &original_samples)?
                || (operand != owner && classifiers[owner].covers_on_support(surface, uv, value.point)?)
            {
                return Ok(None);
            }
        }
        let parameter_normal = value.normal()?;
        if has_coincident_support {
            // On a shared support, exact oriented trims determine the local
            // material sides. A fixed normal probe can cross a nearby blend
            // or thin wall and invent an interface between tangent cutters.
            let first = classifiers[0].sides_on_support(surface, uv, value.point, parameter_normal)?;
            let second = classifiers[1].sides_on_support(surface, uv, value.point, parameter_normal)?;
            let below_occupied = occupied(kind, first.0, second.0);
            let above_occupied = occupied(kind, first.1, second.1);
            let coincident_orientation = match (below_occupied, above_occupied) {
                (true, false) => Some(FaceOrientation::Forward),
                (false, true) => Some(FaceOrientation::Reversed),
                _ => None,
            };
            return Ok(coincident_orientation);
        }
        let mut other = classifiers[1 - operand].classify(value.point)?;
        if other == PointClassification::Boundary {
            // A probe can land on a tangential contact line crossing the
            // region, such as a symmetric meridian. The region then lies on
            // one side of the other boundary just beside that line.
            let step = classification_offset / 8.0;
            let mut beside = Vec::new();
            for (direction, offset) in [(value.du, Vec2::unit_x()), (value.dv, Vec2::unit_y())] {
                if direction.magnitude() <= tolerance.linear { continue; }
                for sign in [1.0, -1.0] {
                    let probe = uv + offset * (sign * (step / direction.magnitude()).min(uv_probe_limit));
                    if domains.iter().enumerate().any(|(axis, domain)| {
                        domain.period.is_none() && !domain.contains(probe[axis], 0.0)
                    }) || !point_in_sampled_trim(probe, original_boundary, &original_samples)? {
                        continue;
                    }
                    let location = classifiers[1 - operand].classify(surface.evaluate(probe.x, probe.y)?.point)?;
                    if location != PointClassification::Boundary { beside.push(location); }
                }
            }
            if beside.first().is_some_and(|first| beside.iter().all(|location| location == first)) {
                other = beside[0];
            }
        }
        if other == PointClassification::Boundary {
            // Resolve a tangent contact by its two material sides, including
            // general supports that share a tangent plane but are not coincident.
            let own = (source_orientation == FaceOrientation::Forward,
                source_orientation == FaceOrientation::Reversed);
            let other = classifiers[1 - operand].sides_at_point(value.point, parameter_normal)?;
            let [first, second] = if operand == 0 { [own, other] } else { [other, own] };
            return Ok(match (occupied(kind, first.0, second.0), occupied(kind, first.1, second.1)) {
                (true, false) => Some(FaceOrientation::Forward),
                (false, true) => Some(FaceOrientation::Reversed),
                _ => None,
            });
        }
        let other_contains = other == PointClassification::Inside;
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
        // Keep the source trim's periodic representative. Relocating seams
        // around a cut can produce a stepped chart wider than one period;
        // clipping it to a rectangle would discard a real boundary segment.
        // Newly exposed material uses the default one-period chart only when
        // none of its copies already belongs to the source trim.
        if has_coincident_support {
            let outside = domains.iter().enumerate().any(|(axis, domain)| {
                domain.period.is_some_and(|period| {
                    uv[axis] < uv_bounds[0][axis] || uv[axis] > uv_bounds[0][axis] + period
                })
            });
            let in_source_chart = stepped_chart
                && point_in_sampled_trim(uv, original_boundary, &original_samples)?;
            if !in_source_chart && (outside || (stepped_chart && point_in_sampled_periodic_trim(
                uv, original_boundary, &original_samples, domains, chart_bounds, tolerance,
            )?)) {
                return Ok(None);
            }
        }
        if surface
            .domains()
            .iter()
            .enumerate()
            .any(|(axis, domain)| domain.period.is_none() && !domain.contains(uv[axis], 0.0))
        {
            return Ok(None);
        }
        if !has_coincident_support && !point_in_sampled_trim(uv, original_boundary, &original_samples)? {
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

    arrange_surface_regions(
        surface, curves, classification_offset, uv_probe_limit, tolerance, orientation_at,
    )
}
}

/// Split a support's candidate trims and retain the regions selected by the
/// operation's material classifier. Offsets use the same arrangement as Booleans
/// when a rolling join intersects a third support.
pub(super) fn arrange_surface_regions(
    surface: &Surface3,
    curves: &[FaceCurve],
    classification_offset: f64,
    uv_probe_limit: f64,
    tolerance: ModelTolerance,
    orientation_at: impl Fn(Point2) -> KernelResult<Option<FaceOrientation>>,
) -> KernelResult<Vec<BrepPatch>> {
    arrange_regions(surface, curves, classification_offset, uv_probe_limit, tolerance,
        // Probe clearance is not a geometric equality tolerance. Distinct arcs
        // bounding a thin lens can share endpoints and parallel mid-tangents.
        tolerance.linear * 16.0, None, orientation_at)
}

struct TrimContact {
    indices: [usize; 2],
    parameters: [f64; 2],
}

/// Split a curved chart at a self-touching trim vertex when both resulting
/// contours bound outer regions of the original face.
fn partition_tangent_trims(
    patches: &[BrepPatch],
    tolerance: ModelTolerance,
) -> KernelResult<Option<Vec<BrepPatch>>> {
    let mut result = Vec::new();
    let mut changed = false;
    for patch in patches {
        if patch.loops.len() != 1 || matches!(patch.surface, Surface3::Plane(_)) {
            result.push(patch.clone());
            continue;
        }
        let domains = patch.surface.domains();
        let mut previous = None;
        let segments = &patch.loops[0].segments;
        let boundary = segments.iter().enumerate().map(|(index, segment)| {
            let mut pcurve = segment.pcurve.clone();
            if let Some(end) = previous {
                let start = pcurve.first_order(segment.range.start)?.0;
                let shift = super::whole_periods(domains, end - start);
                if shift.magnitude2() > 0.0 {
                    pcurve = super::trim::translate_pcurve(&pcurve, shift, tolerance)?;
                }
            }
            previous = Some(pcurve.first_order(segment.range.end)?.0);
            Ok(FaceCurve {
                provenance: segment.provenance.clone(),
                curve: segment.curve.clone(),
                pcurve,
                range: segment.range,
                loop_end: index + 1 == segments.len(),
                loop_kind: LoopKind::Outer,
                parameter_seam: false,
            })
        }).collect::<KernelResult<Vec<_>>>()?;
        let mut pinch = pinched_trim_contact(&boundary, tolerance)?;
        if pinch.is_none() {
            let starts = boundary.iter().map(|edge| edge.pcurve.first_order(edge.range.start).map(|value| value.0))
                .collect::<KernelResult<Vec<_>>>()?;
            'contact: for (first, point) in starts.iter().enumerate() {
                for (second, other) in starts.iter().enumerate().skip(first + 2) {
                    if first == 0 && second + 1 == starts.len() { continue; }
                    if point.distance(*other) <= tolerance.parameter * 64.0 {
                        pinch = Some(TrimContact {
                            indices: [first, second],
                            parameters: [boundary[first].range.start, boundary[second].range.start],
                        });
                        break 'contact;
                    }
                }
            }
        }
        let Some(pinch) = pinch else {
            result.push(patch.clone());
            continue;
        };
        let [first, second] = pinch.indices;
        let mut segments = segments.clone();
        let mut tails = [segments[first].clone(), segments[second].clone()];
        for ((index, parameter), tail) in pinch.indices.into_iter().zip(pinch.parameters).zip(&mut tails) {
            let segment = &mut segments[index];
            let range = segment.range;
            segment.range.end = parameter;
            tail.range.start = parameter;
            segment.provenance = segment.provenance.trimmed(range, segment.range);
            tail.provenance = tail.provenance.trimmed(range, tail.range);
        }
        let [first_tail, second_tail] = tails;
        let mut regions = [
            [vec![first_tail], segments[first + 1..=second].to_vec()].concat(),
            [vec![second_tail], segments[second + 1..].to_vec(), segments[..=first].to_vec()].concat(),
        ];
        for segments in &mut regions {
            segments.retain(|segment| segment.range.span().abs() > tolerance.parameter * 64.0);
        }
        let sign = patch.orientation.sign();
        if trim_loop_area(regions[0].iter(), tolerance)? * sign <= tolerance.parameter
            || trim_loop_area(regions[1].iter(), tolerance)? * sign <= tolerance.parameter
        {
            result.push(patch.clone());
            continue;
        }
        result.extend(regions.into_iter().map(|segments| BrepPatch {
            preserved: Vec::new(),
            sources: patch.sources.clone(),
            surface: patch.surface.clone(),
            orientation: patch.orientation,
            loops: vec![BrepPatchLoop { kind: LoopKind::Outer, segments }],
        }));
        changed = true;
    }
    Ok(changed.then_some(result))
}

fn pinched_trim_contact(
    boundary: &[FaceCurve],
    tolerance: ModelTolerance,
) -> KernelResult<Option<TrimContact>> {
    let bounds = boundary.iter().map(|edge| super::bounds::curve3(&edge.curve, edge.range).ok())
        .collect::<Vec<_>>();
    for (index, first) in boundary.iter().enumerate() {
        let a = super::TrimmedCurve2::new(first.pcurve.clone(), first.range)?;
        for (other, second) in boundary.iter().enumerate().skip(index + 1) {
            if bounds_separated(bounds[index], bounds[other], tolerance) { continue; }
            let b = super::TrimmedCurve2::new(second.pcurve.clone(), second.range)?;
            let intersection = super::wire::curve_pair_intersection(&a, &b, tolerance)?;
            for hit in intersection.hits {
                let mut parameters = [
                    first.pcurve.domain().nearest(hit.first_parameter, first.range.middle()),
                    second.pcurve.domain().nearest(hit.second_parameter, second.range.middle()),
                ];
                let tangent = first.pcurve.first_order(parameters[0])?.1.normalize();
                // At a pinch the oriented boundary tangents point in
                // opposite directions. Resolve their tangency separately
                // from the ill-conditioned crossing of two parallel curves.
                for _ in 0..12 {
                    let a = first.pcurve.evaluate(parameters[0])?;
                    let b = second.pcurve.evaluate(parameters[1])?;
                    let da = a.first_derivative;
                    let db = b.first_derivative;
                    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
                    if da.dot(db) * first.range.span() * second.range.span() >= 0.0
                        || cross(da.normalize(), db.normalize()).abs() > 1.0e-3
                    {
                        break;
                    }
                    let separation = (a.point - b.point).dot(tangent);
                    let sine = cross(da, db);
                    let Some(step) = crate::linalg::solve_linear([
                        [da.dot(tangent), -db.dot(tangent)],
                        [cross(a.second_derivative, db), cross(da, b.second_derivative)],
                    ], [-separation, -sine]) else { break; };
                    if step[0].abs().max(step[1].abs()) <= tolerance.parameter * 64.0 {
                        if a.point.distance(b.point) > tolerance.linear * 0.125 { break; }
                        let interior = |edge: &FaceCurve, t: f64| {
                            t > edge.range.start.min(edge.range.end) + tolerance.parameter * 64.0
                                && t < edge.range.start.max(edge.range.end) - tolerance.parameter * 64.0
                        };
                        let adjacent = other == index + 1 || (index == 0 && other + 1 == boundary.len());
                        if !adjacent || interior(first, parameters[0]) || interior(second, parameters[1]) {
                            return Ok(Some(TrimContact { indices: [index, other], parameters }));
                        }
                        break;
                    }
                    parameters[0] += step[0];
                    parameters[1] += step[1];
                    if !first.range.contains(parameters[0], tolerance)
                        || !second.range.contains(parameters[1], tolerance) { break; }
                }
            }
        }
    }
    Ok(None)
}

pub(super) fn arrange_section_regions(
    surface: &Surface3,
    curves: &[FaceCurve],
    uv_probe_limit: f64,
    tolerance: ModelTolerance,
    plane: super::Plane3,
    keep: super::SplitKeep,
    orientation: FaceOrientation,
    orientation_at: impl Fn(Point2) -> KernelResult<Option<FaceOrientation>>,
) -> KernelResult<Vec<BrepPatch>> {
    arrange_regions(surface, curves, 0.0, uv_probe_limit, tolerance,
        tolerance.linear * 2.0, Some((plane, keep, orientation)), orientation_at)
}

fn arrange_regions(
    surface: &Surface3,
    curves: &[FaceCurve],
    classification_offset: f64,
    uv_probe_limit: f64,
    tolerance: ModelTolerance,
    duplicate_distance: f64,
    section: Option<(super::Plane3, super::SplitKeep, FaceOrientation)>,
    orientation_at: impl Fn(Point2) -> KernelResult<Option<FaceOrientation>>,
) -> KernelResult<Vec<BrepPatch>> {
    let mut split_parameters = curves
        .iter()
        .map(|curve| vec![curve.range.start, curve.range.end])
        .collect::<Vec<_>>();
    // Reuse each curve's samples and certified enclosure across pair queries.
    let samples = trim_samples(curves)?;
    let spatial_bounds = curves.iter()
        .map(|curve| super::bounds::curve3(&curve.curve, curve.range).ok())
        .collect::<Vec<_>>();
    for first in 0..curves.len() {
        for second in first + 1..curves.len() {
            // The source loops already form a valid arrangement. Only the
            // section can introduce new intersections into their boundaries.
            if section.is_some()
                && !curves[first].provenance.edges.is_empty()
                && !curves[second].provenance.edges.is_empty()
            {
                continue;
            }
            if bounds_separated(spatial_bounds[first], spatial_bounds[second], tolerance) {
                continue;
            }
            let hits = if section.is_some() && !curves[first].provenance.edges.is_empty() {
                // Anchor section curves to existing trim vertices.
                super::trim::intersect_sampled_trims(&curves[second], &curves[first],
                    Some([&samples[second], &samples[first]]), false, tolerance, true)?
                    .into_iter().map(|(cut, trim)| (trim, cut)).collect()
            } else {
                curve_intersections_sampled(&curves[first], &curves[second],
                    Some([&samples[first], &samples[second]]), false, tolerance)?
            };
            for (first_parameter, second_parameter) in hits {
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
        for (index, candidate) in nodes.iter().enumerate() {
            let delta = *candidate - point;
            // Intersection solvers converge in model units, not UV units.
            // Use the local surface metric while keeping distinct periodic
            // chart endpoints (whose raw UV separation is a full period).
            // At a collapsed rail the metric loses a direction, so also
            // verify the actual spatial separation before welding.
            if candidate.distance(point) <= tolerance.parameter * 64.0
                || ((value.du * delta.x + value.dv * delta.y).magnitude() <= linear
                    && surface.evaluate(candidate.x, candidate.y)?.point.distance(value.point) <= linear)
            {
                return Ok(index);
            }
        }
        nodes.push(point);
        Ok(nodes.len() - 1)
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
            let collapse_distance = tolerance.linear * if section.is_some() { 1.0 } else { 8.0 };
            if start.distance(end) <= collapse_distance
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
                    if start.distance(point) > collapse_distance {
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
        if let Some((plane, keep, orientation)) = section {
            let side = if keep == super::SplitKeep::Left { 1.0 } else { -1.0 };
            let signed = |point: Point3| (point - plane.origin()).dot(plane.normal()) * side;
            let (point, derivative) = piece.curve.first_order(parameter)?;
            let slot = if orientation == FaceOrientation::Forward { 0 } else { 1 };
            if !piece.provenance.edges.is_empty() {
                let (surface_point, du, dv) = surface.first_order(uv.x, uv.y)?;
                let distance = signed(surface_point);
                let uncertainty = (distance - signed(point)).abs().max(tolerance.linear);
                let retain = if distance.abs() > uncertainty { distance < 0.0 } else {
                    let tangent = tangent * piece.range.span().signum() * orientation.sign();
                    let inward = Vec2::new(-tangent.y, tangent.x).normalize();
                    let step = (tolerance.linear * 128.0 / (du * inward.x + dv * inward.y).magnitude()).min(uv_probe_limit);
                    let probe = uv + inward * step;
                    signed(surface.first_order(probe.x, probe.y)?.0) < 0.0
                };
                if retain { selected[slot].push(piece.clone()); }
            } else if orientation_at(uv)?.is_some() {
                let outward = surface.normal(uv.x, uv.y)? * orientation.sign();
                let direction = outward.cross(plane.normal() * side);
                selected[slot].push(if derivative.dot(direction) * piece.range.span().signum() >= 0.0 {
                    piece.clone()
                } else { piece.reversed() });
            }
            continue;
        }
        if matches!(piece.curve, Curve3::SurfaceIntersection(_))
            && !((matches!(surface, Surface3::RollingBall(_)) && surface.domains()[0].period.is_none()
                || matches!(surface, Surface3::Swept(_) | Surface3::Revolved(_)))
                && matches!(piece.pcurve, Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_)))
        {
            // Center side probes on the corrected intersection. Seed the
            // projection from the tracing p-curve to stay on its local branch.
            let point = piece.curve.evaluate(parameter)?.point;
            let projection = surface.project_near(point, uv, tolerance)?;
            uv = nearest_uv(
                surface.domains(),
                Point2::new(projection.parameters.0, projection.parameters.1),
                uv,
            );
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
        for (curve, bounds) in curves.iter().zip(&spatial_bounds) {
            // A curve cannot limit the clearance of probes beside itself.
            // Numerical projection onto an implicit trim can otherwise turn
            // its solver residual into an artificial micrometer-sized gap.
            if curve.curve == piece.curve { continue; }
            // Only curves within four probe lengths can reduce clearance.
            // Unknown enclosures still use the exact projection below.
            if bounds.is_some_and(|bounds| bounds.expanded(probe * 4.0)
                .separated(Bounds(surface_value.point, surface_value.point), tolerance.linear * 16.0)) {
                continue;
            }
            let Ok(projection) =
                curve
                    .curve
                    .project_on_trim(surface_value.point, curve.range, tolerance)
            else {
                continue;
            };
            // Allow for both curves' residuals without discarding resolved
            // narrow gaps and probing across the neighboring boundary.
            let uncertainty = if let Curve3::SurfaceIntersection(intersection) = &curve.curve {
                intersection.positional_uncertainty(projection.parameter)?.max(tolerance.linear) * 2.0
            } else { tolerance.linear * 2.0 };
            if projection.distance > uncertainty {
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
                        let parameter = candidate
                            .curve
                            .domain()
                            .nearest(projection.parameter, candidate.range.middle());
                        projection.distance <= duplicate_distance
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
                        first.point.distance(second.point) <= duplicate_distance
                            && first
                                .first_derivative
                                .normalize()
                                .dot(second.first_derivative.normalize())
                                .abs()
                                >= 1.0 - tolerance.angular * 16.0
                    }
                    _ => false,
                };
                projected_match || sampled_match
            });
            if let Some(index) = duplicate {
                if section.is_some() && piece.parameter_seam && unique[index].parameter_seam
                    && piece.start_node == unique[index].end_node && piece.end_node == unique[index].start_node
                    && piece.start_node != piece.end_node
                {
                    // Both sides of a chart cut can land in the same UV lift.
                    // Their opposite uses cancel in the combined region.
                    unique.remove(index);
                    continue;
                }
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
        if section.is_none() { add_periodic_seams(&mut unique, &pieces, surface, tolerance)?; }
        let loops = connect_loops(&unique, tolerance, surface, orientation, section.is_none())?;
        if loops.is_empty() {
            continue;
        }
        let orientation_sign = orientation.sign();
        let mut loops = loops
            .into_iter()
            .map(|loop_| {
                let area_tolerance = if section.is_some() {
                    let polygon = loop_polyline(&loop_)?;
                    let low = polygon.iter().fold(Point2::new(f64::INFINITY, f64::INFINITY), |a, b| Point2::new(a.x.min(b.x), a.y.min(b.y)));
                    let high = polygon.iter().fold(Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| Point2::new(a.x.max(b.x), a.y.max(b.y)));
                    tolerance.parameter.max((high.x - low.x) * (high.y - low.y) * 1e-7)
                } else { tolerance.parameter };
                let area = loop_.iter().enumerate().try_fold(0.0, |sum, (index, piece)| {
                    // A pole closes in space while its incident meridians
                    // retain different angular coordinates. Include that UV
                    // bridge when deciding winding; it has no spatial edge.
                    let next = loop_[(index + 1) % loop_.len()].start_uv;
                    let bridge = (piece.end_uv.x * next.y - piece.end_uv.y * next.x) * 0.5;
                    Ok::<_, KernelError>(
                        sum + bridge + if section.is_some() {
                            piece.pcurve.precise_area_contribution(piece.range, area_tolerance)?
                        } else {
                            piece.pcurve.signed_area_contribution(piece.range, tolerance.parameter)?
                        },
                    )
                })?;
                Ok::<_, KernelError>((loop_, area))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        // Enclosing loops must be classified before their holes, regardless
        // of which source face supplied the first arrangement edge.
        loops.sort_by(|first, second| second.1.abs().total_cmp(&first.1.abs()));
        let curved_nesting = loops.len() > 1 && (section.is_some() || matches!(surface, Surface3::Plane(_)));
        let mut classified = Vec::<(Vec<DirectedPiece>, Vec<Point2>, f64, bool, Option<usize>)>::new();
        let mut loop_trims: Vec<(Vec<FaceCurve>, _)> = Vec::new();
        for (mut loop_, area) in loops {
            if area.abs() <= tolerance.parameter.powi(2) {
                continue;
            }
            let polygon = loop_polyline(&loop_)?;
            // Sample inside the loop: a concave loop's centroid can lie
            // outside it, while a boundary sample can lie on a shared chart
            // seam and mistake an adjacent region for an enclosing one.
            let sample = polygon_interior_point(&polygon)?;
            let mut nesting = 0;
            let mut parent = None;
            for (index, (_, enclosing, enclosing_area, is_outer, _)) in classified.iter().enumerate() {
                if enclosing_area.abs() <= area.abs() {
                    continue;
                }
                let contains = if curved_nesting {
                    let (boundary, samples) = &loop_trims[index];
                    point_in_sampled_trim(sample, boundary, samples)?
                } else {
                    point_in_polygon(sample, enclosing)
                };
                if contains {
                    nesting += 1;
                    if *is_outer { parent = Some(index); }
                }
            }
            let outer = if section.is_some() {
                // Plane clipping already orients every retained boundary
                // from the source face and the cut normal. A coarse polygon
                // nesting test can reverse a resolved thin crescent.
                area.signum() == orientation_sign
            } else {
                nesting % 2 == 0
            };
            let desired_sign = orientation_sign * if outer { 1.0 } else { -1.0 };
            if area.signum() != desired_sign.signum() {
                loop_.reverse();
                loop_ = loop_.into_iter().map(|piece| piece.reversed()).collect();
            }
            if curved_nesting {
                let boundary = loop_.iter().enumerate().map(|(index, piece)| FaceCurve {
                    provenance: piece.provenance.clone(),
                    curve: piece.curve.clone(),
                    pcurve: piece.pcurve.clone(),
                    range: piece.range,
                    loop_end: index + 1 == loop_.len(),
                    loop_kind: LoopKind::Outer,
                    parameter_seam: false,
                }).collect::<Vec<_>>();
                let samples = trim_samples(&boundary)?;
                loop_trims.push((boundary, samples));
            }
            classified.push((loop_, polygon, area, outer, parent));
        }
        for outer_index in classified
            .iter()
            .enumerate()
            .filter(|(_, value)| value.3)
            .map(|(index, _)| index)
        {
            let outer = &classified[outer_index].0;
            let mut patch_loops = vec![BrepPatchLoop {
                kind: LoopKind::Outer,
                segments: patch_segments(outer),
            }];
            // Each hole belongs to its smallest enclosing outer loop.
            for (hole, _, _, is_outer, parent) in &classified {
                if !is_outer && *parent == Some(outer_index) {
                    patch_loops.push(BrepPatchLoop {
                        kind: LoopKind::Inner,
                        segments: patch_segments(hole),
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
        if degree[&start].is_multiple_of(2) {
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
        if degree[&dangling[first]].is_multiple_of(2) {
            continue;
        }
        for second in first + 1..dangling.len() {
            if degree[&dangling[second]].is_multiple_of(2) {
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

pub(super) fn unchanged_face_patch(
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
        return Err(topology_error("an unchanged BREP face contains an open trim loop"));
    }
    let reverse = output_orientation != source_orientation;
    let mut patch_loops = Vec::new();
    for (kind, segments) in loops {
        let mut patch = BrepPatchLoop { kind, segments };
        if reverse {
            patch.reverse();
        }
        patch_loops.push(patch);
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
    pieces: &[DirectedPiece],
    tolerance: ModelTolerance,
    surface: &Surface3,
    orientation: FaceOrientation,
    reverse_seams: bool,
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
                    pieces[*index].start_node == end_node
                        || (reverse_seams && pieces[*index].parameter_seam && pieces[*index].end_node == end_node)
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
                        let angle = previous_tangent
                            .x
                            .mul_add(tangent.y, -previous_tangent.y * tangent.x)
                            .atan2(previous_tangent.dot(tangent));
                        // Follow the material side at a point contact. Reversed
                        // faces wind clockwise in the surface chart, so choosing
                        // a counterclockwise turn would splice separate caps.
                        (angle * orientation.sign()).rem_euclid(std::f64::consts::TAU)
                    };
                    let distance = |index: usize| pieces[index].start_uv.distance2(end_uv);
                    if !reverse_seams {
                        // At a pole multiple longitudes share one vertex. Pair
                        // the nearest chart endpoints before comparing turns.
                        let a = distance(*first);
                        let b = distance(*second);
                        if a.max(b) > tolerance.parameter.powi(2) * 4096.0 && a != b {
                            return a.total_cmp(&b);
                        }
                    }
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
        }
        loops.push(loop_);
    }
    Ok(loops)
}

fn patch_segments(loop_: &[DirectedPiece]) -> Vec<BrepPatchSegment> {
    loop_
        .iter()
        .map(|piece| BrepPatchSegment {
            curve: piece.curve.clone(),
            provenance: piece.provenance.clone(),
            // Arrangement p-curves are exact and retain the selected face chart.
            // The spatial edge can carry the opposite use of a parameter seam.
            pcurve: piece.pcurve.clone(),
            range: piece.range,
        })
        .collect()
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

fn polygon_interior_point(polygon: &[Point2]) -> KernelResult<Point2> {
    // Place a scanline between vertex heights, avoiding horizontal edges and
    // vertex hits. Paired crossings bound intervals inside a simple polygon,
    // including concave ones whose centroid lies outside the boundary.
    let mut heights = polygon.iter().map(|point| point.y).collect::<Vec<_>>();
    heights.sort_by(f64::total_cmp);
    heights.dedup();
    let band = heights
        .windows(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .ok_or_else(|| {
            geometry_error("Boolean face loop has no interior")
        })?;
    let y = band[0] + (band[1] - band[0]) * 0.5;
    let mut crossings = Vec::new();
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        if (first.y > y) != (second.y > y) {
            crossings.push(
                first.x + (second.x - first.x) * (y - first.y) / (second.y - first.y),
            );
        }
    }
    crossings.sort_by(f64::total_cmp);
    let interval = crossings
        .chunks_exact(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .ok_or_else(|| {
            geometry_error("Boolean face loop has no interior")
        })?;
    Ok(Point2::new(interval[0] + (interval[1] - interval[0]) * 0.5, y))
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

/// Equivalent trim curves can have different parameterizations. Once the
/// endpoints agree, project both midpoints before rejecting an edge match.
pub(super) fn same_patch_edge(
    (first, a): (&BrepPatchSegment, [Point3; 3]),
    (second, b): (&BrepPatchSegment, [Point3; 3]),
    linear: f64,
    tolerance: ModelTolerance,
) -> bool {
    same_unoriented_edge(a, b, linear)
        || (((a[0].distance(b[0]) <= linear && a[2].distance(b[2]) <= linear)
            || (a[0].distance(b[2]) <= linear && a[2].distance(b[0]) <= linear))
            && first.curve.project_on_trim(b[1], first.range, tolerance)
                .is_ok_and(|projection| projection.distance <= linear)
            && second.curve.project_on_trim(a[1], second.range, tolerance)
                .is_ok_and(|projection| projection.distance <= linear))
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
            if patch_contains_patch(&patches[first], &patches[second], &samples[second], tolerance)? {
                let sources = patches[second].sources.clone();
                patches[first].sources.extend(sources);
                duplicate.insert(second);
            } else if patch_contains_patch(&patches[second], &patches[first], &samples[first], tolerance)? {
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
    let offset = whole_periods(domains, box_center(bounds) - box_center(original_bounds));
    // Move the complete chart together. Shifting individual seam uses can
    // collapse a full-period region into a zero-width boundary.
    if offset.magnitude2() != 0.0 {
        for curve in &mut original {
            curve.pcurve = translate_pcurve(&curve.pcurve, offset, tolerance)?;
        }
    }
    let original_bounds = face_parameter_bounds(&original, tolerance)?;
    // Every probe and curve pair reuses these chords of both trims.
    let boundary_samples = trim_samples(&boundary)?;
    let original_samples = trim_samples(&original)?;
    let inside = |point: Point2, curves: &[FaceCurve], samples: &TrimSamples, bounds: [ParameterRange; 2]| {
        point_in_sampled_trim(nearest_uv(domains, point, box_center(bounds)), curves, samples)
    };
    for (a, a_samples, ab, b, b_samples, bb) in [
        (&boundary, &boundary_samples, bounds, &original, &original_samples, original_bounds),
        (&original, &original_samples, original_bounds, &boundary, &boundary_samples, bounds),
    ] {
        for (curve, curve_samples) in a.iter().zip(a_samples.iter()).filter(|(curve, _)| !curve.parameter_seam) {
            let mut parameters = vec![curve.range.start, curve.range.end];
            for (other, other_samples) in b.iter().zip(b_samples.iter()) {
                parameters.extend(
                    curve_intersections_sampled(curve, other, Some([curve_samples, other_samples]), false, tolerance)?
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
                    if inside(probe, a, a_samples, ab)? && inside(probe, b, b_samples, bb)? {
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
    candidate: &BrepPatch,
    candidate_edges: &[[Point3; 3]],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let boundaries = |patch: &BrepPatch| {
        patch.loops.iter().flat_map(|loop_| loop_.segments.iter().enumerate().map(|(index, segment)| FaceCurve {
            provenance: Default::default(), curve: segment.curve.clone(),
            pcurve: segment.pcurve.clone(), range: segment.range,
            loop_end: index + 1 == loop_.segments.len(), loop_kind: loop_.kind, parameter_seam: false,
        })).collect::<Vec<_>>()
    };
    let boundary = boundaries(container);
    let samples = trim_samples(&boundary)?;
    let bounds = face_parameter_bounds(&boundary, tolerance)?;
    let contains = |point: Point3| -> KernelResult<bool> {
        let projection = container.surface.project(point, tolerance)?;
        let domains = container.surface.domains();
        let uv = nearest_uv(domains, Point2::new(projection.parameters.0, projection.parameters.1), box_center(bounds));
        Ok(projection.distance <= tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0
            && point_in_sampled_periodic_trim(uv, &boundary, &samples, domains, bounds, tolerance)?)
    };
    for &point in candidate_edges.iter().flatten() {
        if !contains(point)? { return Ok(false); }
    }
    // Coincident chart boundaries do not imply coincident material. Two
    // complementary patches of a closed support can have the same rim.
    let candidate_boundary = boundaries(candidate);
    let candidate_samples = trim_samples(&candidate_boundary)?;
    let mut interior = false;
    for edge in candidate_boundary.iter().filter(|edge| edge.loop_kind == LoopKind::Outer) {
        let (uv, tangent) = edge.pcurve.first_order(edge.range.middle())?;
        if tangent.magnitude2() <= tolerance.parameter.powi(2) { continue; }
        let inward = Vec2::new(-tangent.y, tangent.x).normalize()
            * (candidate.orientation.sign() * edge.range.span().signum());
        let (point, du, dv) = candidate.surface.first_order(uv.x, uv.y)?;
        let speed = (du * inward.x + dv * inward.y).magnitude();
        if speed <= tolerance.linear { continue; }
        let probe = uv + inward * (tolerance.linear_at_scale(point.to_vec().magnitude()) * 32.0 / speed);
        if point_in_sampled_trim(probe, &candidate_boundary, &candidate_samples)? {
            if !contains(candidate.surface.first_order(probe.x, probe.y)?.0)? { return Ok(false); }
            interior = true;
        }
    }
    Ok(interior)
}

pub(super) fn patch_components(
    patches: Vec<BrepPatch>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<Vec<BrepPatch>>> {
    patch_components_with_edge_distance(patches, tolerance, tolerance.linear * 32.0)
}

pub(super) fn patch_components_with_edge_distance(
    patches: Vec<BrepPatch>,
    tolerance: ModelTolerance,
    linear: f64,
) -> KernelResult<Vec<Vec<BrepPatch>>> {
    if patches.is_empty() {
        return Ok(Vec::new());
    }
    let samples = patches
        .iter()
        .map(patch_edge_samples)
        .collect::<KernelResult<Vec<_>>>()?;
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
    let mut edge_uses = Vec::<Vec<(usize, usize)>>::new();
    for (face, edges) in samples.iter().enumerate() {
        for (index, sample) in edges.iter().enumerate() {
            let matching = edge_uses.iter().position(|uses| {
                let (other_face, other_index) = uses[0];
                same_patch_edge(
                    (segments[face][index], *sample),
                    (segments[other_face][other_index], samples[other_face][other_index]),
                    linear,
                    tolerance,
                )
            });
            if let Some(group) = matching {
                edge_uses[group].push((face, index));
            } else {
                edge_uses.push(vec![(face, index)]);
            }
        }
    }
    // An edge with four uses is a contact between shells, not an adjacency
    // within a manifold shell. Reach each shell through its two-use edges;
    // the Euler builder then gives coincident contact edges separate topology.
    let mut neighbors = vec![BTreeSet::new(); patches.len()];
    for uses in &edge_uses {
        if let [(first, _), (second, _)] = uses.as_slice() {
            neighbors[*first].insert(*second);
            neighbors[*second].insert(*first);
        }
    }
    let mut remaining = (0..patches.len()).collect::<BTreeSet<_>>();
    let mut components = Vec::new();
    while let Some(seed) = remaining.iter().next().copied() {
        remaining.remove(&seed);
        let mut stack = vec![seed];
        let mut component = vec![seed];
        while let Some(current) = stack.pop() {
            let connected = neighbors[current]
                .intersection(&remaining)
                .copied()
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
    use super::super::Plane3;
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

}
