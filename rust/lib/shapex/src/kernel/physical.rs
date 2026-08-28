use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3};

use super::mass_properties::mass_properties_from_mesh;
use super::{
    BodyId, Curve2, Curve3, Diagnostic, DiagnosticKind, FaceId, Kernel, KernelError, KernelResult,
    MassProperties, ModelTolerance, ShellId, Surface3, TessellationOptions, Topology, TriangleMesh,
};

#[derive(Debug, Clone)]
pub(crate) struct PhysicalCertificate {
    pub properties: MassProperties,
}

pub(crate) fn certify_topology(
    topology: &Topology,
    tolerance: ModelTolerance,
) -> Result<BTreeMap<BodyId, PhysicalCertificate>, Vec<Diagnostic>> {
    let kernel = Kernel {
        tolerance,
        topology: topology.clone(),
    };
    let mut certificates = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for (body_id, body) in topology.bodies() {
        match certify_body(&kernel, body_id) {
            Ok(certificate) => {
                certificates.insert(body_id, certificate);
            }
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    diagnostics.push(diagnostic.clone().with_entity(body.name()));
                }
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(certificates)
    } else {
        Err(diagnostics)
    }
}

fn certify_body(kernel: &Kernel, body_id: BodyId) -> KernelResult<PhysicalCertificate> {
    let topology = kernel.topology();
    let body = topology.body(body_id)?;
    if body.shells().is_empty() {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "a physical body needs at least one shell",
        ));
    }
    // The first certifier is restricted to polyhedral BREPs and certifies them at
    // the model's scale-aware tolerance. Curved certification is deliberately
    // rejected rather than inferred from an approximate mesh.
    let shell_ids: BTreeSet<_> = body.shells().iter().copied().collect();
    let face_ids: BTreeSet<_> = shell_ids
        .iter()
        .flat_map(|shell_id| {
            topology
                .shell(*shell_id)
                .map(|shell| shell.faces().to_vec())
                .unwrap_or_default()
        })
        .collect();
    let edge_ids: BTreeSet<_> = topology
        .edges()
        .filter_map(|(edge_id, edge)| {
            edge.coedges()
                .iter()
                .any(|coedge_id| {
                    topology
                        .coedge(*coedge_id)
                        .ok()
                        .and_then(|coedge| topology.loop_(coedge.loop_id()).ok())
                        .map(|loop_| face_ids.contains(&loop_.face()))
                        .unwrap_or(false)
                })
                .then_some(edge_id)
        })
        .collect();
    let body_scale = edge_ids.iter().try_fold(1.0_f64, |scale, edge_id| {
        let edge = topology.edge(*edge_id)?;
        let first = topology
            .vertex(edge.vertices()[0])?
            .point()
            .to_vec()
            .magnitude();
        let second = topology
            .vertex(edge.vertices()[1])?
            .point()
            .to_vec()
            .magnitude();
        Ok::<f64, KernelError>(scale.max(first).max(second))
    })?;
    let physical_tolerance = ModelTolerance {
        linear: kernel.tolerance().linear_at_scale(body_scale),
        ..kernel.tolerance()
    };
    if let Some(certificate) =
        try_certify_torus(topology, body_id, &face_ids, &edge_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_sphere(topology, body_id, &face_ids, &edge_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_cone(topology, body_id, &face_ids, &edge_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_cylinder(topology, body_id, &face_ids, &edge_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_right_prism(topology, body_id, &face_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_prism_with_bosses(topology, body_id, &face_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    if let Some(certificate) =
        try_certify_prism_with_pockets(topology, body_id, &face_ids, physical_tolerance)?
    {
        return Ok(certificate);
    }
    for edge_id in &edge_ids {
        let edge = topology.edge(*edge_id)?;
        if !matches!(edge.curve(), Curve3::Line(_)) {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "certified physical publication currently requires linear edges",
            ));
        }
        for coedge_id in edge.coedges() {
            let coedge = topology.coedge(*coedge_id)?;
            let face = topology.loop_(coedge.loop_id())?.face();
            if face_ids.contains(&face) && !matches!(coedge.pcurve(), Curve2::Line(_)) {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "certified polyhedral publication requires linear p-curves",
                ));
            }
        }
    }
    for face_id in face_ids {
        if !matches!(topology.face(face_id)?.surface(), Surface3::Plane(_)) {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "certified physical publication currently requires planar faces",
            ));
        }
        validate_planar_trim(topology, face_id, physical_tolerance)?;
    }

    let options =
        TessellationOptions::new(physical_tolerance.linear, physical_tolerance.angular, 8)?;
    let mut shell_meshes = Vec::new();
    for shell_id in body.shells() {
        validate_shell_connected(topology, *shell_id)?;
        let mesh = kernel.tessellate_shell(*shell_id, options)?;
        validate_no_self_intersections(topology, &mesh, physical_tolerance)?;
        let properties = mass_properties_from_mesh(&mesh)?;
        if properties.volume <= physical_tolerance.linear.powi(3) {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "shell volume does not exceed the model tolerance volume",
            ));
        }
        shell_meshes.push((*shell_id, mesh, properties));
    }

    // One connected positive outer shell; all other shells are negatively oriented cavities.
    let positive: Vec<_> = shell_meshes
        .iter()
        .filter(|(_, _, properties)| properties.signed_volume > 0.0)
        .collect();
    if positive.len() != 1 {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "a body must contain exactly one outward-oriented outer shell",
        ));
    }
    let outer_shell = positive[0].0;
    let outer_mesh = &positive[0].1;
    for (shell_id, mesh, properties) in &shell_meshes {
        if *shell_id == outer_shell {
            continue;
        }
        if properties.signed_volume >= 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "inner shells must be oriented as cavities",
            ));
        }
        validate_meshes_disjoint(topology, outer_mesh, mesh, physical_tolerance)?;
        if !point_inside_mesh(properties.centroid, outer_mesh, physical_tolerance)? {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "cavity shell is not contained by the outer shell",
            ));
        }
    }
    for first in 0..shell_meshes.len() {
        for second in first + 1..shell_meshes.len() {
            if shell_meshes[first].0 == outer_shell || shell_meshes[second].0 == outer_shell {
                continue;
            }
            validate_meshes_disjoint(
                topology,
                &shell_meshes[first].1,
                &shell_meshes[second].1,
                physical_tolerance,
            )?;
            let first_center = shell_meshes[first].2.centroid;
            let second_center = shell_meshes[second].2.centroid;
            if point_inside_mesh(first_center, &shell_meshes[second].1, physical_tolerance)?
                || point_inside_mesh(second_center, &shell_meshes[first].1, physical_tolerance)?
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "cavity shells may not contain or overlap one another",
                ));
            }
        }
    }

    let mut combined = TriangleMesh::default();
    for (_, mesh, _) in shell_meshes {
        combined.append(mesh);
    }
    let properties = mass_properties_from_mesh(&combined)?;
    if properties.signed_volume <= physical_tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "body has no positive physical volume after subtracting cavities",
        ));
    }
    Ok(PhysicalCertificate { properties })
}

#[derive(Debug, Clone, Copy)]
struct PlanarSection {
    area_uv: f64,
    first_x: f64,
    first_y: f64,
    second_xx: f64,
    second_yy: f64,
    second_xy: f64,
    perimeter: f64,
    edge_count: usize,
}

