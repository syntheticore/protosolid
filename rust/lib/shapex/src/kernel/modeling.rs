use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::euler::CommittedOperation;
use super::integration::adaptive_simpson;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, BooleanKind, ChangeKind, Circle2, Curve2, Curve3, DiagnosticKind, EdgeId, FaceId,
    Kernel, KernelError, KernelResult, LoopKind, MassProperties, ModelTolerance, NurbsCurve2,
    OperationHistory, Origin, ParameterRange, PersistentId, Plane3, Profile, RigidTransform3,
    Surface3, Tessellate, TessellationOptions, TriangleMesh, TrimmedCurve2, Wire2,
};

#[derive(Clone, Copy)]
struct ConservativeBounds3 {
    minimum: Point3,
    maximum: Point3,
}

impl ConservativeBounds3 {
    fn empty() -> Self {
        Self {
            minimum: Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            maximum: Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    fn include(&mut self, point: Point3) {
        self.minimum.x = self.minimum.x.min(point.x);
        self.minimum.y = self.minimum.y.min(point.y);
        self.minimum.z = self.minimum.z.min(point.z);
        self.maximum.x = self.maximum.x.max(point.x);
        self.maximum.y = self.maximum.y.max(point.y);
        self.maximum.z = self.maximum.z.max(point.z);
    }

    fn separated(self, other: Self, tolerance: f64) -> bool {
        self.maximum.x < other.minimum.x - tolerance
            || other.maximum.x < self.minimum.x - tolerance
            || self.maximum.y < other.minimum.y - tolerance
            || other.maximum.y < self.minimum.y - tolerance
            || self.maximum.z < other.minimum.z - tolerance
            || other.maximum.z < self.minimum.z - tolerance
    }
}

#[derive(Debug, Clone, Default)]
struct LineageEntry {
    descendants: BTreeSet<PersistentId>,
    changed: bool,
    merged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntityKind {
    Vertex,
    Edge,
    Face,
}

/// Compact, transitive ancestry for every entity that has contributed to the
/// current result. It stores names and relationships only, never old geometry.
#[derive(Debug, Clone, Default)]
struct Lineage {
    entries: BTreeMap<(EntityKind, PersistentId), LineageEntry>,
}

impl Lineage {
    fn root(current: &BTreeMap<PersistentId, EntityKind>) -> Self {
        Self {
            entries: current
                .iter()
                .map(|(name, kind)| {
                    (
                        (*kind, *name),
                        LineageEntry {
                            descendants: BTreeSet::from([*name]),
                            changed: false,
                            merged: false,
                        },
                    )
                })
                .collect(),
        }
    }

    fn compose(
        parents: impl IntoIterator<Item = Arc<Lineage>>,
        history: &OperationHistory,
        current: &BTreeMap<PersistentId, EntityKind>,
    ) -> Self {
        let parents = parents.into_iter().collect::<Vec<_>>();
        if parents.is_empty() {
            return Self::root(current);
        }

        let generated = generated_index(history);
        let mut entries = BTreeMap::<(EntityKind, PersistentId), LineageEntry>::new();
        for parent in parents {
            for ((kind, ancestor), previous) in &parent.entries {
                let entry = entries.entry((*kind, *ancestor)).or_default();
                entry.changed |= previous.changed;
                entry.merged |= previous.merged;
                for source in &previous.descendants {
                    let direct = direct_descendants(*kind, *source, history, current, &generated);
                    entry.changed |= direct.changed;
                    entry.merged |= direct.merged;
                    entry.descendants.extend(direct.descendants);
                }
            }
        }

        for (current_name, kind) in current {
            entries
                .entry((*kind, *current_name))
                .or_insert_with(|| LineageEntry {
                    descendants: BTreeSet::from([*current_name]),
                    changed: false,
                    merged: false,
                });
        }
        Self { entries }
    }
}

fn generated_index(history: &OperationHistory) -> BTreeMap<PersistentId, BTreeSet<PersistentId>> {
    let mut generated = BTreeMap::<PersistentId, BTreeSet<PersistentId>>::new();
    for (output, origin) in history.origins() {
        if let Origin::Derived { sources, .. } = origin {
            for source in sources {
                generated.entry(*source).or_default().insert(*output);
            }
        }
    }
    generated
}

fn semantic_roots(history: &OperationHistory, entity: PersistentId) -> BTreeSet<PersistentId> {
    let mut pending = vec![entity];
    let mut visited = BTreeSet::new();
    let mut roots = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        match history.origin(current) {
            Some(Origin::Derived { sources, .. }) if !sources.is_empty() => {
                pending.extend(sources.iter().copied());
            }
            _ => {
                roots.insert(current);
            }
        }
    }
    // Imported entities can participate in cyclic provenance when a no-op
    // Boolean preserves their persistent IDs. The visited set is the finite
    // semantic ancestry in that case; recursive expansion would be exponential.
    if roots.is_empty() {
        visited
    } else {
        roots
    }
}

fn semantic_edge(history: &OperationHistory, edge: &EdgeHandle) -> bool {
    let Ok(faces) = edge.connected_faces() else {
        return true;
    };
    if faces.len() != 2 {
        return true;
    }
    let first_roots = semantic_roots(history, faces[0].persistent_id());
    let second_roots = semantic_roots(history, faces[1].persistent_id());
    if first_roots.is_disjoint(&second_roots) {
        return true;
    }
    // An exact Boolean deliberately gives all of its output faces the same
    // operation ancestry. Preserve creases between unlike analytical supports,
    // including perpendicular planes, without exposing the many edges of a
    // faceted feature reconstruction.
    if std::mem::discriminant(faces[0].surface()) != std::mem::discriminant(faces[1].surface()) {
        return true;
    }
    let exact_boolean_face = |face: &FaceHandle| {
        matches!(
            history.origin(face.persistent_id()),
            Some(Origin::Derived { role, .. })
                if matches!(
                    role.as_str(),
                    "boolean.union" | "boolean.difference" | "boolean.intersection"
                )
        )
    };
    match (faces[0].surface(), faces[1].surface()) {
        (Surface3::Plane(first), Surface3::Plane(second))
            if exact_boolean_face(&faces[0]) && exact_boolean_face(&faces[1]) =>
        {
            first.normal().cross(second.normal()).magnitude() > edge.state.tolerance().angular
        }
        // Faceted feature reconstructions deliberately share planar ancestry,
        // so their internal triangle edges remain hidden. Curved analytical
        // patches, however, can have the same surface representation while
        // lying on different supports (for example two adjacent NURBS patches
        // in a rolling blend). Their shared boundary is a semantic BREP edge.
        (Surface3::Plane(_), Surface3::Plane(_)) => false,
        (first, second) => !same_surface_support(first, second, edge.state.tolerance()),
    }
}

fn same_curve_support(
    first: &super::Curve3,
    second: &super::Curve3,
    tolerance: ModelTolerance,
) -> bool {
    match (first, second) {
        (super::Curve3::Line(first), super::Curve3::Line(second)) => {
            let first_direction = first.direction().normalize();
            let second_direction = second.direction().normalize();
            first_direction.cross(second_direction).magnitude() <= tolerance.angular
                && (second.origin() - first.origin())
                    .cross(first_direction)
                    .magnitude()
                    <= tolerance.linear
        }
        (super::Curve3::Circle(first), super::Curve3::Circle(second)) => {
            first.center().distance(second.center()) <= tolerance.linear
                && (first.radius() - second.radius()).abs() <= tolerance.linear
                && first
                    .normal()
                    .dot(second.normal())
                    .abs()
                    .clamp(-1.0, 1.0)
                    .acos()
                    <= tolerance.angular
        }
        (super::Curve3::Nurbs(first), super::Curve3::Nurbs(second)) => {
            let compatible = first.degree() == second.degree()
                && first.control_points().len() == second.control_points().len()
                && first.weights().len() == second.weights().len()
                && first.knots().len() == second.knots().len();
            if !compatible {
                return false;
            }
            let normalized_knots = |knots: &[f64]| {
                let start = knots[0];
                let span = knots[knots.len() - 1] - start;
                knots
                    .iter()
                    .map(|value| (value - start) / span)
                    .collect::<Vec<_>>()
            };
            let first_knots = normalized_knots(first.knots());
            let second_knots = normalized_knots(second.knots());
            let same = |reversed: bool| {
                let point = |index| {
                    if reversed {
                        second.control_points()[second.control_points().len() - 1 - index]
                    } else {
                        second.control_points()[index]
                    }
                };
                let weight = |index| {
                    if reversed {
                        second.weights()[second.weights().len() - 1 - index]
                    } else {
                        second.weights()[index]
                    }
                };
                let knot = |index| {
                    if reversed {
                        1.0 - second_knots[second_knots.len() - 1 - index]
                    } else {
                        second_knots[index]
                    }
                };
                let weight_scale = weight(0) / first.weights()[0];
                first
                    .control_points()
                    .iter()
                    .enumerate()
                    .all(|(index, value)| value.distance(point(index)) <= tolerance.linear)
                    && first.weights().iter().enumerate().all(|(index, value)| {
                        (value * weight_scale - weight(index)).abs() <= tolerance.parameter
                    })
                    && first_knots
                        .iter()
                        .enumerate()
                        .all(|(index, value)| (value - knot(index)).abs() <= tolerance.parameter)
            };
            same(false) || same(true)
        }
        _ => first == second,
    }
}

fn same_surface_support(
    first: &super::Surface3,
    second: &super::Surface3,
    tolerance: ModelTolerance,
) -> bool {
    match (first, second) {
        (super::Surface3::Plane(first), super::Surface3::Plane(second)) => {
            let first_normal = first.normal();
            let second_normal = second.normal();
            first_normal.cross(second_normal).magnitude() <= tolerance.angular
                && (second.origin() - first.origin()).dot(first_normal).abs() <= tolerance.linear
        }
        (super::Surface3::Cylinder(first), super::Surface3::Cylinder(second)) => {
            let first_axis = first.axis().normalize();
            let second_axis = second.axis().normalize();
            first_axis.cross(second_axis).magnitude() <= tolerance.angular
                && (second.origin() - first.origin())
                    .cross(first_axis)
                    .magnitude()
                    <= tolerance.linear
                && (first.radius() - second.radius()).abs() <= tolerance.linear
        }
        _ => first == second,
    }
}

fn same_surface_support_set(
    first: &[Surface3],
    second: &[Surface3],
    tolerance: ModelTolerance,
) -> bool {
    if first.len() != second.len() {
        return false;
    }
    let mut used = vec![false; second.len()];
    first.iter().all(|surface| {
        second
            .iter()
            .enumerate()
            .find(|(index, candidate)| {
                !used[*index] && same_surface_support(surface, candidate, tolerance)
            })
            .map(|(index, _)| used[index] = true)
            .is_some()
    })
}

fn add_origins(
    origins: &mut BTreeMap<PersistentId, Vec<PersistentId>>,
    name: PersistentId,
    sources: impl IntoIterator<Item = PersistentId>,
) {
    let entry = origins.entry(name).or_default();
    for source in sources {
        if !entry.contains(&source) {
            entry.push(source);
        }
    }
}

#[derive(Debug, Clone)]
struct RoundedProfileCorner {
    source: PersistentId,
    center: Point2,
    incoming_tangent: Point2,
    outgoing_tangent: Point2,
}

fn round_profile_outer_corners(
    profile: &Profile,
    selected: &[(PersistentId, Point2)],
    radius: f64,
) -> KernelResult<(Profile, Vec<RoundedProfileCorner>)> {
    let tolerance = profile.tolerance();
    let segments = profile.outer().segments();
    if segments.len() < 3 {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "analytical prism fillets need a profile with at least three boundary segments",
        ));
    }
    let vertices = segments
        .iter()
        .map(TrimmedCurve2::start)
        .collect::<KernelResult<Vec<_>>>()?;
    let mut corners = vec![None; segments.len()];
    for (source, point) in selected {
        let matches = vertices
            .iter()
            .enumerate()
            .filter_map(|(index, vertex)| {
                (vertex.distance(*point) <= tolerance.linear_at_scale(vertex.to_vec().magnitude()))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 || corners[matches[0]].is_some() {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "selected fillet edges must map uniquely to outer profile corners",
            ));
        }
        let index = matches[0];
        let previous = (index + segments.len() - 1) % segments.len();
        if !matches!(segments[previous].curve(), Curve2::Line(_))
            || !matches!(segments[index].curve(), Curve2::Line(_))
        {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "analytical prism fillets currently require two straight profile sides",
            ));
        }
        let vertex = vertices[index];
        let previous_start = segments[previous].start()?;
        let next_end = segments[index].end()?;
        let incoming = (vertex - previous_start).normalize();
        let outgoing = (next_end - vertex).normalize();
        let turn =
            (incoming.x * outgoing.y - incoming.y * outgoing.x).atan2(incoming.dot(outgoing));
        if turn <= tolerance.angular || std::f64::consts::PI - turn <= tolerance.angular {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "analytical prism fillets currently require a convex outer profile corner",
            ));
        }
        let tangent_distance = radius * (turn * 0.5).tan();
        if !tangent_distance.is_finite() || tangent_distance <= tolerance.linear {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "fillet radius is too small or large for the selected corner",
            ));
        }
        let incoming_tangent = vertex - incoming * tangent_distance;
        let outgoing_tangent = vertex + outgoing * tangent_distance;
        let inward = Vec2::new(-incoming.y, incoming.x);
        corners[index] = Some(RoundedProfileCorner {
            source: *source,
            center: incoming_tangent + inward * radius,
            incoming_tangent,
            outgoing_tangent,
        });
    }

    let mut rounded_segments = Vec::with_capacity(segments.len() + selected.len());
    for index in 0..segments.len() {
        let next = (index + 1) % segments.len();
        let start = corners[index]
            .as_ref()
            .map(|corner| corner.outgoing_tangent)
            .unwrap_or(segments[index].start()?);
        let end = corners[next]
            .as_ref()
            .map(|corner| corner.incoming_tangent)
            .unwrap_or(segments[index].end()?);
        if start.distance(end) <= tolerance.linear_at_scale(start.to_vec().magnitude()) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "fillet radius consumes an adjacent profile edge",
            ));
        }
        if corners[index].is_some() || corners[next].is_some() {
            rounded_segments.push(TrimmedCurve2::line(start, end)?);
        } else {
            rounded_segments.push(segments[index].clone());
        }
        if let Some(corner) = &corners[next] {
            let first_radial = (corner.incoming_tangent - corner.center).normalize();
            let second_radial = (corner.outgoing_tangent - corner.center).normalize();
            let middle_direction = (first_radial + second_radial).normalize();
            rounded_segments.push(TrimmedCurve2::arc_through(
                corner.incoming_tangent,
                corner.center + middle_direction * radius,
                corner.outgoing_tangent,
            )?);
        }
    }
    let rounded = Wire2::new(rounded_segments, tolerance)?;
    let rounded_profile = Profile::new(rounded, profile.holes().iter().cloned(), tolerance)?;
    Ok((
        rounded_profile,
        corners.into_iter().flatten().collect::<Vec<_>>(),
    ))
}

