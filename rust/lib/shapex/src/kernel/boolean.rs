use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2};

use super::brep_classification::{BrepClassifier, PointClassification};
use super::trim::{align_pcurve_to_face_chart, coincident_supports, curve_intersections,
    face_boundary_curves, face_parameter_bounds, intersection_chart_copies,
    point_in_trim, sample_pcurve, translate_pcurve, trim_to_faces};
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::polygon::point_in_polygon;
use super::{
    box_center, nearest_uv, whole_periods, Compound, Curve2, Curve3, DiagnosticKind, FaceHandle,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, LoopKind, ModelTolerance,
    ParameterRange, Surface3,
};

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

/// How faces of both operands lying on one support are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoincidentFaces {
    /// Merge them into one set of material regions.
    Merge,
    /// Keep every first-operand face boundary. Sectioning by a half-space
    /// needs the boundary between an existing face and a coplanar cut cap,
    /// even when both have the same support and orientation.
    PreserveFirst,
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
    boolean_brep_with(first, second, kind, CoincidentFaces::Merge)
}

pub(crate) fn boolean_brep_with(
    first: &Compound,
    second: &Compound,
    kind: BooleanKind,
    coincident: CoincidentFaces,
) -> KernelResult<Compound> {
    let tolerance = first.tolerance();
    let mut arrangement = Arrangement::new([first, second], kind, coincident)?;
    arrangement.imprint_intersections()?;
    arrangement.overlay_coincident_boundaries()?;
    let mut patches = arrangement.face_patches()?;
    if coincident == CoincidentFaces::PreserveFirst {
        for patch in &mut patches {
            let Surface3::Plane(plane) = &patch.surface else {
                continue;
            };
            for segment in patch.loops.iter_mut().flat_map(|loop_| &mut loop_.segments) {
                if let Curve3::Line(_) = &segment.curve {
                    segment.pcurve = plane
                        .flatten_curve(&segment.curve, tolerance)?
                        .expect("a line has an exact plane image");
                }
            }
        }
    }
    super::sew::conform_brep_interfaces(&mut patches, tolerance)?;
    // Section regions partition each shared support exactly once. Boundary-only
    // containment would mistake a cap for a duplicate of its surrounding face.
    if coincident == CoincidentFaces::Merge {
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
    transaction.nest_brep_shells(&bodies)?;
    transaction.unify_same_domain()?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [first, second])?
        .with_boolean_parents(first, second, super::DerivationRole::Boolean(kind))
}

/// A face of either operand, addressed as `(operand, index)`.
type FaceKey = (usize, usize);

/// How a face being arranged shares its support with the other operand.
#[derive(Debug, Clone, Copy)]
enum Support {
    /// No face of the other operand lies on this support.
    Own,
    /// Faces of both operands lie on this support. With an `owner`, each of
    /// that operand's trims is one chart cell; with `exclude_first`, regions
    /// covered by first-operand faces are left to those faces.
    Shared {
        owner: Option<usize>,
        exclude_first: bool,
    },
}

/// The faces of both operands with their trims, as intersections are imprinted
/// and coincident boundaries overlaid before regions are classified.
struct Arrangement<'a> {
    operands: [&'a Compound; 2],
    faces: [Vec<FaceHandle>; 2],
    kind: BooleanKind,
    coincident: CoincidentFaces,
    tolerance: ModelTolerance,
    /// Probe distance used to classify the material side of a region.
    classification_offset: f64,
    /// Each face's own trim, recharted against the other operand.
    original: BTreeMap<FaceKey, Vec<FaceCurve>>,
    /// The trim plus every curve imprinted on the face.
    imprinted: BTreeMap<FaceKey, Vec<FaceCurve>>,
    supports: RefCell<BTreeMap<(FaceKey, FaceKey), bool>>,
}