/// Recognize and certify a straight, normal sweep directly from its completed
/// BREP.  This deliberately does not rely on constructor provenance: a solid
/// assembled through the public Euler operators receives the same treatment.
/// Line, circular-arc, and rational spline boundaries are integrated directly
/// in parameter space. NURBS intervals use bounded adaptive quadrature over
/// their exact rational representation; no tessellated geometry participates.
fn try_certify_right_prism(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 || face_ids.len() < 3 {
        return Ok(None);
    }
    validate_shell_connected(topology, body.shells()[0])?;
    let planar = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Plane(plane) => Some((*face_id, *plane)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for first_index in 0..planar.len() {
        for second_index in first_index + 1..planar.len() {
            let (first_id, first_plane) = planar[first_index];
            let (second_id, second_plane) = planar[second_index];
            let first_face = topology.face(first_id)?;
            let second_face = topology.face(second_id)?;
            let first_normal = first_plane.normal();
            let second_normal = second_plane.normal();
            if first_normal.cross(second_normal).magnitude() > tolerance.angular {
                continue;
            }
            if validate_section_profile(topology, first_id, tolerance).is_err()
                || validate_section_profile(topology, second_id, tolerance).is_err()
            {
                continue;
            }
            let Some(first_section) =
                integrate_planar_section(topology, first_id, first_plane, tolerance)?
            else {
                continue;
            };
            let Some(second_section) =
                integrate_planar_section(topology, second_id, second_plane, tolerance)?
            else {
                continue;
            };
            if first_section.edge_count != second_section.edge_count
                || face_ids.len() < 3
                || face_ids.len() > first_section.edge_count + 2
            {
                continue;
            }
            let first_jacobian = first_plane.u().cross(first_plane.v()).magnitude();
            let second_jacobian = second_plane.u().cross(second_plane.v()).magnitude();
            let first_area = first_section.area_uv * first_jacobian;
            let second_area = second_section.area_uv * second_jacobian;
            let area_tolerance = tolerance.linear * first_area.abs().sqrt().max(1.0);
            if first_area <= tolerance.linear.powi(2)
                || (first_area - second_area).abs() > area_tolerance
            {
                continue;
            }
            let first_center = section_centroid(first_plane, first_section);
            let second_center = section_centroid(second_plane, second_section);
            let delta = second_center - first_center;
            let height = delta.dot(first_normal).abs();
            if height <= tolerance.linear
                || delta.cross(first_normal).magnitude() > tolerance.linear
            {
                continue;
            }
            let first_effective = match first_face.orientation() {
                super::FaceOrientation::Forward => first_normal,
                super::FaceOrientation::Reversed => -first_normal,
            };
            let second_effective = match second_face.orientation() {
                super::FaceOrientation::Forward => second_normal,
                super::FaceOrientation::Reversed => -second_normal,
            };
            let axis = delta / height;
            if first_effective.dot(axis) > -1.0 + tolerance.angular
                || second_effective.dot(axis) < 1.0 - tolerance.angular
            {
                // Try the same pair in the opposite order.
                if first_effective.dot(axis) < 1.0 - tolerance.angular
                    || second_effective.dot(axis) > -1.0 + tolerance.angular
                {
                    continue;
                }
            }
            let first_edges = face_edges(topology, first_id)?;
            let second_edges = face_edges(topology, second_id)?;
            if !validate_prism_side_incidence(
                topology,
                face_ids,
                first_id,
                second_id,
                &first_edges,
                &second_edges,
                axis * height,
                tolerance,
            )? {
                continue;
            }

            let section = first_section;
            let volume = first_area * height;
            let centroid = first_center + delta * 0.5;
            let covariance = prism_covariance(first_plane, section, axis, height);
            let trace = covariance[0][0] + covariance[1][1] + covariance[2][2];
            let mut inertia = [[0.0; 3]; 3];
            for row in 0..3 {
                for column in 0..3 {
                    inertia[row][column] = volume
                        * (if row == column { trace } else { 0.0 } - covariance[row][column]);
                }
            }
            return Ok(Some(PhysicalCertificate {
                properties: MassProperties {
                    volume,
                    signed_volume: volume,
                    surface_area: 2.0 * first_area + section.perimeter * height,
                    centroid,
                    inertia,
                },
            }));
        }
    }
    Ok(None)
}

fn validate_section_profile(
    topology: &Topology,
    face_id: FaceId,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let face = topology.face(face_id)?;
    let mut outer = None;
    let mut holes = Vec::new();
    for loop_id in face.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let segments = topology
            .loop_coedges(*loop_id)?
            .into_iter()
            .map(|coedge_id| {
                let coedge = topology.coedge(coedge_id)?;
                super::TrimmedCurve2::new(coedge.pcurve().clone(), coedge.range())
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let wire = super::Wire2::new(segments, tolerance)?;
        match loop_value.kind() {
            super::LoopKind::Outer if outer.is_none() => outer = Some(wire),
            super::LoopKind::Outer => {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a planar section has more than one outer loop",
                ));
            }
            super::LoopKind::Inner => holes.push(wire),
        }
    }
    let outer = outer.ok_or_else(|| {
        KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "a planar section has no outer loop",
        )
    })?;
    super::Profile::new(outer, holes, tolerance).map(|_| ())
}

fn integrate_planar_section(
    topology: &Topology,
    face_id: FaceId,
    plane: super::Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PlanarSection>> {
    let face = topology.face(face_id)?;
    let orientation = match face.orientation() {
        super::FaceOrientation::Forward => 1.0,
        super::FaceOrientation::Reversed => -1.0,
    };
    let mut result = PlanarSection {
        area_uv: 0.0,
        first_x: 0.0,
        first_y: 0.0,
        second_xx: 0.0,
        second_yy: 0.0,
        second_xy: 0.0,
        perimeter: 0.0,
        edge_count: 0,
    };
    for loop_id in face.loops() {
        for coedge_id in topology.loop_coedges(*loop_id)? {
            let coedge = topology.coedge(coedge_id)?;
            let range = coedge.range();
            result.area_uv += orientation
                * coedge
                    .pcurve()
                    .signed_area_contribution(range, f64::EPSILON)?;
            let integrals = simpson_curve_integrals(coedge.pcurve(), range, plane, tolerance)?;
            result.first_x += orientation * integrals[0];
            result.first_y += orientation * integrals[1];
            result.second_xx += orientation * integrals[2];
            result.second_yy += orientation * integrals[3];
            result.second_xy += orientation * integrals[4];
            result.perimeter += integrals[5].abs();
            result.edge_count += 1;
        }
    }
    if result.area_uv <= 0.0 {
        return Ok(None);
    }
    Ok(Some(result))
}

fn simpson_curve_integrals(
    curve: &Curve2,
    range: super::ParameterRange,
    plane: super::Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<[f64; 6]> {
    if let Curve2::Nurbs(nurbs) = curve {
        let mut breaks = nurbs
            .knots()
            .iter()
            .copied()
            .filter(|value| {
                *value >= range.start.min(range.end) && *value <= range.start.max(range.end)
            })
            .collect::<Vec<_>>();
        breaks.extend([range.start, range.end]);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup_by(|first, second| (*first - *second).abs() <= f64::EPSILON * 64.0);
        if range.span() < 0.0 {
            breaks.reverse();
        }
        let mut sum = [0.0; 6];
        for interval in breaks.windows(2) {
            if (interval[1] - interval[0]).abs() <= f64::EPSILON * 64.0 {
                continue;
            }
            let values =
                adaptive_curve_integrals(curve, interval[0], interval[1], plane, tolerance)?;
            for component in 0..6 {
                sum[component] += values[component];
            }
        }
        return Ok(sum);
    }
    // Products of circular coordinates and derivatives have low trigonometric
    // degree; this fixed subdivision is substantially below the model tolerance
    // while avoiding a tessellation-dependent volume certificate.
    let intervals = if matches!(curve, Curve2::Circle(_)) {
        256
    } else {
        2
    };
    let step = range.span() / intervals as f64;
    let mut sum = [0.0; 6];
    for index in 0..=intervals {
        let parameter = range.start + step * index as f64;
        let value = curve.evaluate(parameter)?;
        let x = value.point.x;
        let y = value.point.y;
        let dx = value.first_derivative.x;
        let dy = value.first_derivative.y;
        let tangent = plane.u() * dx + plane.v() * dy;
        let terms = [
            0.5 * x * x * dy,
            -0.5 * y * y * dx,
            x * x * x * dy / 3.0,
            -y * y * y * dx / 3.0,
            0.5 * x * x * y * dy,
            tangent.magnitude(),
        ];
        let weight = if index == 0 || index == intervals {
            1.0
        } else if index % 2 == 0 {
            2.0
        } else {
            4.0
        };
        for component in 0..6 {
            sum[component] += weight * terms[component];
        }
    }
    for value in &mut sum {
        *value *= step / 3.0;
    }
    Ok(sum)
}

fn adaptive_curve_integrals(
    curve: &Curve2,
    start: f64,
    end: f64,
    plane: super::Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<[f64; 6]> {
    let terms = |parameter| curve_integral_terms(curve, parameter, plane);
    let middle = (start + end) * 0.5;
    let start_value = terms(start)?;
    let middle_value = terms(middle)?;
    let end_value = terms(end)?;
    let scale = [start_value, middle_value, end_value]
        .iter()
        .flat_map(|values| values.iter().copied())
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let absolute = tolerance
        .linear_at_scale(scale)
        .max(f64::EPSILON * scale * 256.0);
    let targets = [absolute; 6];
    let whole = simpson_values(start, end, start_value, middle_value, end_value);
    adaptive_curve_integrals_interval(
        &terms,
        start,
        end,
        start_value,
        middle_value,
        end_value,
        whole,
        targets,
        0,
        24,
    )
}

#[allow(clippy::too_many_arguments)]
fn adaptive_curve_integrals_interval<F>(
    terms: &F,
    start: f64,
    end: f64,
    start_value: [f64; 6],
    middle_value: [f64; 6],
    end_value: [f64; 6],
    whole: [f64; 6],
    targets: [f64; 6],
    depth: u8,
    maximum_depth: u8,
) -> KernelResult<[f64; 6]>
where
    F: Fn(f64) -> KernelResult<[f64; 6]>,
{
    let middle = (start + end) * 0.5;
    let left_middle = (start + middle) * 0.5;
    let right_middle = (middle + end) * 0.5;
    let left_middle_value = terms(left_middle)?;
    let right_middle_value = terms(right_middle)?;
    let left = simpson_values(start, middle, start_value, left_middle_value, middle_value);
    let right = simpson_values(middle, end, middle_value, right_middle_value, end_value);
    let converged = (0..6).all(|component| {
        (left[component] + right[component] - whole[component]).abs() / 15.0 <= targets[component]
    });
    if converged {
        return Ok(std::array::from_fn(|component| {
            left[component]
                + right[component]
                + (left[component] + right[component] - whole[component]) / 15.0
        }));
    }
    if depth >= maximum_depth {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "certified NURBS boundary integration did not converge",
        ));
    }
    let next_targets = targets.map(|target| target * 0.5);
    let first = adaptive_curve_integrals_interval(
        terms,
        start,
        middle,
        start_value,
        left_middle_value,
        middle_value,
        left,
        next_targets,
        depth + 1,
        maximum_depth,
    )?;
    let second = adaptive_curve_integrals_interval(
        terms,
        middle,
        end,
        middle_value,
        right_middle_value,
        end_value,
        right,
        next_targets,
        depth + 1,
        maximum_depth,
    )?;
    Ok(std::array::from_fn(|component| {
        first[component] + second[component]
    }))
}

fn simpson_values(
    start: f64,
    end: f64,
    first: [f64; 6],
    middle: [f64; 6],
    last: [f64; 6],
) -> [f64; 6] {
    std::array::from_fn(|component| {
        (end - start) * (first[component] + 4.0 * middle[component] + last[component]) / 6.0
    })
}

fn curve_integral_terms(
    curve: &Curve2,
    parameter: f64,
    plane: super::Plane3,
) -> KernelResult<[f64; 6]> {
    let value = curve.evaluate(parameter)?;
    let x = value.point.x;
    let y = value.point.y;
    let dx = value.first_derivative.x;
    let dy = value.first_derivative.y;
    let tangent = plane.u() * dx + plane.v() * dy;
    Ok([
        0.5 * x * x * dy,
        -0.5 * y * y * dx,
        x * x * x * dy / 3.0,
        -y * y * y * dx / 3.0,
        0.5 * x * x * y * dy,
        tangent.magnitude(),
    ])
}

fn section_centroid(plane: super::Plane3, section: PlanarSection) -> Point3 {
    plane.origin()
        + plane.u() * (section.first_x / section.area_uv)
        + plane.v() * (section.first_y / section.area_uv)
}

fn prism_covariance(
    plane: super::Plane3,
    section: PlanarSection,
    axis: Vec3,
    height: f64,
) -> [[f64; 3]; 3] {
    let mean = [
        section.first_x / section.area_uv,
        section.first_y / section.area_uv,
    ];
    let uv = [
        [
            section.second_xx / section.area_uv - mean[0] * mean[0],
            section.second_xy / section.area_uv - mean[0] * mean[1],
        ],
        [
            section.second_xy / section.area_uv - mean[0] * mean[1],
            section.second_yy / section.area_uv - mean[1] * mean[1],
        ],
    ];
    let basis = [plane.u(), plane.v()];
    let mut covariance = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            for first in 0..2 {
                for second in 0..2 {
                    covariance[row][column] +=
                        basis[first][row] * uv[first][second] * basis[second][column];
                }
            }
            covariance[row][column] += axis[row] * axis[column] * height * height / 12.0;
        }
    }
    covariance
}