fn end_fillet_mass_properties(
    source: MassProperties,
    plane: Plane3,
    profile: &Profile,
    chosen: &[bool],
    axis: Vec3,
    height: f64,
    radius: f64,
) -> KernelResult<MassProperties> {
    let segments = profile.outer().segments();
    if chosen.len() != segments.len() || chosen.iter().all(|value| !*value) {
        return Err(KernelError::new(
            DiagnosticKind::InvalidInput,
            "analytical end-fillet certificate has no valid rail chain",
        ));
    }
    let tolerance = profile.tolerance();
    let coordinate_scale = segments
        .iter()
        .flat_map(|segment| [segment.start(), segment.end()])
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .map(|point| point.to_vec().magnitude())
        .fold(
            plane
                .origin()
                .to_vec()
                .magnitude()
                .max(height)
                .max(radius)
                .max(1.0),
            f64::max,
        );
    let volume_target = tolerance
        .linear
        .powi(3)
        .max(f64::EPSILON * source.volume.abs().max(1.0) * 4096.0);
    let area_target = tolerance
        .linear
        .powi(2)
        .max(f64::EPSILON * source.surface_area.abs().max(1.0) * 4096.0);
    let targets = [
        volume_target,
        volume_target * coordinate_scale,
        volume_target * coordinate_scale,
        volume_target * coordinate_scale,
        volume_target * coordinate_scale.powi(2),
        volume_target * coordinate_scale.powi(2),
        volume_target * coordinate_scale.powi(2),
        volume_target * coordinate_scale.powi(2),
        volume_target * coordinate_scale.powi(2),
        volume_target * coordinate_scale.powi(2),
        area_target,
        area_target,
        area_target,
    ];
    let selected_count = chosen.iter().filter(|value| **value).count() as f64;
    let mut removed_volume = 0.0;
    let mut removed_first = Vec3::new(0.0, 0.0, 0.0);
    let mut removed_second = [[0.0; 3]; 3];
    let mut top_strip = 0.0;
    let mut side_strip = 0.0;
    let mut blend_area = 0.0;
    for (index, segment) in segments.iter().enumerate() {
        if !chosen[index] {
            continue;
        }
        let range = segment.range();
        let per_segment_targets = targets.map(|target| target / selected_count);
        let values = adaptive_simpson(
            0.0,
            1.0,
            |unit_parameter| {
                let parameter = range.start + range.span() * unit_parameter;
                let evaluation = segment.curve().evaluate(parameter)?;
                let traversal = range.span().signum();
                let derivative = evaluation.first_derivative;
                let speed = derivative.magnitude();
                if speed <= tolerance.parameter {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "end-fillet rail has a singular tangent",
                    ));
                }
                let tangent = derivative / speed * traversal;
                let inward = Vec2::new(-tangent.y, tangent.x);
                let normalized_derivative = evaluation.second_derivative / speed
                    - derivative * derivative.dot(evaluation.second_derivative) / speed.powi(3);
                let inward_derivative = Vec2::new(
                    -normalized_derivative.y * traversal,
                    normalized_derivative.x * traversal,
                );
                let inner_targets = per_segment_targets.map(|target| target * 0.01);
                let mut integrated = adaptive_simpson(
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                    |angle| {
                        let (sin, cos) = angle.sin_cos();
                        let s = radius * (1.0 - cos);
                        let ds = radius * sin;
                        let offset_derivative = derivative + inward_derivative * s;
                        if offset_derivative.dot(derivative) <= tolerance.parameter * speed.powi(2)
                        {
                            return Err(KernelError::new(
                                DiagnosticKind::InvalidInput,
                                "end fillet radius folds a circular offset rail",
                            ));
                        }
                        let offset_speed = offset_derivative.magnitude();
                        let lower_q = radius * sin;
                        let q0 = radius - lower_q;
                        let q1 = (radius.powi(2) - lower_q.powi(2)) * 0.5;
                        let q2 = (radius.powi(3) - lower_q.powi(3)) / 3.0;
                        let uv = evaluation.point + inward * s;
                        let base = plane.evaluate(uv.x, uv.y)?.point + axis * (height - radius);
                        let base_coordinates = [base.x, base.y, base.z];
                        let axis_coordinates = [axis.x, axis.y, axis.z];
                        let measure = offset_speed * ds;
                        let first = base.to_vec() * q0 + axis * q1;
                        let mut terms = [0.0; 13];
                        terms[0] = q0 * measure;
                        terms[1] = first.x * measure;
                        terms[2] = first.y * measure;
                        terms[3] = first.z * measure;
                        let pairs = [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)];
                        for (slot, (row, column)) in pairs.into_iter().enumerate() {
                            terms[4 + slot] =
                                (base_coordinates[row] * base_coordinates[column] * q0
                                    + (base_coordinates[row] * axis_coordinates[column]
                                        + axis_coordinates[row] * base_coordinates[column])
                                        * q1
                                    + axis_coordinates[row] * axis_coordinates[column] * q2)
                                    * measure;
                        }
                        terms[10] = offset_speed * ds;
                        terms[11] = offset_speed * radius;
                        Ok(terms)
                    },
                    inner_targets,
                    24,
                    "analytical end-fillet cross-section integration did not converge",
                )?;
                for value in &mut integrated {
                    *value *= range.span().abs();
                }
                integrated[12] = speed * radius * range.span().abs();
                Ok(integrated)
            },
            per_segment_targets,
            24,
            "analytical end-fillet rail integration did not converge",
        )?;
        removed_volume += values[0];
        removed_first += Vec3::new(values[1], values[2], values[3]);
        removed_second[0][0] += values[4];
        removed_second[1][1] += values[5];
        removed_second[2][2] += values[6];
        removed_second[0][1] += values[7];
        removed_second[1][0] += values[7];
        removed_second[0][2] += values[8];
        removed_second[2][0] += values[8];
        removed_second[1][2] += values[9];
        removed_second[2][1] += values[9];
        top_strip += values[10];
        blend_area += values[11];
        side_strip += values[12];
    }
    let mass = source.volume - removed_volume;
    if !mass.is_finite() || mass <= tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::InvalidInput,
            "end fillet removes all or an uncertifiable amount of material",
        ));
    }
    let source_center = source.centroid.to_vec();
    let mut inertia_origin = source.inertia;
    let center_squared = source_center.magnitude2();
    for row in 0..3 {
        for column in 0..3 {
            inertia_origin[row][column] += source.volume
                * (if row == column { center_squared } else { 0.0 }
                    - source_center[row] * source_center[column]);
        }
    }
    let trace = inertia_origin[0][0] + inertia_origin[1][1] + inertia_origin[2][2];
    let mut source_second = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            source_second[row][column] =
                if row == column { trace * 0.5 } else { 0.0 } - inertia_origin[row][column];
        }
    }
    let first = source_center * source.volume - removed_first;
    let centroid = Point3::from_vec(first / mass);
    let mut second = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            second[row][column] = source_second[row][column] - removed_second[row][column];
        }
    }
    let second_trace = second[0][0] + second[1][1] + second[2][2];
    let center = centroid.to_vec();
    let mut inertia = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            let origin_value = if row == column { second_trace } else { 0.0 } - second[row][column];
            inertia[row][column] = origin_value
                - mass
                    * (if row == column {
                        center.magnitude2()
                    } else {
                        0.0
                    } - center[row] * center[column]);
        }
    }
    let chain_starts = (0..chosen.len())
        .filter(|index| chosen[*index] && !chosen[(*index + chosen.len() - 1) % chosen.len()])
        .count();
    let endpoint_loss =
        chain_starts as f64 * 2.0 * radius.powi(2) * (1.0 - std::f64::consts::FRAC_PI_4);
    let properties = MassProperties {
        volume: mass,
        signed_volume: mass,
        surface_area: source.surface_area - top_strip - side_strip - endpoint_loss + blend_area,
        centroid,
        inertia,
    };
    super::physical::validate_supplied_properties(properties, tolerance, coordinate_scale)?;
    Ok(properties)
}