impl<'a> Arrangement<'a> {
    fn new(
        operands: [&'a Compound; 2],
        kind: BooleanKind,
        coincident: CoincidentFaces,
    ) -> KernelResult<Self> {
        let tolerance = operands[0].tolerance();
        let scale = operands
            .iter()
            .flat_map(|operand| operand.vertices())
            .map(|vertex| vertex.point().to_vec().magnitude())
            .fold(1.0_f64, f64::max);
        let faces = operands.map(|operand| operand.faces().cloned().collect::<Vec<_>>());
        let mut curves = BTreeMap::new();
        for operand in 0..2 {
            for (index, face) in faces[operand].iter().enumerate() {
                curves.insert(
                    (operand, index),
                    boundary_curves(operands[operand], face.topology_id())?,
                );
            }
        }
        let source = curves.clone();
        for operand in 0..2 {
            for (index, face) in faces[operand].iter().enumerate() {
                rechart_periodic_band(
                    face.surface(),
                    curves.get_mut(&(operand, index)).expect("every face has a trim"),
                    &faces[1 - operand],
                    &source,
                    1 - operand,
                    tolerance,
                )?;
            }
        }
        Ok(Self {
            operands,
            faces,
            kind,
            coincident,
            tolerance,
            classification_offset: (tolerance.linear_at_scale(scale) * 32.0).max(scale * 5.0e-4)
                * 8.0,
            original: curves.clone(),
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

    /// Whether two faces lie on one support, computed once per ordered pair.
    fn same_support(&self, first: FaceKey, second: FaceKey) -> KernelResult<bool> {
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
        let imprinted = &mut self.imprinted;
        for (first_index, first_face) in faces[0].iter().enumerate() {
            for (second_index, second_face) in faces[1].iter().enumerate() {
                let first_boundary = &original[&(0, first_index)];
                let second_boundary = &original[&(1, second_index)];
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
                            let trimmed = trim_to_faces(
                                &intersection.curve,
                                first_pcurve,
                                first_boundary,
                                second_pcurve,
                                second_boundary,
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
                    let mut chart = face_parameter_bounds(target_boundary, tolerance)?;
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
        let preserve_first = self.coincident == CoincidentFaces::PreserveFirst;
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
                            let bounds = face_parameter_bounds(candidate_boundary, tolerance)?;
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
                    let canonical = usize::from(!preserve_first && !full_band(0) && full_band(1));
                    if singular && !preserve_first {
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
                let exclude_first = preserve_first && has_coincident_support;
                let mut arranged = Vec::new();
                if exclude_first {
                    // A coincident half-space retains each existing face
                    // boundary. Arrange the remaining cut cap separately.
                    for first_key in self.keys(0) {
                        if !self.same_support(key, first_key)? {
                            continue;
                        }
                        let boundary = &self.original[&first_key];
                        for patch in self.arrange_face(
                            classifiers,
                            first_key,
                            boundary,
                            boundary,
                            Support::Shared {
                                owner: None,
                                exclude_first: false,
                            },
                        )? {
                            arranged.push((first_key, patch));
                        }
                    }
                }
                let support = if has_coincident_support {
                    Support::Shared {
                        owner: shared_owner,
                        exclude_first,
                    }
                } else {
                    Support::Own
                };
                let curves = if has_coincident_support && shared_owner.is_none() {
                    &combined_curves
                } else {
                    &self.imprinted[&key]
                };
                for patch in self.arrange_face(classifiers, key, &original, curves, support)? {
                    arranged.push((key, patch));
                }
                for (patch_key, mut patch) in arranged {
                    if has_coincident_support {
                        // A shared-support arrangement can contain several
                        // disjoint source faces. Attribute only the trimmed
                        // regions it uses.
                        for parent_operand in 0..2 {
                            let parent = self.operands[parent_operand];
                            for candidate_key in self.keys(parent_operand) {
                                let candidate = self.face(candidate_key);
                                if self.same_support(patch_key, candidate_key)?
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
    opposite: &[super::FaceHandle],
    sources: &BTreeMap<(usize, usize), Vec<FaceCurve>>,
    operand: usize,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    if !matches!(surface, Surface3::Cylinder(_)) || boundary.len() != 4
        || boundary.iter().filter(|curve| curve.parameter_seam).count() != 2
        || boundary.iter().any(|curve| !matches!(curve.pcurve, Curve2::Line(_)))
        || boundary.iter().any(|curve| !curve.parameter_seam && !matches!(curve.curve, Curve3::Circle(_)))
    {
        return Ok(());
    }
    let bounds = face_parameter_bounds(boundary, tolerance)?;
    let period = std::f64::consts::TAU;
    if (bounds[0].span() - period).abs() > tolerance.parameter * 64.0 {
        return Ok(());
    }
    let mut occupied = Vec::new();
    for (index, face) in opposite.iter().enumerate() {
        let other = &sources[&(operand, index)];
        if face_bounds_separated(surface, boundary, face.surface(), other, tolerance)? { continue; }
        let intersections = surface.intersect_surface_in_bounds(face.surface(), bounds,
            face_parameter_bounds(other, tolerance)?, tolerance)?;
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
            if maximum - minimum >= period - tolerance.parameter * 64.0 { return Ok(()); }
            let start = (minimum - bounds[0].start).rem_euclid(period);
            let end = start + maximum - minimum;
            occupied.push((start, end.min(period)));
            if end > period { occupied.push((0.0, end - period)); }
        }
    }
    if occupied.is_empty() { return Ok(()); }
    occupied.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = occupied[0].1;
    let mut gap = (0.0, 0.0);
    for interval in &occupied[1..] {
        if interval.0 - end > gap.1 - gap.0 { gap = (end, interval.0); }
        end = end.max(interval.1);
    }
    if occupied[0].0 + period - end > gap.1 - gap.0 { gap = (end, occupied[0].0 + period); }
    if gap.1 - gap.0 <= tolerance.parameter * 64.0 { return Ok(()); }
    // Preserve a seam that already lies in the free interval.
    if occupied[0].0 > tolerance.parameter * 64.0 && end < period - tolerance.parameter * 64.0 {
        return Ok(());
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
    Ok(())
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
            let reference = parameters
                .last()
                .copied()
                .unwrap_or(candidate.range.middle());
            let projected = candidate.curve.domain().nearest(projection.parameter, reference);
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
    let (has_coincident_support, shared_owner, exclude_existing_faces) = match support {
        Support::Own => (false, None, false),
        Support::Shared {
            owner,
            exclude_first,
        } => (true, owner, exclude_first),
    };
    let kind = self.kind;
    let classification_offset = self.classification_offset;
    let tolerance = self.tolerance;
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
        if let Some(owner) = shared_owner {
            // Arrange each source trim as one chart cell. This retains pole
            // meridians and periodic cuts instead of collapsing an entire
            // closed support into a singular one-face boundary. The other
            // operand contributes only regions outside the owner's trims.
            if !point_in_trim(uv, original_boundary)?
                || (operand != owner && classifiers[owner].covers_on_support(surface, value.point)?)
            {
                return Ok(None);
            }
        }
        if exclude_existing_faces && classifiers[0].covers_on_support(surface, value.point)? {
            return Ok(None);
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
        let other = classifiers[1 - operand].classify(value.point)?;
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
        // Classify one complete periodic chart. The material is periodic,
        // but its planar arrangement needs an exterior at the chart seams.
        if has_coincident_support && surface.domains().iter().enumerate().any(|(axis, domain)| {
            domain.period.is_some_and(|period| {
                uv[axis] < uv_bounds[0][axis] || uv[axis] > uv_bounds[0][axis] + period
            })
        }) {
            return Ok(None);
        }
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
        if matches!(piece.curve, Curve3::SurfaceIntersection(_))
            && !(matches!(surface, Surface3::RollingBall(_)) && surface.domains()[0].period.is_none()
                && matches!(piece.pcurve, Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_)))
        {
            // A tracing spline selects the intersection branch but is not the
            // intersection itself. Center side probes on the corrected curve.
            // Exact p-curves on open rolling joins already select that branch;
            // a global blend projection can land on a different local minimum.
            let point = piece.curve.evaluate(parameter)?.point;
            let projection = surface.project(point, tolerance)?;
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
        for curve in curves {
            // A curve cannot limit the clearance of probes beside itself.
            // Numerical projection onto an implicit trim can otherwise turn
            // its solver residual into an artificial micrometer-sized gap.
            if curve.curve == piece.curve { continue; }
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
        let orientation_sign = orientation.sign();
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
        let mut classified = Vec::<(Vec<DirectedPiece>, Vec<Point2>, f64, bool, Point2)>::new();
        for (mut loop_, area) in loops {
            if area.abs() <= tolerance.parameter.powi(2) {
                continue;
            }
            let polygon = loop_polyline(&loop_)?;
            // Sample inside the loop: a concave loop's centroid can lie
            // outside it, while a boundary sample can lie on a shared chart
            // seam and mistake an adjacent region for an enclosing one.
            let sample = polygon_interior_point(&polygon)?;
            let depth = classified
                .iter()
                .filter(|(_, enclosing, enclosing_area, _, _)| {
                    enclosing_area.abs() > area.abs() && point_in_polygon(sample, enclosing)
                })
                .count();
            let outer = depth % 2 == 0;
            let desired_sign = orientation_sign * if outer { 1.0 } else { -1.0 };
            if area.signum() != desired_sign.signum() {
                loop_.reverse();
                loop_ = loop_.into_iter().map(|piece| piece.reversed()).collect();
            }
            classified.push((loop_, polygon, area, outer, sample));
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
                segments: patch_segments(outer),
            }];
            for (hole, _, hole_area, is_outer, sample) in &classified {
                if *is_outer {
                    continue;
                }
                if classified[outer_index].2.abs() > hole_area.abs()
                    && point_in_polygon(*sample, outer_polygon)
                {
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
                    pieces[*index].start_node == end_node
                        || (pieces[*index].parameter_seam && pieces[*index].end_node == end_node)
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
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "Boolean face loop has no interior",
            )
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
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "Boolean face loop has no interior",
            )
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
    let inside = |point: Point2, curves: &[FaceCurve], bounds: [ParameterRange; 2]| {
        point_in_trim(nearest_uv(domains, point, box_center(bounds)), curves)
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
    let mut edge_uses = Vec::<Vec<(usize, usize)>>::new();
    for (face, edges) in samples.iter().enumerate() {
        for (index, sample) in edges.iter().enumerate() {
            let matching = edge_uses.iter().position(|uses| {
                let (other_face, other_index) = uses[0];
                let other = samples[other_face][other_index];
                same_unoriented_edge(*sample, other, linear)
                    || (((sample[0].distance(other[0]) <= linear
                        && sample[2].distance(other[2]) <= linear)
                        || (sample[0].distance(other[2]) <= linear
                            && sample[2].distance(other[0]) <= linear))
                        && segments[face][index]
                            .curve
                            .project_on_trim(other[1], segments[face][index].range, tolerance)
                            .is_ok_and(|p| p.distance <= linear)
                        && segments[other_face][other_index]
                            .curve
                            .project_on_trim(
                                sample[1],
                                segments[other_face][other_index].range,
                                tolerance,
                            )
                            .is_ok_and(|p| p.distance <= linear))
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