fn face_edges(topology: &Topology, face_id: FaceId) -> KernelResult<BTreeSet<super::EdgeId>> {
    let mut result = BTreeSet::new();
    for loop_id in topology.face(face_id)?.loops() {
        for coedge_id in topology.loop_coedges(*loop_id)? {
            result.insert(topology.coedge(coedge_id)?.edge());
        }
    }
    Ok(result)
}

fn validate_prism_side_incidence(
    topology: &Topology,
    face_ids: &BTreeSet<FaceId>,
    first_cap: FaceId,
    second_cap: FaceId,
    first_edges: &BTreeSet<super::EdgeId>,
    second_edges: &BTreeSet<super::EdgeId>,
    sweep: Vec3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let mut first_incidence = 0usize;
    let mut second_incidence = 0usize;
    for face_id in face_ids {
        if *face_id == first_cap || *face_id == second_cap {
            continue;
        }
        let face = topology.face(*face_id)?;
        if face.loops().len() != 1 {
            return Ok(false);
        }
        if let Surface3::Nurbs(surface) = face.surface() {
            if surface.degrees().1 != 1 || surface.control_points().len() != 2 {
                return Ok(false);
            }
            let first = &surface.control_points()[0];
            let second = &surface.control_points()[1];
            if first.len() != second.len()
                || surface.weights()[0].len() != surface.weights()[1].len()
                || surface.weights()[0]
                    .iter()
                    .zip(&surface.weights()[1])
                    .any(|(first, second)| {
                        (*first - *second).abs()
                            > f64::EPSILON * first.abs().max(second.abs()).max(1.0) * 256.0
                    })
            {
                return Ok(false);
            }
            let scale = sweep.magnitude().max(1.0);
            let allowed = tolerance.linear_at_scale(scale) * 8.0;
            if first.iter().zip(second).any(|(first, second)| {
                let delta = *second - *first;
                (delta - sweep).magnitude() > allowed && (delta + sweep).magnitude() > allowed
            }) {
                return Ok(false);
            }
        } else if !matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_)) {
            return Ok(false);
        }
        let edges = face_edges(topology, *face_id)?;
        let on_first = edges.intersection(first_edges).count();
        let on_second = edges.intersection(second_edges).count();
        if on_first == 0 || on_second == 0 {
            return Ok(false);
        }
        first_incidence += on_first;
        second_incidence += on_second;
    }
    Ok(first_incidence == first_edges.len() && second_incidence == second_edges.len())
}

fn try_certify_prism_with_bosses(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    let shell_id = body.shells()[0];
    for target_face in face_ids {
        let target = topology.face(*target_face)?;
        if !matches!(target.surface(), Surface3::Plane(_)) || target.loops().len() < 2 {
            continue;
        }
        let inner_loops = target
            .loops()
            .iter()
            .copied()
            .filter(|loop_id| {
                topology
                    .loop_(*loop_id)
                    .map(|loop_value| loop_value.kind() == super::LoopKind::Inner)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if inner_loops.is_empty() {
            continue;
        }

        let mut boss_components = Vec::new();
        let mut boss_faces = BTreeSet::new();
        let mut failed = false;
        for inner_loop in &inner_loops {
            let component = faces_across_loop(topology, *target_face, *inner_loop)?;
            if component.is_empty() || component.iter().any(|face| boss_faces.contains(face)) {
                failed = true;
                break;
            }
            boss_faces.extend(component.iter().copied());
            boss_components.push((*inner_loop, component));
        }
        if failed || boss_faces.iter().any(|face| !face_ids.contains(face)) {
            continue;
        }
        let Surface3::Plane(target_plane) = target.surface() else {
            unreachable!()
        };
        let target_normal = match target.orientation() {
            super::FaceOrientation::Forward => target_plane.normal(),
            super::FaceOrientation::Reversed => -target_plane.normal(),
        };
        if boss_components.iter().any(|(_, faces)| {
            component_centroid(topology, faces)
                .map(|centroid| {
                    (centroid - target_plane.origin()).dot(target_normal) <= tolerance.linear
                })
                .unwrap_or(true)
        }) {
            continue;
        }

        let mut base_topology = topology.clone();
        base_topology
            .shells
            .get_mut(shell_id)?
            .faces
            .retain(|face| !boss_faces.contains(face));
        base_topology
            .faces
            .get_mut(*target_face)?
            .loops
            .retain(|loop_id| !inner_loops.contains(loop_id));
        let base_faces = face_ids
            .iter()
            .copied()
            .filter(|face| !boss_faces.contains(face))
            .collect::<BTreeSet<_>>();
        let Some(base_certificate) =
            try_certify_right_prism(&base_topology, body_id, &base_faces, tolerance)?
        else {
            continue;
        };

        let mut components = vec![base_certificate.properties];
        let mut shared_area = 0.0;
        for (inner_loop, component_faces) in boss_components {
            let segments = topology
                .loop_coedges(inner_loop)?
                .into_iter()
                .map(|coedge_id| {
                    let coedge = topology.coedge(coedge_id)?;
                    super::TrimmedCurve2::new(coedge.pcurve().clone(), coedge.range())
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let shared_profile = super::Wire2::new(segments, tolerance)?;
            let mut boss_topology = topology.clone();
            let mut base_face = boss_topology.face(*target_face)?.clone();
            base_face.orientation = match base_face.orientation {
                super::FaceOrientation::Forward => super::FaceOrientation::Reversed,
                super::FaceOrientation::Reversed => super::FaceOrientation::Forward,
            };
            base_face.loops = vec![inner_loop];
            let base_face_id = boss_topology.faces.insert(base_face);
            {
                let loop_value = boss_topology.loops.get_mut(inner_loop)?;
                loop_value.face = base_face_id;
                loop_value.kind = super::LoopKind::Outer;
            }
            let mut component_face_ids = component_faces.clone();
            component_face_ids.insert(base_face_id);
            boss_topology.shells.get_mut(shell_id)?.faces =
                component_face_ids.iter().copied().collect();
            let Some(certificate) =
                try_certify_right_prism(&boss_topology, body_id, &component_face_ids, tolerance)?
            else {
                failed = true;
                break;
            };
            shared_area += shared_profile.area();
            components.push(certificate.properties);
        }
        if failed {
            continue;
        }
        return Ok(Some(PhysicalCertificate {
            properties: combine_disjoint_properties(&components, shared_area * 2.0),
        }));
    }
    Ok(None)
}

fn try_certify_prism_with_pockets(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    let shell_id = body.shells()[0];
    for opening_face in face_ids {
        let opening = topology.face(*opening_face)?;
        if !matches!(opening.surface(), Surface3::Plane(_)) || opening.loops().len() < 2 {
            continue;
        }
        let inner_loops = opening
            .loops()
            .iter()
            .copied()
            .filter(|loop_id| {
                topology
                    .loop_(*loop_id)
                    .map(|loop_value| loop_value.kind() == super::LoopKind::Inner)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if inner_loops.is_empty() {
            continue;
        }

        let mut pocket_components = Vec::new();
        let mut pocket_faces = BTreeSet::new();
        let mut failed = false;
        for inner_loop in &inner_loops {
            let component = faces_across_loop(topology, *opening_face, *inner_loop)?;
            if component.is_empty() || component.iter().any(|face| pocket_faces.contains(face)) {
                failed = true;
                break;
            }
            pocket_faces.extend(component.iter().copied());
            pocket_components.push((*inner_loop, component));
        }
        if failed || pocket_faces.iter().any(|face| !face_ids.contains(face)) {
            continue;
        }
        let Surface3::Plane(opening_plane) = opening.surface() else {
            unreachable!()
        };
        let opening_normal = match opening.orientation() {
            super::FaceOrientation::Forward => opening_plane.normal(),
            super::FaceOrientation::Reversed => -opening_plane.normal(),
        };
        if pocket_components.iter().any(|(_, faces)| {
            component_centroid(topology, faces)
                .map(|centroid| {
                    (centroid - opening_plane.origin()).dot(opening_normal) >= -tolerance.linear
                })
                .unwrap_or(true)
        }) {
            continue;
        }

        let mut base_topology = topology.clone();
        base_topology
            .shells
            .get_mut(shell_id)?
            .faces
            .retain(|face| !pocket_faces.contains(face));
        base_topology
            .faces
            .get_mut(*opening_face)?
            .loops
            .retain(|loop_id| !inner_loops.contains(loop_id));
        let base_faces = face_ids
            .iter()
            .copied()
            .filter(|face| !pocket_faces.contains(face))
            .collect::<BTreeSet<_>>();
        let Some(base_certificate) =
            try_certify_right_prism(&base_topology, body_id, &base_faces, tolerance)?
        else {
            continue;
        };

        let mut voids = Vec::new();
        let mut lateral_area = 0.0;
        for (inner_loop, component_faces) in pocket_components {
            let segments = topology
                .loop_coedges(inner_loop)?
                .into_iter()
                .map(|coedge_id| {
                    let coedge = topology.coedge(coedge_id)?;
                    super::TrimmedCurve2::new(coedge.pcurve().clone(), coedge.range())
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let profile =
                super::Profile::new(super::Wire2::new(segments, tolerance)?, [], tolerance)?;
            let mut component_vertices = BTreeSet::new();
            for face in &component_faces {
                for edge in face_edges(topology, *face)? {
                    component_vertices.extend(topology.edge(edge)?.vertices());
                }
            }
            let depth = component_vertices
                .into_iter()
                .map(|vertex| {
                    topology
                        .vertex(vertex)
                        .map(|vertex| (vertex.point() - opening_plane.origin()).dot(opening_normal))
                })
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .fold(0.0_f64, f64::min);
            let height = -depth;
            if height <= tolerance.linear {
                failed = true;
                break;
            }
            let void = profile.extrude(*opening_plane, opening_normal * depth)?;
            if void.solids().len() != 1 {
                failed = true;
                break;
            }
            let properties = void.mass_properties();
            let cap_area = profile.area();
            lateral_area += properties.surface_area - 2.0 * cap_area;
            voids.push(properties);
        }
        if failed {
            continue;
        }
        return Ok(Some(PhysicalCertificate {
            properties: subtract_properties(base_certificate.properties, &voids, lateral_area)?,
        }));
    }
    Ok(None)
}

fn component_centroid(topology: &Topology, faces: &BTreeSet<FaceId>) -> KernelResult<Point3> {
    let mut vertices = BTreeSet::new();
    for face in faces {
        for edge in face_edges(topology, *face)? {
            vertices.extend(topology.edge(edge)?.vertices());
        }
    }
    if vertices.is_empty() {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "a connected face component has no vertices",
        ));
    }
    let count = vertices.len() as f64;
    Ok(Point3::from_vec(
        vertices
            .into_iter()
            .try_fold(Vec3::new(0.0, 0.0, 0.0), |sum, vertex| {
                Ok::<_, KernelError>(sum + topology.vertex(vertex)?.point().to_vec())
            })?
            / count,
    ))
}

fn subtract_properties(
    base: MassProperties,
    voids: &[MassProperties],
    added_surface_area: f64,
) -> KernelResult<MassProperties> {
    let removed_volume = voids
        .iter()
        .map(|properties| properties.volume)
        .sum::<f64>();
    let volume = base.volume - removed_volume;
    if volume <= 0.0 {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "pockets remove the entire certified base volume",
        ));
    }
    let first_moment = voids.iter().fold(
        base.centroid.to_vec() * base.volume,
        |moment, properties| moment - properties.centroid.to_vec() * properties.volume,
    );
    let centroid = Point3::from_vec(first_moment / volume);
    let origin_inertia = |properties: MassProperties| {
        let center = properties.centroid.to_vec();
        let distance2 = center.magnitude2();
        std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                properties.inertia[row][column]
                    + properties.volume
                        * (if row == column { distance2 } else { 0.0 }
                            - center[row] * center[column])
            })
        })
    };
    let mut inertia = origin_inertia(base);
    for properties in voids {
        let removed = origin_inertia(*properties);
        for row in 0..3 {
            for column in 0..3 {
                inertia[row][column] -= removed[row][column];
            }
        }
    }
    let center = centroid.to_vec();
    let distance2 = center.magnitude2();
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] -= volume
                * (if row == column { distance2 } else { 0.0 } - center[row] * center[column]);
        }
    }
    Ok(MassProperties {
        volume,
        signed_volume: volume,
        surface_area: base.surface_area + added_surface_area,
        centroid,
        inertia,
    })
}