fn direct_descendants(
    kind: EntityKind,
    source: PersistentId,
    history: &OperationHistory,
    current: &BTreeMap<PersistentId, EntityKind>,
    generated: &BTreeMap<PersistentId, BTreeSet<PersistentId>>,
) -> LineageEntry {
    if let Some((change_kind, outputs)) = history.change(source) {
        return LineageEntry {
            descendants: outputs
                .iter()
                .copied()
                .filter(|output| current.get(output) == Some(&kind))
                .collect(),
            changed: true,
            merged: change_kind == ChangeKind::Merged,
        };
    }
    if let Some(outputs) = generated.get(&source) {
        let descendants = outputs
            .iter()
            .copied()
            .filter(|output| current.get(output) == Some(&kind))
            .collect::<BTreeSet<_>>();
        if !descendants.is_empty() {
            let merged = descendants.iter().any(|output| {
                matches!(
                    history.origin(*output),
                    Some(Origin::Derived { sources, .. }) if sources.len() > 1
                )
            });
            return LineageEntry {
                descendants,
                changed: true,
                merged,
            };
        }
    }
    LineageEntry {
        descendants: (current.get(&source) == Some(&kind))
            .then_some(source)
            .into_iter()
            .collect(),
        changed: current.get(&source) != Some(&kind),
        merged: false,
    }
}

/// A cloneable face reference tied to an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct FaceHandle {
    state: Arc<Kernel>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    face: FaceId,
    name: PersistentId,
    revision: uuid::Uuid,
    surface: super::Surface3,
}

impl FaceHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn surface(&self) -> &super::Surface3 {
        &self.surface
    }
    pub fn boundary_edges(&self) -> KernelResult<Vec<EdgeHandle>> {
        self.state
            .topology()
            .face_edges(self.face)?
            .into_iter()
            .map(|edge| {
                build_edge_handle(Arc::clone(&self.state), edge, Arc::clone(&self.revisions))
            })
            .collect()
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        <Self as Tessellate>::tessellate(self, options)
    }
    pub(crate) fn normal_at(
        &self,
        point: Point3,
        tolerance: ModelTolerance,
    ) -> KernelResult<crate::Vec3> {
        let projection = self.surface.project(point, tolerance)?;
        let normal = self
            .surface
            .normal(projection.parameters.0, projection.parameters.1)?;
        Ok(match self.state.topology().face(self.face)?.orientation() {
            super::FaceOrientation::Forward => normal,
            super::FaceOrientation::Reversed => -normal,
        })
    }
}

impl PartialEq for FaceHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for FaceHandle {}

impl Tessellate for FaceHandle {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_face(self.face, options)
    }
}

/// A cloneable edge reference tied to an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct EdgeHandle {
    state: Arc<Kernel>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    edge: EdgeId,
    name: PersistentId,
    revision: uuid::Uuid,
    curve: super::Curve3,
}

impl EdgeHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn curve(&self) -> &super::Curve3 {
        &self.curve
    }
    pub fn endpoints(&self) -> KernelResult<[Point3; 2]> {
        let topology = self.state.topology();
        let vertices = topology.edge(self.edge)?.vertices();
        Ok([
            topology.vertex(vertices[0])?.point(),
            topology.vertex(vertices[1])?.point(),
        ])
    }
    pub fn connected_faces(&self) -> KernelResult<Vec<FaceHandle>> {
        self.state
            .topology()
            .edge_faces(self.edge)?
            .into_iter()
            .map(|face| {
                build_face_handle(Arc::clone(&self.state), face, Arc::clone(&self.revisions))
            })
            .collect()
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<super::Polyline> {
        <Self as Tessellate>::tessellate(self, options)
    }
}

impl PartialEq for EdgeHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for EdgeHandle {}

impl Tessellate for EdgeHandle {
    type Output = super::Polyline;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_edge(self.edge, options)
    }
}

/// A cloneable vertex reference tied to an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct VertexHandle {
    name: PersistentId,
    revision: uuid::Uuid,
    point: Point3,
}

impl VertexHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn point(&self) -> Point3 {
        self.point
    }
}

impl PartialEq for VertexHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for VertexHandle {}

/// Resolution of an ancestor entity in the current result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<T> {
    Preserved(T),
    Modified(T),
    Split(Vec<T>),
    Merged(T),
    Deleted,
    Unrelated,
}

mod sealed {
    pub trait Sealed {}
}

/// Entity types accepted by [`Compound::resolve`] and provenance queries.
#[doc(hidden)]
pub trait ResolvableEntity: sealed::Sealed + Clone {
    fn persistent_name(&self) -> PersistentId;
    fn revision(&self) -> uuid::Uuid;
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self>;
    fn resolve_in_solid(&self, solid: &Solid) -> Resolution<Self>;
}

macro_rules! impl_resolvable {
    ($type:ty, $iterator:ident, $kind:ident) => {
        impl sealed::Sealed for $type {}
        impl ResolvableEntity for $type {
            fn persistent_name(&self) -> PersistentId {
                self.name
            }
            fn revision(&self) -> uuid::Uuid {
                self.revision
            }
            fn resolve_in(&self, compound: &Compound) -> Resolution<Self> {
                resolve_entity_from(
                    &compound.lineage,
                    &compound.revisions,
                    EntityKind::$kind,
                    self.name,
                    self.revision,
                    |name| {
                        compound
                            .$iterator()
                            .find(|entity| entity.name == name)
                            .cloned()
                    },
                )
            }
            fn resolve_in_solid(&self, solid: &Solid) -> Resolution<Self> {
                resolve_entity_from(
                    &solid.lineage,
                    &solid.revisions,
                    EntityKind::$kind,
                    self.name,
                    self.revision,
                    |name| {
                        solid
                            .$iterator()
                            .find(|entity| entity.name == name)
                            .cloned()
                    },
                )
            }
        }
    };
}

impl_resolvable!(FaceHandle, faces, Face);
impl_resolvable!(EdgeHandle, edges, Edge);
impl_resolvable!(VertexHandle, vertices, Vertex);

/// One connected, physically certified material volume.
#[derive(Debug, Clone)]
pub struct Solid {
    state: Arc<Kernel>,
    body: BodyId,
    properties: MassProperties,
    faces: Vec<FaceHandle>,
    edges: Vec<EdgeHandle>,
    vertices: Vec<VertexHandle>,
    history: Arc<OperationHistory>,
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
}

impl Solid {
    pub fn volume(&self) -> f64 {
        self.properties.volume
    }
    pub fn surface_area(&self) -> f64 {
        self.properties.surface_area
    }
    pub fn centroid(&self) -> Point3 {
        self.properties.centroid
    }
    pub fn mass_properties(&self) -> MassProperties {
        self.properties
    }
    pub fn faces(&self) -> impl ExactSizeIterator<Item = &FaceHandle> {
        self.faces.iter()
    }
    pub fn edges(&self) -> impl ExactSizeIterator<Item = &EdgeHandle> {
        self.edges.iter()
    }
    pub fn vertices(&self) -> impl ExactSizeIterator<Item = &VertexHandle> {
        self.vertices.iter()
    }
    pub fn topology_history(&self) -> &OperationHistory {
        &self.history
    }
    pub fn resolve<T: ResolvableEntity>(&self, entity: &T) -> Resolution<T> {
        entity.resolve_in_solid(self)
    }
    pub fn origin<T: ResolvableEntity>(&self, entity: &T) -> Option<&Origin> {
        self.history.origin(entity.persistent_name())
    }

    /// Whether an edge separates semantic faces rather than merely subdividing
    /// a faceted construction surface.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(&self.history, edge)
    }
    pub fn faces_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<FaceHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.faces.iter().find(|face| face.name == name).cloned())
            .collect()
    }
    pub fn edges_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<EdgeHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.edges.iter().find(|edge| edge.name == name).cloned())
            .collect()
    }
    pub fn to_compound(&self) -> Compound {
        Compound {
            state: Arc::clone(&self.state),
            solids: vec![self.clone()],
            history: Arc::clone(&self.history),
            lineage: Arc::clone(&self.lineage),
            revisions: Arc::clone(&self.revisions),
            properties: self.properties,
        }
    }
    pub fn union(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().union(&other.to_compound())
    }
    pub fn difference(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().difference(&other.to_compound())
    }
    pub fn intersection(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().intersection(&other.to_compound())
    }
    pub fn transformed(&self, transform: RigidTransform3) -> KernelResult<Solid> {
        self.to_compound()
            .transformed(transform)?
            .solids
            .into_iter()
            .next()
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "rigidly transforming a solid produced no solid",
                )
            })
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        <Self as Tessellate>::tessellate(self, options)
    }
}

impl Tessellate for Solid {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_body(self.body, options)
    }
}

/// Immutable result of a modeling operation, containing zero or more solids.
#[derive(Debug, Clone)]
pub struct Compound {
    state: Arc<Kernel>,
    solids: Vec<Solid>,
    history: Arc<OperationHistory>,
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    properties: MassProperties,
}