fn faces_across_loop(
    topology: &Topology,
    excluded_face: FaceId,
    loop_id: super::LoopId,
) -> KernelResult<BTreeSet<FaceId>> {
    let mut queue = VecDeque::new();
    for coedge_id in topology.loop_coedges(loop_id)? {
        let edge = topology.edge(topology.coedge(coedge_id)?.edge())?;
        for mate in edge.coedges() {
            let face = topology.loop_(topology.coedge(*mate)?.loop_id())?.face();
            if face != excluded_face {
                queue.push_back(face);
            }
        }
    }
    let mut faces = BTreeSet::new();
    while let Some(face) = queue.pop_front() {
        if face == excluded_face || !faces.insert(face) {
            continue;
        }
        for loop_id in topology.face(face)?.loops() {
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let edge = topology.edge(topology.coedge(coedge_id)?.edge())?;
                for mate in edge.coedges() {
                    let adjacent = topology.loop_(topology.coedge(*mate)?.loop_id())?.face();
                    if adjacent != excluded_face && !faces.contains(&adjacent) {
                        queue.push_back(adjacent);
                    }
                }
            }
        }
    }
    Ok(faces)
}

fn combine_disjoint_properties(
    components: &[MassProperties],
    removed_surface_area: f64,
) -> MassProperties {
    let volume = components
        .iter()
        .map(|properties| properties.volume)
        .sum::<f64>();
    let centroid = Point3::from_vec(
        components
            .iter()
            .fold(Vec3::new(0.0, 0.0, 0.0), |sum, properties| {
                sum + properties.centroid.to_vec() * properties.volume
            })
            / volume,
    );
    let mut inertia = [[0.0; 3]; 3];
    for properties in components {
        let offset = properties.centroid - centroid;
        let distance2 = offset.magnitude2();
        for row in 0..3 {
            for column in 0..3 {
                inertia[row][column] += properties.inertia[row][column]
                    + properties.volume
                        * (if row == column { distance2 } else { 0.0 }
                            - offset[row] * offset[column]);
            }
        }
    }
    MassProperties {
        volume,
        signed_volume: volume,
        surface_area: components
            .iter()
            .map(|properties| properties.surface_area)
            .sum::<f64>()
            - removed_surface_area,
        centroid,
        inertia,
    }
}

fn try_certify_torus(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    edge_ids: &BTreeSet<super::EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    if face_ids.len() != 2 || edge_ids.len() != 6 {
        return Ok(None);
    }
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    validate_shell_connected(topology, body.shells()[0])?;
    let toruses: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Torus(torus) => Some((*face_id, *torus)),
            _ => None,
        })
        .collect();
    if toruses.len() != 2 {
        return Ok(None);
    }
    let reference = toruses[0].1;
    let major = reference.major_radius();
    let minor = reference.minor_radius();
    let linear = tolerance.linear;
    for (face_id, torus) in &toruses {
        let face = topology.face(*face_id)?;
        if face.orientation() != super::FaceOrientation::Forward
            || torus.center().distance(reference.center()) > linear
            || (torus.x_major() - reference.x_major()).magnitude() > linear
            || (torus.y_major() - reference.y_major()).magnitude() > linear
            || (torus.minor_radius() - minor).abs() > linear
            || face.loops().len() != 1
        {
            return Ok(None);
        }
        for coedge_id in topology.loop_coedges(face.loops()[0])? {
            if !matches!(topology.coedge(coedge_id)?.pcurve(), Curve2::Line(_)) {
                return Ok(None);
            }
        }
    }
    let mut torus_bounds = toruses
        .iter()
        .map(|(face_id, _)| face_uv_bounds(topology, *face_id))
        .collect::<KernelResult<Vec<_>>>()?;
    torus_bounds.sort_by(|first, second| first[2].total_cmp(&second[2]));
    if torus_bounds.len() != 2
        || !near(torus_bounds[0][0], 0.0, tolerance.parameter)
        || !near(
            torus_bounds[0][1],
            std::f64::consts::TAU,
            tolerance.parameter,
        )
        || !near(torus_bounds[0][2], 0.0, tolerance.parameter)
        || !near(
            torus_bounds[0][3],
            std::f64::consts::PI,
            tolerance.parameter,
        )
        || !near(torus_bounds[1][0], 0.0, tolerance.parameter)
        || !near(
            torus_bounds[1][1],
            std::f64::consts::TAU,
            tolerance.parameter,
        )
        || !near(
            torus_bounds[1][2],
            std::f64::consts::PI,
            tolerance.parameter,
        )
        || !near(
            torus_bounds[1][3],
            std::f64::consts::TAU,
            tolerance.parameter,
        )
    {
        return Ok(None);
    }
    let mut outer_arcs = 0usize;
    let mut inner_arcs = 0usize;
    let mut meridians = 0usize;
    let tube_center = reference.center() + reference.x_major();
    let mut vertices = BTreeSet::new();
    for edge_id in edge_ids {
        let edge = topology.edge(*edge_id)?;
        vertices.extend(edge.vertices());
        let Curve3::Circle(circle) = edge.curve() else {
            return Ok(None);
        };
        if (edge.range().span().abs() - std::f64::consts::PI).abs() > tolerance.parameter {
            return Ok(None);
        }
        if circle.center().distance(reference.center()) <= linear {
            if (circle.radius() - (major + minor)).abs() <= linear {
                outer_arcs += 1;
            } else if (circle.radius() - (major - minor)).abs() <= linear {
                inner_arcs += 1;
            } else {
                return Ok(None);
            }
        } else if circle.center().distance(tube_center) <= linear
            && (circle.radius() - minor).abs() <= linear
        {
            meridians += 1;
        } else {
            return Ok(None);
        }
    }
    if outer_arcs != 2 || inner_arcs != 2 || meridians != 2 || vertices.len() != 4 {
        return Ok(None);
    }

    let volume = 2.0 * std::f64::consts::PI.powi(2) * major * minor.powi(2);
    let axial_inertia = volume * (major * major + 0.75 * minor * minor);
    let transverse_inertia = volume * (0.5 * major * major + 0.625 * minor * minor);
    let normal = reference.normal();
    let n = [normal.x, normal.y, normal.z];
    let mut inertia = [[0.0_f64; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] = if row == column {
                transverse_inertia
            } else {
                0.0
            } + (axial_inertia - transverse_inertia) * n[row] * n[column];
        }
    }
    Ok(Some(PhysicalCertificate {
        properties: MassProperties {
            volume,
            signed_volume: volume,
            surface_area: 4.0 * std::f64::consts::PI.powi(2) * major * minor,
            centroid: reference.center(),
            inertia,
        },
    }))
}