impl Compound {
    fn fillet_prism_end_edges(
        &self,
        edges: &[EdgeHandle],
        radius: f64,
    ) -> KernelResult<Option<Self>> {
        let Some(first) = edges.first() else {
            return Ok(None);
        };
        let mut candidate = None;
        for face in first.connected_faces()? {
            let Surface3::Plane(plane) = face.surface() else {
                continue;
            };
            let axis = plane.normal().normalize();
            let endpoints = first.endpoints()?;
            if (endpoints[1] - endpoints[0]).dot(axis).abs()
                > self
                    .tolerance()
                    .linear_at_scale((endpoints[1] - endpoints[0]).magnitude())
            {
                continue;
            }
            if self.is_prism_along(axis)? {
                candidate = Some((face.clone(), *plane, axis));
                break;
            }
        }
        let Some((selected_cap_face, selected_cap, mut axis)) = candidate else {
            return Ok(None);
        };
        let mut bounds = self
            .vertices()
            .map(|vertex| axis.dot(vertex.point().to_vec()))
            .fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
        let selected_level = axis.dot(selected_cap.origin().to_vec());
        if (selected_level - bounds.0).abs() < (selected_level - bounds.1).abs() {
            axis = -axis;
            bounds = (-bounds.1, -bounds.0);
        }
        let height = bounds.1 - bounds.0;
        let base_plane = Plane3::from_origin_normal(Point3::from_vec(axis * bounds.0), axis)?;
        let profile = self
            .profile_on_parallel_cap(axis, bounds.0, base_plane)?
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "analytical fillet could not recover the prism section",
                )
            })?;
        let tolerance = self.tolerance();
        let cap_edges = selected_cap_face.boundary_edges()?;
        let segment_sources = profile
            .outer()
            .segments()
            .iter()
            .map(|segment| {
                let segment_endpoints = [segment.start()?, segment.end()?];
                let matching = cap_edges
                    .iter()
                    .filter_map(|edge| {
                        let endpoints = edge.endpoints().ok()?;
                        let projected = endpoints.map(|point| {
                            let value = base_plane.project(point).ok()?.parameters;
                            Some(Point2::new(value.0, value.1))
                        });
                        let [Some(first), Some(second)] = projected else {
                            return None;
                        };
                        ((first.distance(segment_endpoints[0]) <= tolerance.linear
                            && second.distance(segment_endpoints[1]) <= tolerance.linear)
                            || (first.distance(segment_endpoints[1]) <= tolerance.linear
                                && second.distance(segment_endpoints[0]) <= tolerance.linear))
                            .then_some(edge)
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "prism cap segment does not map uniquely to a source edge",
                    ));
                }
                let edge = matching[0];
                let lateral_faces = edge
                    .connected_faces()?
                    .into_iter()
                    .filter(|face| face.persistent_id() != selected_cap_face.persistent_id())
                    .collect::<Vec<_>>();
                if lateral_faces.len() != 1 {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "prism cap edge does not have one lateral source face",
                    ));
                }
                Ok((edge.persistent_id(), lateral_faces[0].persistent_id()))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let mut selected_segments = Vec::new();
        for edge in edges {
            let endpoints = edge.endpoints()?;
            let projected = endpoints
                .into_iter()
                .map(|point| {
                    let value = base_plane.project(point)?.parameters;
                    Ok(Point2::new(value.0, value.1))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let matches = profile
                .outer()
                .segments()
                .iter()
                .enumerate()
                .filter_map(|(index, segment)| {
                    let start = segment.start().ok()?;
                    let end = segment.end().ok()?;
                    ((start.distance(projected[0]) <= tolerance.linear
                        && end.distance(projected[1]) <= tolerance.linear)
                        || (start.distance(projected[1]) <= tolerance.linear
                            && end.distance(projected[0]) <= tolerance.linear))
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Ok(None);
            }
            selected_segments.push(matches[0]);
        }
        let mut kernel = Kernel::new(tolerance)?;
        let mut transaction = kernel.begin_operation();
        let topology = transaction.extrude_profile_with_end_fillet(
            base_plane,
            &profile,
            axis * height,
            &selected_segments,
            radius,
        )?;
        let properties = end_fillet_mass_properties(
            self.properties,
            base_plane,
            &profile,
            &topology.chosen,
            axis,
            height,
            radius,
        )?;
        let committed = transaction.finish_with_certificate(properties)?;
        let rebuilt = Self::from_operation(kernel, committed, [])?;
        let mut extra_origins = BTreeMap::<PersistentId, Vec<PersistentId>>::new();
        let edge_name = |edge: EdgeId| -> KernelResult<PersistentId> {
            Ok(rebuilt.state.topology().edge(edge)?.name())
        };
        let face_name = |face: FaceId| -> KernelResult<PersistentId> {
            Ok(rebuilt.state.topology().face(face)?.name())
        };
        for index in 0..segment_sources.len() {
            let (source_edge, source_face) = segment_sources[index];
            add_origins(
                &mut extra_origins,
                edge_name(topology.lower_edges[index])?,
                [source_edge],
            );
            add_origins(
                &mut extra_origins,
                edge_name(topology.top_edges[index])?,
                [source_edge],
            );
            add_origins(
                &mut extra_origins,
                face_name(topology.lower_faces[index])?,
                [source_face],
            );
            if topology.chosen[index] {
                add_origins(
                    &mut extra_origins,
                    face_name(topology.blend_faces[index])?,
                    [source_edge, source_face],
                );
            }
            let previous = (index + segment_sources.len() - 1) % segment_sources.len();
            let path_sources = [index, previous]
                .into_iter()
                .filter(|source| topology.chosen[*source])
                .map(|source| segment_sources[source].0)
                .collect::<Vec<_>>();
            if !path_sources.is_empty() {
                add_origins(
                    &mut extra_origins,
                    edge_name(topology.path_edges[index])?,
                    path_sources,
                );
            }
        }
        rebuilt
            .with_geometric_parents(&[self], "fillet", &extra_origins)
            .map(Some)
    }

    pub(crate) fn from_operation<'a>(
        kernel: Kernel,
        committed: CommittedOperation,
        parents: impl IntoIterator<Item = &'a Compound>,
    ) -> KernelResult<Self> {
        let CommittedOperation {
            operation,
            bodies,
            history,
            certificates,
        } = committed;
        Self::from_parts(kernel, operation, bodies, history, certificates, parents)
    }

    fn from_parts<'a>(
        kernel: Kernel,
        operation: uuid::Uuid,
        bodies: Vec<BodyId>,
        history: OperationHistory,
        certificates: BTreeMap<BodyId, PhysicalCertificate>,
        parents: impl IntoIterator<Item = &'a Compound>,
    ) -> KernelResult<Self> {
        let parents = parents.into_iter().collect::<Vec<_>>();
        let current_entities = body_entities(&kernel, &bodies)?;
        let current_names = current_entities.keys().copied().collect::<BTreeSet<_>>();
        let lineage = Arc::new(Lineage::compose(
            parents.iter().map(|parent| Arc::clone(&parent.lineage)),
            &history,
            &current_entities,
        ));
        let revisions = Arc::new(compose_revisions(
            &parents,
            &history,
            &current_names,
            operation,
        ));
        let state = Arc::new(kernel);
        let history = Arc::new(history);
        let mut solids = Vec::with_capacity(bodies.len());
        for body in bodies {
            solids.push(build_solid(
                Arc::clone(&state),
                body,
                Arc::clone(&history),
                Arc::clone(&lineage),
                Arc::clone(&revisions),
                &certificates,
            )?);
        }
        let properties = combine_properties(solids.iter().map(Solid::mass_properties));
        Ok(Self {
            state,
            solids,
            history,
            lineage,
            revisions,
            properties,
        })
    }

    pub fn solids(&self) -> impl ExactSizeIterator<Item = &Solid> {
        self.solids.iter()
    }
    pub(crate) fn tolerance(&self) -> ModelTolerance {
        self.state.tolerance()
    }
    pub(crate) fn face_name(&self, face: FaceId) -> KernelResult<PersistentId> {
        Ok(self.state.topology().face(face)?.name())
    }
    pub fn volume(&self) -> f64 {
        self.properties.volume
    }
    pub fn surface_area(&self) -> f64 {
        self.properties.surface_area
    }
    pub fn centroid(&self) -> Point3 {
        self.properties.centroid
    }
    pub fn mass_properties(&self) -> MassProperties {
        self.properties
    }
    pub fn topology_history(&self) -> &OperationHistory {
        &self.history
    }
    pub fn faces(&self) -> impl Iterator<Item = &FaceHandle> {
        self.solids.iter().flat_map(Solid::faces)
    }
    pub fn edges(&self) -> impl Iterator<Item = &EdgeHandle> {
        self.solids.iter().flat_map(Solid::edges)
    }
    pub fn vertices(&self) -> impl Iterator<Item = &VertexHandle> {
        self.solids.iter().flat_map(Solid::vertices)
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        <Self as Tessellate>::tessellate(self, options)
    }

    pub fn resolve<T: ResolvableEntity>(&self, entity: &T) -> Resolution<T> {
        entity.resolve_in(self)
    }

    pub fn origin<T: ResolvableEntity>(&self, entity: &T) -> Option<&Origin> {
        self.history.origin(entity.persistent_name())
    }

    /// Whether an edge separates semantic faces rather than merely subdividing
    /// a faceted construction surface.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(&self.history, edge)
    }

    pub fn faces_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<FaceHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.faces().find(|face| face.name == name).cloned())
            .collect()
    }

    pub fn edges_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<EdgeHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.edges().find(|edge| edge.name == name).cloned())
            .collect()
    }

    /// Rigidly transform every solid while retaining transitive entity lineage.
    pub fn transformed(&self, transform: RigidTransform3) -> KernelResult<Self> {
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::with_capacity(self.solids.len());
        for solid in &self.solids {
            bodies.push(
                kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)?,
            );
        }
        let mut history = OperationHistory::default();
        for body in &bodies {
            history.absorb(kernel.transform_body(*body, transform)?);
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            [self],
        )
    }

    pub fn boolean(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        ensure_compatible_tolerances(self.state.tolerance(), other.state.tolerance())?;
        if self.solids.len() == 1 && other.solids.len() == 1 {
            return self.boolean_single(other, kind);
        }
        if let Some(result) = self.boolean_parallel_prisms_exact(other, kind)? {
            return Ok(result);
        }
        match kind {
            BooleanKind::Union => self.union_components_iterative(other),
            BooleanKind::Intersection => {
                let mut results = Vec::new();
                for first in &self.solids {
                    for second in &other.solids {
                        let result = first
                            .to_compound()
                            .boolean_single(&second.to_compound(), BooleanKind::Intersection)?;
                        if !result.solids.is_empty() {
                            results.push(result);
                        }
                    }
                }
                combine_compounds(results, self.state.tolerance())
            }
            BooleanKind::Difference => {
                let mut results = Vec::new();
                for first in &self.solids {
                    let mut pieces = vec![first.to_compound()];
                    for tool in &other.solids {
                        let mut next = Vec::new();
                        for piece in pieces {
                            for piece_solid in &piece.solids {
                                let result = piece_solid
                                    .to_compound()
                                    .boolean_single(&tool.to_compound(), BooleanKind::Difference)?;
                                if !result.solids.is_empty() {
                                    next.push(result);
                                }
                            }
                        }
                        pieces = next;
                    }
                    results.extend(pieces);
                }
                combine_compounds(results, self.state.tolerance())
            }
        }
    }

    fn union_components_iterative(&self, other: &Self) -> KernelResult<Self> {
        let mut components = Vec::<Self>::new();
        for solid in self.solids.iter().chain(&other.solids) {
            let mut candidate = solid.to_compound();
            let mut index = 0;
            while index < components.len() {
                let united = components[index].boolean_single(&candidate, BooleanKind::Union)?;
                if united.solids.len() == 1 {
                    candidate = united;
                    components.remove(index);
                    index = 0;
                } else {
                    index += 1;
                }
            }
            components.push(candidate);
        }
        combine_compounds(components, self.state.tolerance())
    }

    /// Reconstruct a Boolean of uniform, parallel analytical prisms as exact
    /// axial layers. Each interval evaluates the two operand profile sets in
    /// two dimensions, then adjacent layers are sewn through Euler operations.
    fn boolean_parallel_prisms_exact(
        &self,
        other: &Self,
        kind: BooleanKind,
    ) -> KernelResult<Option<Self>> {
        let first_operands = self
            .solids
            .iter()
            .map(Solid::to_compound)
            .collect::<Vec<_>>();
        let second_operands = other
            .solids
            .iter()
            .map(Solid::to_compound)
            .collect::<Vec<_>>();
        let operands = first_operands
            .iter()
            .chain(&second_operands)
            .collect::<Vec<_>>();
        let mut candidates = Vec::<Vec3>::new();
        for operand in &operands {
            for face in operand.faces() {
                let candidate = match face.surface() {
                    Surface3::Plane(plane) => Some(plane.normal()),
                    Surface3::Cylinder(cylinder) => Some(cylinder.axis().normalize()),
                    Surface3::Nurbs(surface) => nurbs_translation_axis(surface, self.tolerance()),
                    Surface3::Cone(_) | Surface3::Sphere(_) | Surface3::Torus(_) => None,
                };
                if let Some(candidate) = candidate {
                    let candidate = candidate.normalize();
                    if !candidates.iter().any(|existing| {
                        existing
                            .angle(candidate)
                            .0
                            .min(existing.angle(-candidate).0)
                            <= self.tolerance().angular
                    }) {
                        candidates.push(candidate);
                    }
                }
            }
        }
        let tolerance = self.tolerance();
        let mut common_axis = None;
        for candidate in candidates {
            let candidate = candidate.normalize();
            if operands
                .iter()
                .map(|operand| operand.is_prism_along(candidate))
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .all(|valid| valid)
            {
                common_axis = Some(candidate);
                break;
            }
        }
        let Some(axis) = common_axis else {
            return Ok(None);
        };
        let bounds = operands
            .iter()
            .map(|operand| {
                operand
                    .vertices()
                    .map(|vertex| axis.dot(vertex.point().to_vec()))
                    .fold(
                        (f64::INFINITY, f64::NEG_INFINITY),
                        |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
                    )
            })
            .collect::<Vec<_>>();
        let minimum = bounds
            .iter()
            .map(|bounds| bounds.0)
            .fold(f64::INFINITY, f64::min);
        let maximum = bounds
            .iter()
            .map(|bounds| bounds.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let scale = minimum.abs().max(maximum.abs()).max(1.0);
        let linear = tolerance.linear_at_scale(scale) * 8.0;
        let base_plane = Plane3::from_origin_normal(Point3::from_vec(axis * minimum), axis)?;
        let mut profiles = Vec::with_capacity(operands.len());
        for (operand, bounds) in operands.iter().zip(&bounds) {
            let Some(profile) = operand.profile_on_parallel_cap(axis, bounds.0, base_plane)? else {
                return Ok(None);
            };
            profiles.push(profile);
        }
        let mut levels = bounds
            .iter()
            .flat_map(|bounds| [bounds.0, bounds.1])
            .collect::<Vec<_>>();
        levels.sort_by(f64::total_cmp);
        levels.dedup_by(|first, second| (*first - *second).abs() <= linear);

        let mut layers = Vec::new();
        for interval in levels.windows(2) {
            let start = interval[0];
            let end = interval[1];
            if end - start <= linear {
                continue;
            }
            let middle = (start + end) * 0.5;
            let first_active = profiles
                .iter()
                .zip(&bounds)
                .take(first_operands.len())
                .filter_map(|(profile, bounds)| {
                    (middle > bounds.0 - linear && middle < bounds.1 + linear)
                        .then_some(profile.clone())
                })
                .collect::<Vec<_>>();
            let second_active = profiles
                .iter()
                .zip(&bounds)
                .skip(first_operands.len())
                .filter_map(|(profile, bounds)| {
                    (middle > bounds.0 - linear && middle < bounds.1 + linear)
                        .then_some(profile.clone())
                })
                .collect::<Vec<_>>();
            let regions = boolean_profile_sets(first_active, second_active, kind)?;
            if regions.is_empty() {
                continue;
            }
            let plane = Plane3::new(
                base_plane.origin() + axis * (start - minimum),
                base_plane.u(),
                base_plane.v(),
                tolerance,
            )?;
            let pieces = regions
                .into_iter()
                .map(|profile| profile.extrude(plane, axis * (end - start)))
                .collect::<KernelResult<Vec<_>>>()?;
            layers.push(combine_compounds(pieces, tolerance)?);
        }
        let mut layers = layers.into_iter();
        let mut result = layers
            .next()
            .unwrap_or(combine_compounds(Vec::new(), tolerance)?);
        for layer in layers {
            result = result.union_components_iterative(&layer)?;
        }
        let role = match kind {
            BooleanKind::Union => "boolean.union",
            BooleanKind::Difference => "boolean.difference",
            BooleanKind::Intersection => "boolean.intersection",
        };
        Ok(Some(result.with_boolean_parents(self, other, role)?))
    }

    fn boolean_single(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        debug_assert_eq!(self.solids.len(), 1);
        debug_assert_eq!(other.solids.len(), 1);
        if self
            .conservative_bounds()
            .separated(other.conservative_bounds(), self.tolerance().linear * 8.0)
        {
            let result = match kind {
                BooleanKind::Union => {
                    combine_compounds(vec![self.clone(), other.clone()], self.tolerance())?
                }
                BooleanKind::Difference => self.clone(),
                BooleanKind::Intersection => combine_compounds(Vec::new(), self.tolerance())?,
            };
            let role = match kind {
                BooleanKind::Union => "boolean.union",
                BooleanKind::Difference => "boolean.difference",
                BooleanKind::Intersection => "boolean.intersection",
            };
            return result.with_boolean_parents(self, other, role);
        }
        // Shared analytical faces are sewn directly with Euler operators for
        // both planar and curved prisms. Besides preserving the source BREP,
        // this avoids routing a simple attached boss through the general
        // polyhedral Boolean reconstruction.
        if kind == BooleanKind::Union {
            if let Some(result) = self.union_adjacent_exact(other)? {
                return Ok(result);
            }
        }
        // Parallel analytical prisms are reconstructed as exact profile
        // layers. This also covers tools that pass through an operand and
        // extend beyond it, and disconnected profile sets at several depths.
        if let Some(result) = self.boolean_parallel_prisms_exact(other, kind)? {
            return Ok(result);
        }
        let curved = self
            .faces()
            .chain(other.faces())
            .any(|face| !matches!(face.surface(), Surface3::Plane(_)));
        if curved {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "this curved Boolean does not yet have an analytic construction",
            ));
        }
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let first = kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let second = kernel
            .topology
            .import_body(other.state.topology(), other.solids[0].body)?;
        let mut transaction = kernel.begin_operation();
        transaction.boolean(first, second, kind)?;
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self, other])
    }

    /// A conservative analytic enclosure used only to prove separation. Circle
    /// extents are closed-form and positive-weight NURBS lie in the convex hull
    /// of their control points, so a reported gap cannot hide an intersection.
    fn conservative_bounds(&self) -> ConservativeBounds3 {
        let mut bounds = ConservativeBounds3::empty();
        for vertex in self.vertices() {
            bounds.include(vertex.point());
        }
        for edge in self.edges() {
            match edge.curve() {
                Curve3::Line(_) => {}
                Curve3::Circle(circle) => {
                    let x = circle.x_radius();
                    let y = circle.y_radius();
                    let amplitude = Vec3::new(x.x.hypot(y.x), x.y.hypot(y.y), x.z.hypot(y.z));
                    bounds.include(circle.center() - amplitude);
                    bounds.include(circle.center() + amplitude);
                }
                Curve3::Nurbs(curve) => {
                    for point in curve.control_points() {
                        bounds.include(*point);
                    }
                }
            }
        }
        for face in self.faces() {
            if let Surface3::Nurbs(surface) = face.surface() {
                for row in surface.control_points() {
                    for point in row {
                        bounds.include(*point);
                    }
                }
            }
        }
        bounds
    }

    fn is_prism_along(&self, axis: Vec3) -> KernelResult<bool> {
        let axis = axis.normalize();
        for face in self.faces() {
            match face.surface() {
                Surface3::Plane(plane) => {
                    let alignment = plane.normal().dot(axis).abs();
                    if alignment > self.tolerance().angular
                        && 1.0 - alignment > self.tolerance().angular
                    {
                        return Ok(false);
                    }
                }
                Surface3::Cylinder(cylinder) => {
                    if cylinder
                        .axis()
                        .normalize()
                        .angle(axis)
                        .0
                        .min(cylinder.axis().normalize().angle(-axis).0)
                        > self.tolerance().angular
                    {
                        return Ok(false);
                    }
                }
                Surface3::Nurbs(surface) => {
                    let Some(direction) = nurbs_translation_axis(surface, self.tolerance()) else {
                        return Ok(false);
                    };
                    if direction.angle(axis).0.min(direction.angle(-axis).0)
                        > self.tolerance().angular
                    {
                        return Ok(false);
                    }
                }
                Surface3::Cone(_) | Surface3::Sphere(_) | Surface3::Torus(_) => return Ok(false),
            }
        }
        let (minimum, maximum) = self
            .vertices()
            .map(|vertex| axis.dot(vertex.point().to_vec()))
            .fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
        if maximum - minimum <= self.tolerance().linear {
            return Ok(false);
        }
        let parameter_plane = Plane3::from_origin_normal(Point3::from_vec(axis * minimum), axis)?;
        if self
            .profile_on_parallel_cap(axis, minimum, parameter_plane)?
            .is_none()
        {
            return Ok(false);
        }
        if self
            .profile_on_parallel_cap(axis, maximum, parameter_plane)?
            .is_none()
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn profile_on_parallel_cap(
        &self,
        axis: Vec3,
        minimum: f64,
        target_plane: Plane3,
    ) -> KernelResult<Option<Profile>> {
        let tolerance = self.tolerance();
        let cap = self.faces().find(|face| {
            let Surface3::Plane(plane) = face.surface() else {
                return false;
            };
            if plane
                .normal()
                .angle(axis)
                .0
                .min(plane.normal().angle(-axis).0)
                > tolerance.angular
            {
                return false;
            }
            (axis.dot(plane.origin().to_vec()) - minimum).abs()
                <= tolerance.linear_at_scale(minimum.abs().max(1.0)) * 8.0
        });
        let Some(cap) = cap else {
            return Ok(None);
        };
        let Surface3::Plane(cap_plane) = cap.surface() else {
            unreachable!()
        };
        let map = |point: Point2| -> KernelResult<Point2> {
            let world = cap_plane.evaluate(point.x, point.y)?.point;
            let projected = target_plane.project(world)?.parameters;
            Ok(Point2::new(projected.0, projected.1))
        };
        let topology = self.state.topology();
        let mut outer = None;
        let mut holes = Vec::new();
        for loop_id in topology.face(cap.face)?.loops() {
            let mut segments = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let range = coedge.range();
                let start = map(coedge.pcurve().evaluate(range.start)?.point)?;
                let end = map(coedge.pcurve().evaluate(range.end)?.point)?;
                let segment = match coedge.pcurve() {
                    Curve2::Line(_) => TrimmedCurve2::line(start, end)?,
                    Curve2::Circle(_) => {
                        if start.distance(end) <= tolerance.linear {
                            let quarter = map(coedge
                                .pcurve()
                                .evaluate(range.start + range.span() * 0.25)?
                                .point)?;
                            let center = map(match coedge.pcurve() {
                                Curve2::Circle(circle) => circle.center(),
                                _ => unreachable!(),
                            })?;
                            let radius = center.distance(start);
                            let orientation = (start - center).x * (quarter - center).y
                                - (start - center).y * (quarter - center).x;
                            TrimmedCurve2::new(
                                Circle2::new(center, radius)?,
                                ParameterRange::new(
                                    0.0,
                                    orientation.signum() * std::f64::consts::TAU,
                                )?,
                            )?
                        } else {
                            let middle = map(coedge
                                .pcurve()
                                .evaluate((range.start + range.end) * 0.5)?
                                .point)?;
                            TrimmedCurve2::arc_through(start, middle, end)?
                        }
                    }
                    Curve2::Nurbs(curve) => TrimmedCurve2::new(
                        NurbsCurve2::new(
                            curve.degree(),
                            curve
                                .control_points()
                                .iter()
                                .copied()
                                .map(map)
                                .collect::<KernelResult<Vec<_>>>()?,
                            curve.weights().to_vec(),
                            curve.knots().to_vec(),
                        )?,
                        range,
                    )?,
                };
                segments.push(segment);
            }
            let wire = Wire2::new(segments, tolerance)?;
            match topology.loop_(*loop_id)?.kind() {
                LoopKind::Outer => outer = Some(wire),
                LoopKind::Inner => holes.push(wire),
            }
        }
        let Some(outer) = outer else {
            return Ok(None);
        };
        Profile::new(outer, holes, tolerance).map(Some)
    }

    fn with_geometric_parents(
        &self,
        parents: &[&Self],
        role: &str,
        extra_origins: &BTreeMap<PersistentId, Vec<PersistentId>>,
    ) -> KernelResult<Self> {
        let tolerance = self.tolerance();
        let source_vertices = parents
            .iter()
            .flat_map(|parent| parent.vertices())
            .map(|vertex| (vertex.persistent_id(), vertex.point()))
            .collect::<BTreeMap<_, _>>();
        let mut source_edges = BTreeMap::new();
        for parent in parents {
            for edge in parent.edges() {
                source_edges.insert(
                    edge.persistent_id(),
                    (
                        edge.curve().clone(),
                        edge.connected_faces()?
                            .into_iter()
                            .map(|face| face.surface().clone())
                            .collect::<Vec<_>>(),
                    ),
                );
            }
        }
        let source_faces = parents
            .iter()
            .flat_map(|parent| parent.faces())
            .map(|face| (face.persistent_id(), face.surface().clone()))
            .collect::<BTreeMap<_, _>>();
        let output_vertices = self
            .vertices()
            .map(|vertex| (vertex.persistent_id(), vertex.point()))
            .collect::<BTreeMap<_, _>>();
        let output_edges = self
            .edges()
            .map(|edge| {
                Ok((
                    edge.persistent_id(),
                    (
                        edge.curve().clone(),
                        edge.connected_faces()?
                            .into_iter()
                            .map(|face| face.surface().clone())
                            .collect::<Vec<_>>(),
                    ),
                ))
            })
            .collect::<KernelResult<BTreeMap<_, _>>>()?;
        let output_faces = self
            .faces()
            .map(|face| (face.persistent_id(), face.surface().clone()))
            .collect::<BTreeMap<_, _>>();
        let mut kernel = Kernel::new(tolerance)?;
        let bodies = self
            .solids
            .iter()
            .map(|solid| {
                kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let outputs = body_entities(&kernel, &bodies)?;
        let mut sources = BTreeMap::new();
        for parent in parents {
            sources.extend(body_entities(
                &parent.state,
                &parent
                    .solids
                    .iter()
                    .map(|solid| solid.body)
                    .collect::<Vec<_>>(),
            )?);
        }
        let related = |kind: EntityKind, source: PersistentId, output: PersistentId| match kind {
            EntityKind::Vertex => source_vertices
                .get(&source)
                .zip(output_vertices.get(&output))
                .is_some_and(|(first, second)| first.distance(*second) <= tolerance.linear),
            EntityKind::Edge => source_edges
                .get(&source)
                .zip(output_edges.get(&output))
                .is_some_and(|(first, second)| {
                    same_curve_support(&first.0, &second.0, tolerance)
                        && same_surface_support_set(&first.1, &second.1, tolerance)
                }),
            EntityKind::Face => source_faces
                .get(&source)
                .zip(output_faces.get(&output))
                .is_some_and(|(first, second)| same_surface_support(first, second, tolerance)),
        };
        let mut history = OperationHistory::default();
        for (output, kind) in &outputs {
            let mut contributors = sources
                .iter()
                .filter_map(|(source, source_kind)| {
                    (source_kind == kind && related(*kind, *source, *output)).then_some(*source)
                })
                .collect::<Vec<_>>();
            if contributors.is_empty() {
                if let Some(sources) = extra_origins.get(output) {
                    history.derived(*output, sources.iter().copied(), role);
                } else {
                    history.created(*output);
                }
            } else {
                history.derived(*output, contributors.drain(..), role);
            }
        }
        for (source, kind) in sources {
            let descendants = outputs
                .iter()
                .filter_map(|(output, output_kind)| {
                    (output_kind == &kind
                        && (related(kind, source, *output)
                            || extra_origins
                                .get(output)
                                .is_some_and(|origins| origins.contains(&source))))
                    .then_some(*output)
                })
                .collect::<Vec<_>>();
            history.changed(
                source,
                match descendants.len() {
                    0 => ChangeKind::Deleted,
                    1 => ChangeKind::Modified,
                    _ => ChangeKind::Split,
                },
                descendants,
            );
        }
        // This operation only rebuilds stable names and lineage around an
        // unchanged, already-published exact BREP. Preserve its analytical
        // certificates instead of asking the primitive recognizers to
        // reclassify a composite rational blend.
        let certificates = bodies
            .iter()
            .copied()
            .zip(self.solids.iter().map(|solid| PhysicalCertificate {
                properties: solid.mass_properties(),
            }))
            .collect::<BTreeMap<_, _>>();
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            parents.iter().copied(),
        )
    }

    fn with_boolean_parents(&self, first: &Self, second: &Self, role: &str) -> KernelResult<Self> {
        self.with_geometric_parents(&[first, second], role, &BTreeMap::new())
    }

    fn union_adjacent_exact(&self, other: &Self) -> KernelResult<Option<Self>> {
        let equal_faces = super::features::coincident_opposite_face_names(self, other)?;
        let patch_faces = if equal_faces.is_none() {
            super::features::coincident_opposite_face_patch_names(self, other)?
        } else {
            None
        };
        let Some((target_is_first, target_name, source_name, whole_face)) = equal_faces
            .map(|(first, second)| (true, first, second, true))
            .or_else(|| patch_faces.map(|(first, target, source)| (first, target, source, false)))
        else {
            return Ok(None);
        };
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let first = kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let second = kernel
            .topology
            .import_body(other.state.topology(), other.solids[0].body)?;
        let target_face = kernel
            .topology
            .faces()
            .find_map(|(id, face)| (face.name() == target_name).then_some(id))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::MissingEntity,
                    "imported adjacent union lost its target face",
                )
            })?;
        let source_face = kernel
            .topology
            .faces()
            .find_map(|(id, face)| (face.name() == source_name).then_some(id))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::MissingEntity,
                    "imported adjacent union lost its source face",
                )
            })?;
        let (target, source) = if target_is_first {
            (first, second)
        } else {
            (second, first)
        };
        let mut transaction = kernel.begin_operation();
        if whole_face {
            transaction.sew_bodies_across_faces(target, source, target_face, source_face)?;
        } else {
            transaction.sew_body_across_face_patch(target, source, target_face, source_face)?;
        }
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self, other]).map(Some)
    }

    pub fn union(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Union)
    }
    pub fn difference(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Difference)
    }
    pub fn intersection(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Intersection)
    }

    pub fn shell(&self, openings: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let selected = openings
                .iter()
                .map(FaceHandle::persistent_id)
                .collect::<BTreeSet<_>>();
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .faces()
                    .map(FaceHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = openings
                    .iter()
                    .filter(|face| names.contains(&face.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                debug_assert!(local
                    .iter()
                    .all(|face| selected.contains(&face.persistent_id())));
                results.push(solid.to_compound().shell(&local, distance)?);
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::shell(self, openings, distance)
    }

    pub fn fillet(&self, edges: &[EdgeHandle], radius: f64) -> KernelResult<Self> {
        if !radius.is_finite() || radius <= self.tolerance().linear {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "fillet radius must be finite and exceed model tolerance",
            ));
        }
        if edges.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "fillet needs at least one edge",
            ));
        }
        let mut expanded = Vec::new();
        for edge in edges {
            match self.resolve(edge) {
                Resolution::Preserved(edge)
                | Resolution::Modified(edge)
                | Resolution::Merged(edge) => expanded.push(edge),
                Resolution::Split(descendants) => expanded.extend(descendants),
                Resolution::Deleted => {
                    return Err(KernelError::new(
                        DiagnosticKind::MissingEntity,
                        "selected fillet edge was deleted by an earlier operation",
                    ))
                }
                Resolution::Unrelated => {
                    return Err(KernelError::new(
                        DiagnosticKind::MissingEntity,
                        "selected fillet edge is unrelated to this compound",
                    ))
                }
            }
        }
        expanded.sort_by_key(EdgeHandle::persistent_id);
        expanded.dedup_by_key(|edge| edge.persistent_id());
        if self.solids.len() > 1 {
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .edges()
                    .map(EdgeHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = expanded
                    .iter()
                    .filter(|edge| names.contains(&edge.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                if local.is_empty() {
                    results.push(solid.to_compound());
                } else {
                    results.push(solid.to_compound().fillet(&local, radius)?);
                }
            }
            return combine_compounds(results, self.state.tolerance());
        }
        if expanded.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::MissingEntity,
                "selected fillet edges do not belong to this compound",
            ));
        }

        let follows_prism_axis = if matches!(expanded[0].curve(), Curve3::Line(_)) {
            let endpoints = expanded[0].endpoints()?;
            let vector = endpoints[1] - endpoints[0];
            vector.magnitude() > self.tolerance().linear
                && self.is_prism_along(vector.normalize())?
        } else {
            false
        };
        if !follows_prism_axis {
            if let Some(result) = self.fillet_prism_end_edges(&expanded, radius)? {
                return Ok(result);
            }
        }

        let first_endpoints = expanded[0].endpoints()?;
        let first_vector = first_endpoints[1] - first_endpoints[0];
        if !matches!(expanded[0].curve(), Curve3::Line(_))
            || first_vector.magnitude() <= self.tolerance().linear
        {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "analytical fillets currently support straight prism edges",
            ));
        }
        let axis = first_vector.normalize();
        let tolerance = self.tolerance();
        for edge in &expanded {
            let endpoints = edge.endpoints()?;
            let vector = endpoints[1] - endpoints[0];
            if !matches!(edge.curve(), Curve3::Line(_))
                || vector.magnitude() <= tolerance.linear
                || vector
                    .normalize()
                    .angle(axis)
                    .0
                    .min(vector.normalize().angle(-axis).0)
                    > tolerance.angular
            {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "one analytical fillet operation currently requires parallel straight edges",
                ));
            }
        }
        if !self.is_prism_along(axis)? {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "selected straight edges do not define a uniform analytical prism",
            ));
        }
        let (minimum, maximum) = self
            .vertices()
            .map(|vertex| axis.dot(vertex.point().to_vec()))
            .fold(
                (f64::INFINITY, f64::NEG_INFINITY),
                |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
            );
        let height = maximum - minimum;
        let span_tolerance = tolerance.linear_at_scale(height.abs().max(1.0)) * 8.0;
        for edge in &expanded {
            let mut parameters = edge
                .endpoints()?
                .into_iter()
                .map(|point| axis.dot(point.to_vec()))
                .collect::<Vec<_>>();
            parameters.sort_by(f64::total_cmp);
            if (parameters[0] - minimum).abs() > span_tolerance
                || (parameters[1] - maximum).abs() > span_tolerance
            {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "analytical prism fillets currently require full-length sweep edges",
                ));
            }
        }
        let plane = Plane3::from_origin_normal(Point3::from_vec(axis * minimum), axis)?;
        let profile = self
            .profile_on_parallel_cap(axis, minimum, plane)?
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "analytical fillet could not recover the prism section",
                )
            })?;
        let selected = expanded
            .iter()
            .map(|edge| {
                let point = edge.endpoints()?[0];
                let projected = plane.project(point)?.parameters;
                Ok((edge.persistent_id(), Point2::new(projected.0, projected.1)))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let (rounded_profile, rounded_corners) =
            round_profile_outer_corners(&profile, &selected, radius)?;
        let rebuilt = rounded_profile.extrude(plane, axis * height)?;
        let mut extra_origins = BTreeMap::<PersistentId, Vec<PersistentId>>::new();
        for face in rebuilt.faces() {
            let Surface3::Cylinder(cylinder) = face.surface() else {
                continue;
            };
            for corner in &rounded_corners {
                let center = plane.evaluate(corner.center.x, corner.center.y)?.point;
                let parallel = cylinder
                    .axis()
                    .normalize()
                    .angle(axis)
                    .0
                    .min(cylinder.axis().normalize().angle(-axis).0)
                    <= tolerance.angular;
                if parallel
                    && (cylinder.radius() - radius).abs() <= span_tolerance
                    && (cylinder.origin() - center).cross(axis).magnitude() <= span_tolerance
                {
                    extra_origins.insert(face.persistent_id(), vec![corner.source]);
                }
            }
        }
        rebuilt.with_geometric_parents(&[self], "fillet", &extra_origins)
    }

    pub fn combined(&self, others: &[Self]) -> KernelResult<Self> {
        let parents = std::iter::once(self)
            .chain(others.iter())
            .collect::<Vec<_>>();
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::new();
        for parent in &parents {
            ensure_compatible_tolerances(self.state.tolerance(), parent.state.tolerance())?;
            for solid in &parent.solids {
                bodies.push(
                    kernel
                        .topology
                        .import_body(solid.state.topology(), solid.body)?,
                );
            }
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            OperationHistory::default(),
            certificates,
            parents,
        )
    }

    pub fn split(&self, plane: Plane3, keep: &str) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let results = self
                .solids
                .iter()
                .map(|solid| solid.to_compound().split(plane, keep))
                .collect::<KernelResult<Vec<_>>>()?;
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::split(self, plane, keep)
    }

    pub fn without_solids(&self, removed: &[Solid]) -> KernelResult<Self> {
        let removed = removed
            .iter()
            .map(|solid| solid.body)
            .collect::<BTreeSet<_>>();
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::new();
        for solid in &self.solids {
            if !removed.contains(&solid.body) {
                bodies.push(
                    kernel
                        .topology
                        .import_body(solid.state.topology(), solid.body)?,
                );
            }
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            OperationHistory::default(),
            certificates,
            [self],
        )
    }

    pub fn move_faces(
        &self,
        faces: &[FaceHandle],
        transform: RigidTransform3,
    ) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .faces()
                    .map(FaceHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = faces
                    .iter()
                    .filter(|face| names.contains(&face.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                if local.is_empty() {
                    results.push(solid.to_compound());
                } else {
                    results.push(solid.to_compound().move_faces(&local, transform)?);
                }
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::move_faces(self, faces, transform)
    }

    pub fn replace_face(
        &self,
        target: &FaceHandle,
        replacement: &FaceHandle,
    ) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let mut found = false;
            let mut results = Vec::new();
            for solid in &self.solids {
                let contains = solid
                    .faces()
                    .any(|face| face.persistent_id() == target.persistent_id());
                if contains {
                    found = true;
                    results.push(solid.to_compound().replace_face(target, replacement)?);
                } else {
                    results.push(solid.to_compound());
                }
            }
            if !found {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "replacement target face does not belong to the compound",
                ));
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::replace_face(self, target, replacement)
    }
}