fn try_certify_cone(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    edge_ids: &BTreeSet<super::EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    if face_ids.len() != 3 || edge_ids.len() != 4 {
        return Ok(None);
    }
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    validate_shell_connected(topology, body.shells()[0])?;
    let cones: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Cone(cone) => Some((*face_id, *cone)),
            _ => None,
        })
        .collect();
    let planes: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Plane(plane) => Some((*face_id, *plane)),
            _ => None,
        })
        .collect();
    if cones.len() != 2 || planes.len() != 1 {
        return Ok(None);
    }
    let reference = cones[0].1;
    let radius = reference.reference_radius();
    let axis = reference.axis();
    let height = axis.magnitude();
    let linear = tolerance.linear;
    if radius <= linear || height <= linear {
        return Ok(None);
    }
    for (face_id, cone) in &cones {
        let face = topology.face(*face_id)?;
        if face.orientation() != super::FaceOrientation::Forward
            || cone.apex().distance(reference.apex()) > linear
            || (cone.axis() - axis).magnitude() > linear
            || (cone.x_radius() - reference.x_radius()).magnitude() > linear
            || (cone.y_radius() - reference.y_radius()).magnitude() > linear
            || face.loops().len() != 1
            || topology.loop_coedges(face.loops()[0])?.len() != 3
        {
            return Ok(None);
        }
        for coedge_id in topology.loop_coedges(face.loops()[0])? {
            if !matches!(topology.coedge(coedge_id)?.pcurve(), Curve2::Line(_)) {
                return Ok(None);
            }
        }
    }
    let cone_intervals = cones
        .iter()
        .map(|(face_id, _)| face_uv_bounds(topology, *face_id))
        .collect::<KernelResult<Vec<_>>>()?;
    if cone_intervals.iter().any(|bounds| {
        !near(bounds[2], 0.0, tolerance.parameter) || !near(bounds[3], 1.0, tolerance.parameter)
    }) || !covers_period(&cone_intervals, tolerance.parameter)
    {
        return Ok(None);
    }
    let (cap_id, plane) = planes[0];
    let base_center = reference.apex() + axis;
    let cap = topology.face(cap_id)?;
    let cap_normal = match cap.orientation() {
        super::FaceOrientation::Forward => plane.normal(),
        super::FaceOrientation::Reversed => -plane.normal(),
    };
    if plane.origin().distance(base_center) > linear
        || cap_normal.dot(axis.normalize()) < 1.0 - tolerance.angular
        || cap.loops().len() != 1
        || topology.loop_coedges(cap.loops()[0])?.len() != 2
    {
        return Ok(None);
    }

    let slant = (height * height + radius * radius).sqrt();
    let mut circular_edges = 0usize;
    let mut generators = 0usize;
    let mut vertices = BTreeSet::new();
    for edge_id in edge_ids {
        let edge = topology.edge(*edge_id)?;
        vertices.extend(edge.vertices());
        match edge.curve() {
            Curve3::Circle(circle) => {
                circular_edges += 1;
                if circle.center().distance(base_center) > linear
                    || (circle.radius() - radius).abs() > linear
                    || (edge.range().span().abs() - std::f64::consts::PI).abs()
                        > tolerance.parameter
                {
                    return Ok(None);
                }
            }
            Curve3::Line(_) => {
                generators += 1;
                let endpoints = edge.vertices();
                let length = topology
                    .vertex(endpoints[0])?
                    .point()
                    .distance(topology.vertex(endpoints[1])?.point());
                if (length - slant).abs() > linear {
                    return Ok(None);
                }
            }
            Curve3::Nurbs(_) => return Ok(None),
        }
    }
    if circular_edges != 2 || generators != 2 || vertices.len() != 3 {
        return Ok(None);
    }
    let apex_found = vertices.iter().try_fold(false, |found, vertex| {
        Ok::<bool, KernelError>(
            found || topology.vertex(*vertex)?.point().distance(reference.apex()) <= linear,
        )
    })?;
    if !apex_found {
        return Ok(None);
    }

    let volume = std::f64::consts::PI * radius * radius * height / 3.0;
    let axial_inertia = 3.0 * volume * radius * radius / 10.0;
    let transverse_inertia = 3.0 * volume * (4.0 * radius * radius + height * height) / 80.0;
    let axis_unit = axis / height;
    let n = [axis_unit.x, axis_unit.y, axis_unit.z];
    let mut inertia = [[0.0_f64; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] = if row == column {
                transverse_inertia
            } else {
                0.0
            } + (axial_inertia - transverse_inertia) * n[row] * n[column];
        }
    }
    Ok(Some(PhysicalCertificate {
        properties: MassProperties {
            volume,
            signed_volume: volume,
            surface_area: std::f64::consts::PI * radius * (radius + slant),
            centroid: reference.apex() + axis * 0.75,
            inertia,
        },
    }))
}

fn try_certify_sphere(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    edge_ids: &BTreeSet<super::EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    if face_ids.len() != 4 || edge_ids.len() != 4 {
        return Ok(None);
    }
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    validate_shell_connected(topology, body.shells()[0])?;
    let spheres: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Sphere(sphere) => Some((*face_id, *sphere)),
            _ => None,
        })
        .collect();
    if spheres.len() != 4 {
        return Ok(None);
    }
    let reference = spheres[0].1;
    let radius = reference.radius();
    let linear = tolerance.linear;
    if radius <= linear {
        return Ok(None);
    }
    for (face_id, sphere) in &spheres {
        let face = topology.face(*face_id)?;
        if face.orientation() != super::FaceOrientation::Forward
            || sphere.center().distance(reference.center()) > linear
            || (sphere.x_radius() - reference.x_radius()).magnitude() > linear
            || (sphere.y_radius() - reference.y_radius()).magnitude() > linear
            || (sphere.z_radius() - reference.z_radius()).magnitude() > linear
        {
            return Ok(None);
        }
        if face.loops().len() != 1 || topology.loop_coedges(face.loops()[0])?.len() != 2 {
            return Ok(None);
        }
        for coedge_id in topology.loop_coedges(face.loops()[0])? {
            if !matches!(topology.coedge(coedge_id)?.pcurve(), Curve2::Line(_)) {
                return Ok(None);
            }
        }
    }
    let sphere_intervals = spheres
        .iter()
        .map(|(face_id, _)| face_uv_bounds(topology, *face_id))
        .collect::<KernelResult<Vec<_>>>()?;
    if sphere_intervals.iter().any(|bounds| {
        !near(bounds[2], -std::f64::consts::FRAC_PI_2, tolerance.parameter)
            || !near(bounds[3], std::f64::consts::FRAC_PI_2, tolerance.parameter)
    }) || !covers_period(&sphere_intervals, tolerance.parameter)
    {
        return Ok(None);
    }

    let mut poles = BTreeSet::new();
    for edge_id in edge_ids {
        let edge = topology.edge(*edge_id)?;
        let Curve3::Circle(circle) = edge.curve() else {
            return Ok(None);
        };
        if circle.center().distance(reference.center()) > linear
            || (circle.radius() - radius).abs() > linear
            || (edge.range().span().abs() - std::f64::consts::PI).abs() > tolerance.parameter
            || circle.normal().dot(reference.z_radius().normalize()).abs() > tolerance.angular
        {
            return Ok(None);
        }
        poles.extend(edge.vertices());
    }
    if poles.len() != 2 {
        return Ok(None);
    }
    let pole_points: Vec<_> = poles
        .iter()
        .map(|vertex| topology.vertex(*vertex).map(|value| value.point()))
        .collect::<KernelResult<_>>()?;
    let center = reference.center();
    if ((pole_points[0] - center).magnitude() - radius).abs() > linear
        || ((pole_points[1] - center).magnitude() - radius).abs() > linear
        || (pole_points[0] - center).dot(pole_points[1] - center)
            > -radius * radius + linear * radius
    {
        return Ok(None);
    }

    let volume = 4.0 * std::f64::consts::PI * radius.powi(3) / 3.0;
    let moment = 2.0 * volume * radius * radius / 5.0;
    Ok(Some(PhysicalCertificate {
        properties: MassProperties {
            volume,
            signed_volume: volume,
            surface_area: 4.0 * std::f64::consts::PI * radius * radius,
            centroid: center,
            inertia: [[moment, 0.0, 0.0], [0.0, moment, 0.0], [0.0, 0.0, moment]],
        },
    }))
}

fn try_certify_cylinder(
    topology: &Topology,
    body_id: BodyId,
    face_ids: &BTreeSet<FaceId>,
    edge_ids: &BTreeSet<super::EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<Option<PhysicalCertificate>> {
    if face_ids.len() != 4 || edge_ids.len() != 6 {
        return Ok(None);
    }
    let body = topology.body(body_id)?;
    if body.shells().len() != 1 {
        return Ok(None);
    }
    validate_shell_connected(topology, body.shells()[0])?;
    let cylinders: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Cylinder(cylinder) => Some((*face_id, *cylinder)),
            _ => None,
        })
        .collect();
    let planes: Vec<_> = face_ids
        .iter()
        .filter_map(|face_id| match topology.face(*face_id).ok()?.surface() {
            Surface3::Plane(plane) => Some((*face_id, *plane)),
            _ => None,
        })
        .collect();
    if cylinders.len() != 2 || planes.len() != 2 {
        return Ok(None);
    }
    let reference = cylinders[0].1;
    let radius = reference.radius();
    let axis = reference.axis();
    let height = axis.magnitude();
    let linear = tolerance.linear;
    if height <= linear || radius <= linear {
        return Ok(None);
    }
    for (_, cylinder) in &cylinders {
        if cylinder.origin().distance(reference.origin()) > linear
            || (cylinder.axis() - axis).magnitude() > linear
            || (cylinder.x_radius() - reference.x_radius()).magnitude() > linear
            || (cylinder.y_radius() - reference.y_radius()).magnitude() > linear
        {
            return Ok(None);
        }
    }
    let start = reference.origin();
    let end = start + axis;
    let axis_unit = axis / height;
    let mut found_start = false;
    let mut found_end = false;
    for (face_id, plane) in &planes {
        let face = topology.face(*face_id)?;
        let effective_normal = match face.orientation() {
            super::FaceOrientation::Forward => plane.normal(),
            super::FaceOrientation::Reversed => -plane.normal(),
        };
        if plane.origin().distance(start) <= linear {
            found_start = true;
            if effective_normal.dot(-axis_unit) < 1.0 - tolerance.angular {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "cylinder start cap is not outward-oriented",
                ));
            }
        } else if plane.origin().distance(end) <= linear {
            found_end = true;
            if effective_normal.dot(axis_unit) < 1.0 - tolerance.angular {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "cylinder end cap is not outward-oriented",
                ));
            }
        } else {
            return Ok(None);
        }
        for loop_id in face.loops() {
            for coedge_id in topology.loop_coedges(*loop_id)? {
                if !matches!(topology.coedge(coedge_id)?.pcurve(), Curve2::Circle(_)) {
                    return Ok(None);
                }
            }
        }
    }
    if !found_start || !found_end {
        return Ok(None);
    }
    for (face_id, cylinder) in &cylinders {
        let face = topology.face(*face_id)?;
        let evaluation = cylinder.evaluate(0.0, 0.5)?;
        let mut normal = evaluation.normal()?;
        if face.orientation() == super::FaceOrientation::Reversed {
            normal = -normal;
        }
        if normal.dot(reference.x_radius().normalize()) < 1.0 - tolerance.angular {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "cylinder wall is not outward-oriented",
            ));
        }
        for loop_id in face.loops() {
            for coedge_id in topology.loop_coedges(*loop_id)? {
                if !matches!(topology.coedge(coedge_id)?.pcurve(), Curve2::Line(_)) {
                    return Ok(None);
                }
            }
        }
    }
    let cylinder_intervals = cylinders
        .iter()
        .map(|(face_id, _)| face_uv_bounds(topology, *face_id))
        .collect::<KernelResult<Vec<_>>>()?;
    if cylinder_intervals.iter().any(|bounds| {
        !near(bounds[2], 0.0, tolerance.parameter) || !near(bounds[3], 1.0, tolerance.parameter)
    }) || !covers_period(&cylinder_intervals, tolerance.parameter)
    {
        return Ok(None);
    }
    let mut circular_edges = 0usize;
    let mut linear_edges = 0usize;
    for edge_id in edge_ids {
        let edge = topology.edge(*edge_id)?;
        match edge.curve() {
            Curve3::Circle(circle) => {
                circular_edges += 1;
                let expected_center = if circle.center().distance(start) <= linear {
                    start
                } else if circle.center().distance(end) <= linear {
                    end
                } else {
                    return Ok(None);
                };
                if circle.center().distance(expected_center) > linear
                    || (circle.radius() - radius).abs() > linear
                    || (edge.range().span().abs() - std::f64::consts::PI).abs()
                        > tolerance.parameter
                {
                    return Ok(None);
                }
            }
            Curve3::Line(_) => {
                linear_edges += 1;
                let vertices = edge.vertices();
                let delta =
                    topology.vertex(vertices[1])?.point() - topology.vertex(vertices[0])?.point();
                if delta.angle(axis).0.min(delta.angle(-axis).0) > tolerance.angular
                    || (delta.magnitude() - height).abs() > linear
                {
                    return Ok(None);
                }
            }
            Curve3::Nurbs(_) => return Ok(None),
        }
    }
    if circular_edges != 4 || linear_edges != 2 {
        return Ok(None);
    }

    let volume = std::f64::consts::PI * radius * radius * height;
    let centroid = start + axis * 0.5;
    let axial_inertia = 0.5 * volume * radius * radius;
    let transverse_inertia = volume * (3.0 * radius * radius + height * height) / 12.0;
    let n = [axis_unit.x, axis_unit.y, axis_unit.z];
    let mut inertia = [[0.0_f64; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] = if row == column {
                transverse_inertia
            } else {
                0.0
            } + (axial_inertia - transverse_inertia) * n[row] * n[column];
        }
    }
    Ok(Some(PhysicalCertificate {
        properties: MassProperties {
            volume,
            signed_volume: volume,
            surface_area: 2.0 * std::f64::consts::PI * radius * (radius + height),
            centroid,
            inertia,
        },
    }))
}

fn face_uv_bounds(topology: &Topology, face_id: FaceId) -> KernelResult<[f64; 4]> {
    let face = topology.face(face_id)?;
    if face.loops().len() != 1 {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "analytic primitive patch must have exactly one trim loop",
        ));
    }
    let mut bounds = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    for coedge_id in topology.loop_coedges(face.loops()[0])? {
        let coedge = topology.coedge(coedge_id)?;
        for parameter in [coedge.range().start, coedge.range().end] {
            let point = coedge.pcurve().evaluate(parameter)?.point;
            bounds[0] = bounds[0].min(point.x);
            bounds[1] = bounds[1].max(point.x);
            bounds[2] = bounds[2].min(point.y);
            bounds[3] = bounds[3].max(point.y);
        }
    }
    if bounds.iter().any(|value| !value.is_finite()) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "analytic primitive trim bounds are not finite",
        ));
    }
    Ok(bounds)
}

fn covers_period(bounds: &[[f64; 4]], tolerance: f64) -> bool {
    if bounds.is_empty() {
        return false;
    }
    let mut intervals: Vec<_> = bounds.iter().map(|value| [value[0], value[1]]).collect();
    intervals.sort_by(|first, second| first[0].total_cmp(&second[0]));
    near(intervals[0][0], 0.0, tolerance)
        && near(
            intervals[intervals.len() - 1][1],
            std::f64::consts::TAU,
            tolerance,
        )
        && intervals
            .windows(2)
            .all(|pair| near(pair[0][1], pair[1][0], tolerance))
}

fn near(first: f64, second: f64, tolerance: f64) -> bool {
    (first - second).abs() <= tolerance
}

fn validate_planar_trim(
    topology: &Topology,
    face_id: FaceId,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let face = topology.face(face_id)?;
    let Surface3::Plane(plane) = face.surface() else {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "planar trim validation requires a plane",
        ));
    };
    let e = plane.u().magnitude2();
    let f = plane.u().dot(plane.v());
    let g = plane.v().magnitude2();
    let minimum_metric = ((e + g - ((e - g).powi(2) + 4.0 * f * f).sqrt()) * 0.5)
        .max(f64::MIN_POSITIVE)
        .sqrt();
    let uv_tolerance = tolerance.parameter.max(tolerance.linear / minimum_metric);
    let mut rings = Vec::<(super::LoopKind, Vec<Point2>)>::new();
    for loop_id in face.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let first = loop_value.first().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "physical face contains an empty loop",
            )
        })?;
        let mut points = Vec::new();
        let mut current = first;
        loop {
            let coedge = topology.coedge(current)?;
            points.push(coedge.pcurve().evaluate(coedge.range().start)?.point);
            current = coedge.next();
            if current == first {
                break;
            }
        }
        if points.len() < 3 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "physical face loop needs at least three edges",
            ));
        }
        validate_simple_polygon(&points, uv_tolerance)?;
        rings.push((loop_value.kind(), points));
    }
    rings.sort_by_key(|(kind, _)| match kind {
        super::LoopKind::Outer => 0,
        super::LoopKind::Inner => 1,
    });
    let Some((super::LoopKind::Outer, outer)) = rings.first() else {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "physical face has no outer loop",
        ));
    };
    for (_, inner) in rings.iter().skip(1) {
        if polygons_touch_or_cross(outer, inner, uv_tolerance)
            || !point_in_polygon2(inner[0], outer, uv_tolerance)
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "face hole must lie strictly inside its outer loop",
            ));
        }
    }
    for first in 1..rings.len() {
        for second in first + 1..rings.len() {
            if polygons_touch_or_cross(&rings[first].1, &rings[second].1, uv_tolerance)
                || point_in_polygon2(rings[first].1[0], &rings[second].1, uv_tolerance)
                || point_in_polygon2(rings[second].1[0], &rings[first].1, uv_tolerance)
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "face holes may not touch, overlap, or contain one another",
                ));
            }
        }
    }
    Ok(())
}

fn validate_simple_polygon(points: &[Point2], tolerance: f64) -> KernelResult<()> {
    for first in 0..points.len() {
        let first_next = (first + 1) % points.len();
        if points[first].distance(points[first_next]) <= tolerance {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "face loop contains a zero-length UV edge",
            ));
        }
        for second in first + 1..points.len() {
            let second_next = (second + 1) % points.len();
            let adjacent = first == second || first_next == second || second_next == first;
            if adjacent {
                continue;
            }
            if segments_intersect2(
                points[first],
                points[first_next],
                points[second],
                points[second_next],
                tolerance,
            ) {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "face loop self-intersects in UV space",
                ));
            }
        }
    }
    Ok(())
}