impl Tessellate for Compound {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        let mut mesh = TriangleMesh::default();
        for solid in &self.solids {
            mesh.append(solid.tessellate(options)?);
        }
        Ok(mesh)
    }
}

fn nurbs_translation_axis(
    surface: &super::NurbsSurface3,
    tolerance: ModelTolerance,
) -> Option<Vec3> {
    if surface.degrees().1 != 1 || surface.control_points().len() != 2 {
        return None;
    }
    let first = &surface.control_points()[0];
    let second = &surface.control_points()[1];
    let vector = second
        .first()
        .zip(first.first())
        .map(|(top, bottom)| *top - *bottom)?;
    let scale = vector.magnitude().max(1.0);
    (vector.magnitude() > tolerance.linear
        && first.iter().zip(second).all(|(bottom, top)| {
            ((*top - *bottom) - vector).magnitude() <= tolerance.linear_at_scale(scale) * 8.0
        }))
    .then(|| vector.normalize())
}

fn boolean_profile_sets(
    first: impl IntoIterator<Item = Profile>,
    second: impl IntoIterator<Item = Profile>,
    kind: BooleanKind,
) -> KernelResult<Vec<Profile>> {
    let first = union_profile_set(first)?;
    let second = union_profile_set(second)?;
    match kind {
        BooleanKind::Union => union_profile_set(first.into_iter().chain(second)),
        BooleanKind::Intersection => {
            let mut intersections = Vec::new();
            for first_region in &first {
                for second_region in &second {
                    intersections.extend(
                        first_region.boolean_planar(second_region, BooleanKind::Intersection)?,
                    );
                }
            }
            union_profile_set(intersections)
        }
        BooleanKind::Difference => {
            let mut output = Vec::new();
            for first_region in first {
                let mut pieces = vec![first_region];
                for second_region in &second {
                    let mut next = Vec::new();
                    for piece in pieces {
                        next.extend(piece.boolean_planar(second_region, BooleanKind::Difference)?);
                    }
                    pieces = next;
                }
                output.extend(pieces);
            }
            union_profile_set(output)
        }
    }
}