fn polygons_touch_or_cross(first: &[Point2], second: &[Point2], tolerance: f64) -> bool {
    (0..first.len()).any(|a| {
        (0..second.len()).any(|b| {
            segments_intersect2(
                first[a],
                first[(a + 1) % first.len()],
                second[b],
                second[(b + 1) % second.len()],
                tolerance,
            )
        })
    })
}

fn orient_point2(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_segment2(point: Point2, start: Point2, end: Point2, tolerance: f64) -> bool {
    orient_point2(start, end, point).abs() <= tolerance * tolerance
        && point.x >= start.x.min(end.x) - tolerance
        && point.x <= start.x.max(end.x) + tolerance
        && point.y >= start.y.min(end.y) - tolerance
        && point.y <= start.y.max(end.y) + tolerance
}

fn segments_intersect2(a: Point2, b: Point2, c: Point2, d: Point2, tolerance: f64) -> bool {
    let values = [
        orient_point2(a, b, c),
        orient_point2(a, b, d),
        orient_point2(c, d, a),
        orient_point2(c, d, b),
    ];
    let area_tolerance = tolerance * tolerance;
    if values[0] * values[1] < -area_tolerance * area_tolerance
        && values[2] * values[3] < -area_tolerance * area_tolerance
    {
        return true;
    }
    (values[0].abs() <= area_tolerance && on_segment2(c, a, b, tolerance))
        || (values[1].abs() <= area_tolerance && on_segment2(d, a, b, tolerance))
        || (values[2].abs() <= area_tolerance && on_segment2(a, c, d, tolerance))
        || (values[3].abs() <= area_tolerance && on_segment2(b, c, d, tolerance))
}

fn point_in_polygon2(point: Point2, polygon: &[Point2], tolerance: f64) -> bool {
    if (0..polygon.len()).any(|index| {
        on_segment2(
            point,
            polygon[index],
            polygon[(index + 1) % polygon.len()],
            tolerance,
        )
    }) {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        if (start.y > point.y) != (end.y > point.y) {
            let x = start.x + (point.y - start.y) * (end.x - start.x) / (end.y - start.y);
            if x > point.x {
                inside = !inside;
            }
        }
    }
    inside
}

fn validate_shell_connected(topology: &Topology, shell_id: ShellId) -> KernelResult<()> {
    let shell = topology.shell(shell_id)?;
    let faces: BTreeSet<_> = shell.faces().iter().copied().collect();
    let Some(start) = shell.faces().first().copied() else {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "shell has no faces",
        ));
    };
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(face_id) = queue.pop_front() {
        if !visited.insert(face_id) {
            continue;
        }
        for (_, edge) in topology.edges() {
            let incident: Vec<_> = edge
                .coedges()
                .iter()
                .filter_map(|coedge_id| {
                    topology
                        .coedge(*coedge_id)
                        .ok()
                        .and_then(|coedge| topology.loop_(coedge.loop_id()).ok())
                        .map(|loop_| loop_.face())
                })
                .collect();
            if incident.contains(&face_id) {
                for neighbor in incident {
                    if faces.contains(&neighbor) && !visited.contains(&neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }
    if visited != faces {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "all faces of a physical shell must be edge-connected",
        ));
    }
    Ok(())
}

fn validate_no_self_intersections(
    topology: &Topology,
    mesh: &TriangleMesh,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    validate_triangle_pairs(topology, mesh, mesh, tolerance, true)
}

fn validate_meshes_disjoint(
    topology: &Topology,
    first: &TriangleMesh,
    second: &TriangleMesh,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    validate_triangle_pairs(topology, first, second, tolerance, false)
}

fn validate_triangle_pairs(
    topology: &Topology,
    first: &TriangleMesh,
    second: &TriangleMesh,
    tolerance: ModelTolerance,
    same_mesh: bool,
) -> KernelResult<()> {
    let faces = first
        .triangle_faces
        .iter()
        .chain(&second.triangle_faces)
        .copied()
        .collect::<BTreeSet<_>>();
    let face_edges = faces
        .iter()
        .map(|face| {
            topology
                .face_edges(*face)
                .map(|edges| (*face, edges.into_iter().collect::<BTreeSet<_>>()))
        })
        .collect::<KernelResult<BTreeMap<_, _>>>()?;
    let face_vertices = faces
        .iter()
        .map(|face| (*face, face_vertices(topology, *face)))
        .collect::<BTreeMap<_, _>>();
    let bounds = |mesh: &TriangleMesh| {
        mesh.triangles
            .iter()
            .map(|triangle| triangle_bounds(triangle_points(mesh, *triangle)))
            .collect::<Vec<_>>()
    };
    let first_bounds = bounds(first);
    let second_bounds = bounds(second);
    let mut second_order = (0..second.triangles.len()).collect::<Vec<_>>();
    second_order.sort_by(|first, second| {
        second_bounds[*first].0[0].total_cmp(&second_bounds[*second].0[0])
    });
    let mut pairs = Vec::new();
    if same_mesh {
        for (position, first_index) in second_order.iter().copied().enumerate() {
            let maximum = first_bounds[first_index].1[0] + tolerance.linear;
            for second_index in second_order.iter().copied().skip(position + 1) {
                if second_bounds[second_index].0[0] > maximum {
                    break;
                }
                pairs.push((first_index, second_index));
            }
        }
    } else {
        let mut first_order = (0..first.triangles.len()).collect::<Vec<_>>();
        first_order.sort_by(|a, b| first_bounds[*a].0[0].total_cmp(&first_bounds[*b].0[0]));
        let mut lower = 0usize;
        for first_index in first_order {
            let minimum = first_bounds[first_index].0[0] - tolerance.linear;
            let maximum = first_bounds[first_index].1[0] + tolerance.linear;
            while lower < second_order.len() && second_bounds[second_order[lower]].1[0] < minimum {
                lower += 1;
            }
            for second_index in second_order.iter().copied().skip(lower) {
                if second_bounds[second_index].0[0] > maximum {
                    break;
                }
                pairs.push((first_index, second_index));
            }
        }
    }
    for (first_index, second_index) in pairs {
        let first_triangle = &first.triangles[first_index];
        if same_mesh && first.triangle_faces[first_index] == second.triangle_faces[second_index] {
            continue;
        }
        if !bounds_overlap(
            first_bounds[first_index],
            second_bounds[second_index],
            tolerance.linear,
        ) {
            continue;
        }
        let a = triangle_points(first, *first_triangle);
        let b = triangle_points(second, second.triangles[second_index]);
        match triangle_intersection(a, b, tolerance)? {
            TriangleIntersection::None => {}
            TriangleIntersection::CoplanarArea => {
                let first_face = first.triangle_faces[first_index];
                let second_face = second.triangle_faces[second_index];
                if face_edges[&second_face]
                    .iter()
                    .any(|edge| face_edges[&first_face].contains(edge))
                {
                    continue;
                }
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "distinct BREP faces overlap over a coplanar area",
                ));
            }
            TriangleIntersection::Boundary(points) => {
                let first_face = first.triangle_faces[first_index];
                let second_face = second.triangle_faces[second_index];
                if let Some(point) = points.iter().find(|point| {
                    !allowed_face_contact(
                        topology,
                        &face_edges[&first_face],
                        &face_edges[&second_face],
                        &face_vertices[&first_face],
                        &face_vertices[&second_face],
                        **point,
                        tolerance.linear,
                    )
                }) {
                    return Err(KernelError::new(
                            DiagnosticKind::GeometryInvariant,
                            format!(
                                "shell faces {first_face:?} and {second_face:?} intersect away from shared topology near {point:?}",
                            ),
                        ));
                }
            }
        }
    }
    Ok(())
}

fn triangle_points(mesh: &TriangleMesh, triangle: [usize; 3]) -> [Point3; 3] {
    [
        mesh.positions[triangle[0]],
        mesh.positions[triangle[1]],
        mesh.positions[triangle[2]],
    ]
}

fn triangle_bounds(triangle: [Point3; 3]) -> ([f64; 3], [f64; 3]) {
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for point in triangle {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            minimum[axis] = minimum[axis].min(value);
            maximum[axis] = maximum[axis].max(value);
        }
    }
    (minimum, maximum)
}

fn bounds_overlap(
    first: ([f64; 3], [f64; 3]),
    second: ([f64; 3], [f64; 3]),
    tolerance: f64,
) -> bool {
    (0..3).all(|axis| {
        first.0[axis] <= second.1[axis] + tolerance && second.0[axis] <= first.1[axis] + tolerance
    })
}

enum TriangleIntersection {
    None,
    Boundary(Vec<Point3>),
    CoplanarArea,
}

fn triangle_intersection(
    first: [Point3; 3],
    second: [Point3; 3],
    tolerance: ModelTolerance,
) -> KernelResult<TriangleIntersection> {
    let first_normal = (first[1] - first[0]).cross(first[2] - first[0]);
    let second_normal = (second[1] - second[0]).cross(second[2] - second[0]);
    if first_normal.magnitude() <= tolerance.linear.powi(2)
        || second_normal.magnitude() <= tolerance.linear.powi(2)
    {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "tessellation contains a degenerate triangle",
        ));
    }
    let first_unit = first_normal.normalize();
    let second_unit = second_normal.normalize();
    let parallel = first_unit.cross(second_unit).magnitude() <= tolerance.angular;
    if parallel {
        let plane_distance = (second[0] - first[0]).dot(first_unit).abs();
        if plane_distance > tolerance.linear {
            return Ok(TriangleIntersection::None);
        }
        return coplanar_triangle_intersection(first, second, first_unit, tolerance.linear);
    }
    let first_distances = first.map(|point| (point - second[0]).dot(second_unit));
    let second_distances = second.map(|point| (point - first[0]).dot(first_unit));
    if strictly_one_side(first_distances, tolerance.linear)
        || strictly_one_side(second_distances, tolerance.linear)
    {
        return Ok(TriangleIntersection::None);
    }
    let mut points = Vec::new();
    collect_plane_crossings(first, second, second_unit, tolerance.linear, &mut points);
    collect_plane_crossings(second, first, first_unit, tolerance.linear, &mut points);
    deduplicate_points(&mut points, tolerance.linear);
    if points.is_empty() {
        Ok(TriangleIntersection::None)
    } else {
        Ok(TriangleIntersection::Boundary(points))
    }
}

fn strictly_one_side(distances: [f64; 3], tolerance: f64) -> bool {
    distances.iter().all(|distance| *distance > tolerance)
        || distances.iter().all(|distance| *distance < -tolerance)
}

fn collect_plane_crossings(
    triangle: [Point3; 3],
    other: [Point3; 3],
    other_normal: Vec3,
    tolerance: f64,
    output: &mut Vec<Point3>,
) {
    for edge in [[0, 1], [1, 2], [2, 0]] {
        let start = triangle[edge[0]];
        let end = triangle[edge[1]];
        let start_distance = (start - other[0]).dot(other_normal);
        let end_distance = (end - other[0]).dot(other_normal);
        if start_distance.abs() <= tolerance && point_in_triangle(start, other, tolerance) {
            output.push(start);
        }
        if end_distance.abs() <= tolerance && point_in_triangle(end, other, tolerance) {
            output.push(end);
        }
        if start_distance * end_distance < 0.0 {
            let parameter = start_distance / (start_distance - end_distance);
            let point = start + (end - start) * parameter;
            if point_in_triangle(point, other, tolerance) {
                output.push(point);
            }
        }
    }
}

fn point_in_triangle(point: Point3, triangle: [Point3; 3], tolerance: f64) -> bool {
    let v0 = triangle[1] - triangle[0];
    let v1 = triangle[2] - triangle[0];
    let v2 = point - triangle[0];
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let denominator = d00 * d11 - d01 * d01;
    if denominator == 0.0 {
        return false;
    }
    let v = (d11 * d20 - d01 * d21) / denominator;
    let w = (d00 * d21 - d01 * d20) / denominator;
    let u = 1.0 - v - w;
    let scaled = tolerance / d00.sqrt().max(d11.sqrt()).max(tolerance);
    u >= -scaled && v >= -scaled && w >= -scaled
}

fn coplanar_triangle_intersection(
    first: [Point3; 3],
    second: [Point3; 3],
    normal: Vec3,
    tolerance: f64,
) -> KernelResult<TriangleIntersection> {
    let drop_axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let project = |point: Point3| -> [f64; 2] {
        match drop_axis {
            0 => [point.y, point.z],
            1 => [point.x, point.z],
            _ => [point.x, point.y],
        }
    };
    let a = first.map(project);
    let b = second.map(project);
    let overlap = clip_convex_polygon(a.to_vec(), b);
    if overlap.len() >= 3 {
        let twice_area = overlap
            .iter()
            .zip(overlap.iter().cycle().skip(1))
            .map(|(first, second)| first[0] * second[1] - first[1] * second[0])
            .sum::<f64>()
            .abs();
        let scale = a
            .iter()
            .chain(&b)
            .map(|point| point[0].abs().max(point[1].abs()))
            .fold(1.0_f64, f64::max);
        if twice_area > 2.0 * tolerance * scale {
            return Ok(TriangleIntersection::CoplanarArea);
        }
    }
    let mut boundary = Vec::new();
    for point in first {
        if point_in_triangle2_inclusive(project(point), b, tolerance) {
            boundary.push(point);
        }
    }
    for point in second {
        if point_in_triangle2_inclusive(project(point), a, tolerance) {
            boundary.push(point);
        }
    }
    deduplicate_points(&mut boundary, tolerance);
    if boundary.is_empty() {
        Ok(TriangleIntersection::None)
    } else {
        Ok(TriangleIntersection::Boundary(boundary))
    }
}

fn orient2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn clip_convex_polygon(mut subject: Vec<[f64; 2]>, clip: [[f64; 2]; 3]) -> Vec<[f64; 2]> {
    let clip_orientation = orient2(clip[0], clip[1], clip[2]).signum();
    for index in 0..3 {
        let start = clip[index];
        let end = clip[(index + 1) % 3];
        let input = std::mem::take(&mut subject);
        if input.is_empty() {
            break;
        }
        let inside = |point| orient2(start, end, point) * clip_orientation >= 0.0;
        let mut previous = *input.last().expect("nonempty clipped polygon");
        let mut previous_inside = inside(previous);
        for current in input {
            let current_inside = inside(current);
            if current_inside != previous_inside {
                let direction = [current[0] - previous[0], current[1] - previous[1]];
                let edge = [end[0] - start[0], end[1] - start[1]];
                let denominator = direction[0] * edge[1] - direction[1] * edge[0];
                if denominator.abs() > f64::EPSILON {
                    let relative = [start[0] - previous[0], start[1] - previous[1]];
                    let parameter = (relative[0] * edge[1] - relative[1] * edge[0]) / denominator;
                    subject.push([
                        previous[0] + direction[0] * parameter,
                        previous[1] + direction[1] * parameter,
                    ]);
                }
            }
            if current_inside {
                subject.push(current);
            }
            previous = current;
            previous_inside = current_inside;
        }
    }
    subject
}

fn point_in_triangle2_inclusive(point: [f64; 2], triangle: [[f64; 2]; 3], tolerance: f64) -> bool {
    let values = [
        orient2(triangle[0], triangle[1], point),
        orient2(triangle[1], triangle[2], point),
        orient2(triangle[2], triangle[0], point),
    ];
    let area_tolerance = tolerance * tolerance;
    values.iter().all(|value| *value >= -area_tolerance)
        || values.iter().all(|value| *value <= area_tolerance)
}

fn deduplicate_points(points: &mut Vec<Point3>, tolerance: f64) {
    let mut unique = Vec::new();
    for point in points.drain(..) {
        if unique
            .iter()
            .all(|existing: &Point3| existing.distance(point) > tolerance)
        {
            unique.push(point);
        }
    }
    *points = unique;
}

fn allowed_face_contact(
    topology: &Topology,
    first_edges: &BTreeSet<super::EdgeId>,
    second_edges: &BTreeSet<super::EdgeId>,
    first_vertices: &BTreeSet<super::VertexId>,
    second_vertices: &BTreeSet<super::VertexId>,
    point: Point3,
    tolerance: f64,
) -> bool {
    if first_vertices.intersection(second_vertices).any(|vertex| {
        topology
            .vertex(*vertex)
            .map(|value| value.point().distance(point) <= tolerance)
            .unwrap_or(false)
    }) {
        return true;
    }
    for edge_id in first_edges.intersection(second_edges) {
        if let Ok(edge) = topology.edge(*edge_id) {
            let vertices = edge.vertices();
            if let (Ok(start), Ok(end)) =
                (topology.vertex(vertices[0]), topology.vertex(vertices[1]))
            {
                if point_segment_distance(point, start.point(), end.point()) <= tolerance {
                    return true;
                }
            }
        }
    }
    false
}

fn face_vertices(topology: &Topology, face: FaceId) -> BTreeSet<super::VertexId> {
    let mut output = BTreeSet::new();
    if let Ok(face) = topology.face(face) {
        for loop_id in face.loops() {
            let Ok(loop_value) = topology.loop_(*loop_id) else {
                continue;
            };
            let Some(first) = loop_value.first() else {
                continue;
            };
            let mut current = first;
            loop {
                if let Ok(vertices) = topology.coedge_vertices(current) {
                    output.extend(vertices);
                }
                let Ok(coedge) = topology.coedge(current) else {
                    break;
                };
                current = coedge.next();
                if current == first {
                    break;
                }
            }
        }
    }
    output
}

fn point_segment_distance(point: Point3, start: Point3, end: Point3) -> f64 {
    let segment = end - start;
    let parameter = if segment.magnitude2() == 0.0 {
        0.0
    } else {
        (point - start).dot(segment) / segment.magnitude2()
    }
    .clamp(0.0, 1.0);
    point.distance(start + segment * parameter)
}

fn point_inside_mesh(
    point: Point3,
    mesh: &TriangleMesh,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let direction = Vec3::new(1.0, std::f64::consts::SQRT_2, std::f64::consts::PI).normalize();
    let mut parameters = Vec::new();
    for triangle in &mesh.triangles {
        let vertices = triangle_points(mesh, *triangle);
        if let Some(parameter) =
            ray_triangle_parameter(point, direction, vertices, tolerance.linear)
        {
            if parameter.abs() <= tolerance.linear {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "containment point lies on shell boundary",
                ));
            }
            if parameter > 0.0 {
                parameters.push(parameter);
            }
        }
    }
    parameters.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.linear);
    Ok(parameters.len() % 2 == 1)
}

fn ray_triangle_parameter(
    origin: Point3,
    direction: Vec3,
    triangle: [Point3; 3],
    tolerance: f64,
) -> Option<f64> {
    let edge1 = triangle[1] - triangle[0];
    let edge2 = triangle[2] - triangle[0];
    let p = direction.cross(edge2);
    let determinant = edge1.dot(p);
    if determinant.abs() <= tolerance {
        return None;
    }
    let inverse = 1.0 / determinant;
    let t = origin - triangle[0];
    let u = t.dot(p) * inverse;
    if u < -tolerance || u > 1.0 + tolerance {
        return None;
    }
    let q = t.cross(edge1);
    let v = direction.dot(q) * inverse;
    if v < -tolerance || u + v > 1.0 + tolerance {
        return None;
    }
    Some(edge2.dot(q) * inverse)
}