fn union_profile_set(profiles: impl IntoIterator<Item = Profile>) -> KernelResult<Vec<Profile>> {
    let mut components = Vec::<Profile>::new();
    for mut candidate in profiles {
        let mut index = 0;
        while index < components.len() {
            let united = components[index].boolean_planar(&candidate, BooleanKind::Union)?;
            if united.len() == 1 {
                candidate = united.into_iter().next().expect("one union component");
                components.remove(index);
                index = 0;
            } else {
                index += 1;
            }
        }
        components.push(candidate);
    }
    Ok(components)
}

fn combine_compounds(
    mut compounds: Vec<Compound>,
    tolerance: ModelTolerance,
) -> KernelResult<Compound> {
    if compounds.is_empty() {
        return Compound::from_parts(
            Kernel::new(tolerance)?,
            super::identity::fresh_uuid(),
            Vec::new(),
            OperationHistory::default(),
            BTreeMap::new(),
            [],
        );
    }
    let first = compounds.remove(0);
    first.combined(&compounds)
}

fn resolve_entity_from<T: Clone>(
    lineage_graph: &Lineage,
    revisions: &BTreeMap<PersistentId, uuid::Uuid>,
    kind: EntityKind,
    source: PersistentId,
    revision: uuid::Uuid,
    find: impl Fn(PersistentId) -> Option<T>,
) -> Resolution<T> {
    let Some(lineage) = lineage_graph.entries.get(&(kind, source)) else {
        return Resolution::Unrelated;
    };
    let descendants = lineage
        .descendants
        .iter()
        .filter_map(|output| find(*output))
        .collect::<Vec<_>>();
    match descendants.len() {
        0 => Resolution::Deleted,
        1 if lineage.merged => Resolution::Merged(descendants[0].clone()),
        1 if !lineage.changed && revisions.get(&source).copied() == Some(revision) => {
            Resolution::Preserved(descendants[0].clone())
        }
        1 => Resolution::Modified(descendants[0].clone()),
        _ => Resolution::Split(descendants),
    }
}

fn generated_names(
    history: &OperationHistory,
    source: PersistentId,
) -> impl Iterator<Item = PersistentId> + '_ {
    history
        .origins()
        .iter()
        .filter_map(move |(output, origin)| {
            matches!(origin, Origin::Derived { sources, .. } if sources.contains(&source))
                .then_some(*output)
        })
}

impl Profile {
    /// Extrude this profile into an independently owned, certified compound.
    pub fn extrude(&self, plane: Plane3, vector: Vec3) -> KernelResult<Compound> {
        let mut kernel = Kernel::new(self.tolerance())?;
        let mut transaction = kernel.begin_operation();
        transaction.extrude_curve_profile(plane, self, vector)?;
        let committed = transaction.finish()?;
        Compound::from_operation(kernel, committed, [])
    }
}

fn build_solid(
    state: Arc<Kernel>,
    body: BodyId,
    history: Arc<OperationHistory>,
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    certificates: &BTreeMap<BodyId, PhysicalCertificate>,
) -> KernelResult<Solid> {
    let properties = certificates
        .get(&body)
        .map(|certificate| certificate.properties)
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "published solid is missing its physical certificate",
            )
        })?;
    let topology = state.topology();
    let mut face_ids = BTreeSet::new();
    let mut edge_ids = BTreeSet::new();
    let mut vertex_ids = BTreeSet::new();
    for shell in topology.body(body)?.shells() {
        for face in topology.shell(*shell)?.faces() {
            face_ids.insert(*face);
            for edge in topology.face_edges(*face)? {
                edge_ids.insert(edge);
                vertex_ids.extend(topology.edge(edge)?.vertices());
            }
        }
    }
    let faces = face_ids
        .into_iter()
        .map(|face| build_face_handle(Arc::clone(&state), face, Arc::clone(&revisions)))
        .collect::<KernelResult<Vec<_>>>()?;
    let edges = edge_ids
        .into_iter()
        .map(|edge| build_edge_handle(Arc::clone(&state), edge, Arc::clone(&revisions)))
        .collect::<KernelResult<Vec<_>>>()?;
    let vertices = vertex_ids
        .into_iter()
        .map(|vertex| {
            let name = topology.vertex(vertex)?.name();
            Ok(VertexHandle {
                name,
                revision: revisions[&name],
                point: topology.vertex(vertex)?.point(),
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    Ok(Solid {
        state,
        body,
        properties,
        faces,
        edges,
        vertices,
        history,
        lineage,
        revisions,
    })
}

fn build_face_handle(
    state: Arc<Kernel>,
    face: FaceId,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
) -> KernelResult<FaceHandle> {
    let topology = state.topology();
    let entity = topology.face(face)?;
    let name = entity.name();
    let surface = entity.surface().clone();
    let revision = revisions[&name];
    Ok(FaceHandle {
        state,
        revisions,
        face,
        name,
        revision,
        surface,
    })
}

fn build_edge_handle(
    state: Arc<Kernel>,
    edge: EdgeId,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
) -> KernelResult<EdgeHandle> {
    let topology = state.topology();
    let entity = topology.edge(edge)?;
    let name = entity.name();
    let curve = entity.curve().clone();
    let revision = revisions[&name];
    Ok(EdgeHandle {
        state,
        revisions,
        edge,
        name,
        revision,
        curve,
    })
}

fn body_entities(
    kernel: &Kernel,
    bodies: &[BodyId],
) -> KernelResult<BTreeMap<PersistentId, EntityKind>> {
    let mut entities = BTreeMap::new();
    for body in bodies {
        for shell in kernel.topology().body(*body)?.shells() {
            for face in kernel.topology().shell(*shell)?.faces() {
                entities.insert(kernel.topology().face(*face)?.name(), EntityKind::Face);
                for edge in kernel.topology().face_edges(*face)? {
                    entities.insert(kernel.topology().edge(edge)?.name(), EntityKind::Edge);
                    for vertex in kernel.topology().edge(edge)?.vertices() {
                        entities
                            .insert(kernel.topology().vertex(vertex)?.name(), EntityKind::Vertex);
                    }
                }
            }
        }
    }
    Ok(entities)
}

fn compose_revisions(
    parents: &[&Compound],
    history: &OperationHistory,
    current: &BTreeSet<PersistentId>,
    operation: uuid::Uuid,
) -> BTreeMap<PersistentId, uuid::Uuid> {
    let generated = generated_index(history);
    current
        .iter()
        .copied()
        .map(|name| {
            let explicitly_changed = history.changes().contains_key(&name)
                || history
                    .changes()
                    .values()
                    .any(|(_, outputs)| outputs.contains(&name))
                || generated.values().any(|outputs| outputs.contains(&name));
            let inherited = (!explicitly_changed)
                .then(|| {
                    parents
                        .iter()
                        .find_map(|parent| parent.revisions.get(&name).copied())
                })
                .flatten();
            (name, inherited.unwrap_or(operation))
        })
        .collect()
}

fn certify_bodies(
    kernel: &Kernel,
    bodies: &[BodyId],
) -> KernelResult<BTreeMap<BodyId, PhysicalCertificate>> {
    let all = super::physical::certify_topology(kernel.topology(), kernel.tolerance())
        .map_err(KernelError::from_diagnostics)?;
    bodies
        .iter()
        .copied()
        .map(|body| {
            all.get(&body)
                .cloned()
                .map(|value| (body, value))
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "published solid is missing its physical certificate",
                    )
                })
        })
        .collect()
}

fn ensure_compatible_tolerances(
    first: super::ModelTolerance,
    second: super::ModelTolerance,
) -> KernelResult<()> {
    if first != second {
        return Err(KernelError::new(
            DiagnosticKind::InvalidInput,
            "modeling operands must use the same tolerance policy",
        ));
    }
    Ok(())
}

fn combine_properties(properties: impl IntoIterator<Item = MassProperties>) -> MassProperties {
    let properties = properties.into_iter().collect::<Vec<_>>();
    let volume = properties.iter().map(|value| value.volume).sum::<f64>();
    let surface_area = properties
        .iter()
        .map(|value| value.surface_area)
        .sum::<f64>();
    let centroid = if volume > 0.0 {
        Point3::from_vec(
            properties
                .iter()
                .map(|value| value.centroid.to_vec() * value.volume)
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, value| sum + value)
                / volume,
        )
    } else {
        Point3::new(0.0, 0.0, 0.0)
    };
    let mut inertia = [[0.0; 3]; 3];
    for value in &properties {
        let offset = value.centroid - centroid;
        let squared = offset.magnitude2();
        for (row, inertia_row) in inertia.iter_mut().enumerate() {
            for (column, inertia_value) in inertia_row.iter_mut().enumerate() {
                *inertia_value += value.inertia[row][column]
                    + value.volume
                        * if row == column {
                            squared - [offset.x, offset.y, offset.z][row].powi(2)
                        } else {
                            -[offset.x, offset.y, offset.z][row]
                                * [offset.x, offset.y, offset.z][column]
                        };
            }
        }
    }
    MassProperties {
        volume,
        signed_volume: volume,
        surface_area,
        centroid,
        inertia,
    }
}
