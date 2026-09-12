use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::integration::{adaptive_gauss_kronrod_partitioned, adaptive_simpson};
use super::{
    BodyId, Curve2, Curve3, Diagnostic, DiagnosticKind, DomainBounds, FaceId, Kernel, KernelError,
    KernelResult, MassProperties, ModelTolerance, ShellId, Surface3, Topology,
};

#[derive(Debug, Clone)]
pub(crate) struct PhysicalCertificate {
    pub properties: MassProperties,
}

pub(crate) fn validate_supplied_properties(
    properties: MassProperties,
    tolerance: ModelTolerance,
    scale: f64,
) -> KernelResult<()> {
    let minimum_volume = tolerance.linear_at_scale(scale).powi(3);
    if !properties.volume.is_finite()
        || !properties.signed_volume.is_finite()
        || properties.volume <= minimum_volume
        || properties.signed_volume <= minimum_volume
        || (properties.volume - properties.signed_volume).abs()
            > minimum_volume.max(properties.volume * f64::EPSILON * 4096.0)
    {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "analytical certificate has invalid volume",
        ));
    }
    if !properties.surface_area.is_finite()
        || properties.surface_area <= tolerance.linear_at_scale(scale).powi(2)
    {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "analytical certificate has invalid surface area",
        ));
    }
    if !properties.centroid.x.is_finite()
        || !properties.centroid.y.is_finite()
        || !properties.centroid.z.is_finite()
    {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "analytical certificate has a non-finite centroid",
        ));
    }
    let inertia_tolerance = minimum_volume * scale.powi(2);
    for row in 0..3 {
        for column in 0..3 {
            if !properties.inertia[row][column].is_finite() {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "analytical certificate has non-finite inertia",
                ));
            }
            if (properties.inertia[row][column] - properties.inertia[column][row]).abs()
                > inertia_tolerance.max(
                    properties.inertia[row][column]
                        .abs()
                        .max(properties.inertia[column][row].abs())
                        * f64::EPSILON
                        * 4096.0,
                )
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "analytical certificate inertia is not symmetric",
                ));
            }
        }
        if properties.inertia[row][row] < -inertia_tolerance {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "analytical certificate has negative principal-axis inertia",
            ));
        }
    }
    Ok(())
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
    // Curved bodies are handled by structural analytical recognizers above.
    // The remaining polyhedral path integrates the exact planar BREP boundary;
    // publication never depends on rendering tessellation.
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
    let has_general_surface = face_ids.iter().any(|face_id| {
        topology
            .face(*face_id)
            .map(|face| {
                matches!(
                    face.surface(),
                    Surface3::Nurbs(_)
                        | Surface3::Revolved(_)
                        | Surface3::Swept(_)
                        | Surface3::Offset(_)
                        | Surface3::RollingBall(_)
                        | Surface3::Ruled(_)
                )
            })
            .unwrap_or(true)
    });
    if !has_general_surface {
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
    }
    let requires_general_integration = edge_ids.iter().any(|edge_id| {
        topology
            .edge(*edge_id)
            .map(|edge| !matches!(edge.curve(), Curve3::Line(_)))
            .unwrap_or(true)
    }) || face_ids.iter().any(|face_id| {
        topology
            .face(*face_id)
            .map(|face| !matches!(face.surface(), Surface3::Plane(_)))
            .unwrap_or(true)
    });
    if requires_general_integration {
        let mut shell_properties = Vec::new();
        for shell_id in body.shells() {
            validate_shell_connected(topology, *shell_id)?;
            shell_properties.push(brep_shell_properties(
                topology,
                *shell_id,
                physical_tolerance,
                body_scale,
            )?);
        }
        let properties = combine_signed_shell_properties(shell_properties, physical_tolerance)?;
        validate_supplied_properties(properties, physical_tolerance, body_scale)?;
        return Ok(PhysicalCertificate { properties });
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

    let mut shell_properties = Vec::new();
    for shell_id in body.shells() {
        validate_shell_connected(topology, *shell_id)?;
        validate_polyhedral_shell_intersections(topology, *shell_id, physical_tolerance)?;
        let properties = polyhedral_shell_properties(topology, *shell_id, physical_tolerance)?;
        if properties.volume <= physical_tolerance.linear.powi(3) {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "shell volume does not exceed the model tolerance volume",
            ));
        }
        shell_properties.push((*shell_id, properties));
    }

    // One connected positive outer shell; all other shells are negatively oriented cavities.
    let positive: Vec<_> = shell_properties
        .iter()
        .filter(|(_, properties)| properties.signed_volume > 0.0)
        .collect();
    if positive.len() != 1 {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "a body must contain exactly one outward-oriented outer shell",
        ));
    }
    let outer_shell = positive[0].0;
    for (shell_id, properties) in &shell_properties {
        if *shell_id == outer_shell {
            continue;
        }
        if properties.signed_volume >= 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "inner shells must be oriented as cavities",
            ));
        }
        validate_polyhedral_shells_disjoint(topology, outer_shell, *shell_id, physical_tolerance)?;
        if !point_inside_polyhedral_shell(
            topology,
            outer_shell,
            properties.centroid,
            physical_tolerance,
        )? {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "cavity shell is not contained by the outer shell",
            ));
        }
    }
    for first in 0..shell_properties.len() {
        for second in first + 1..shell_properties.len() {
            if shell_properties[first].0 == outer_shell || shell_properties[second].0 == outer_shell
            {
                continue;
            }
            validate_polyhedral_shells_disjoint(
                topology,
                shell_properties[first].0,
                shell_properties[second].0,
                physical_tolerance,
            )?;
            let first_center = shell_properties[first].1.centroid;
            let second_center = shell_properties[second].1.centroid;
            if point_inside_polyhedral_shell(
                topology,
                shell_properties[second].0,
                first_center,
                physical_tolerance,
            )? || point_inside_polyhedral_shell(
                topology,
                shell_properties[first].0,
                second_center,
                physical_tolerance,
            )? {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "cavity shells may not contain or overlap one another",
                ));
            }
        }
    }

    let properties = combine_signed_shell_properties(
        shell_properties
            .into_iter()
            .map(|(_, properties)| properties),
        physical_tolerance,
    )?;
    if properties.signed_volume <= physical_tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "body has no positive physical volume after subtracting cavities",
        ));
    }
    Ok(PhysicalCertificate { properties })
}

/// Direct divergence-theorem integration over exact trimmed parametric faces.
///
/// For each surface quantity `h(u,v)`, Green's theorem converts the UV-domain
/// integral to `integral Q dv` or `-integral P du` around the exact p-curves,
/// where Q integrates h along u and P integrates it along v. Both are adaptive and
/// evaluate the retained analytical/implicit supports; rendering meshes never
/// participate in physical certification.
fn brep_shell_properties(
    topology: &Topology,
    shell_id: ShellId,
    tolerance: ModelTolerance,
    scale: f64,
) -> KernelResult<MassProperties> {
    const TERMS: usize = 11;
    let dimensional_target = |power: i32| {
        (tolerance.linear * scale.powi(power - 1))
            .max(f64::EPSILON.sqrt() * scale.powi(power) * 64.0)
    };
    let targets = [
        dimensional_target(2),
        dimensional_target(3),
        dimensional_target(4),
        dimensional_target(4),
        dimensional_target(4),
        dimensional_target(5),
        dimensional_target(5),
        dimensional_target(5),
        dimensional_target(5),
        dimensional_target(5),
        dimensional_target(5),
    ];
    let mut totals = [0.0; TERMS];
    let shell = topology.shell(shell_id)?;
    for face_id in shell.faces() {
        let face = topology.face(*face_id)?;
        let orientation = match face.orientation() {
            super::FaceOrientation::Forward => 1.0,
            super::FaceOrientation::Reversed => -1.0,
        };
        let surface_terms = |u: f64, v: f64| -> KernelResult<[f64; TERMS]> {
            let (point, du, dv) = face.surface().first_order(u, v)?;
            let parametric_normal = du.cross(dv);
            let jacobian = parametric_normal.magnitude();
            if jacobian <= f64::EPSILON * scale.powi(2) {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "physical integration encountered a singular surface point",
                ));
            }
            let normal = parametric_normal * orientation;
            Ok([
                jacobian,
                point.to_vec().dot(normal) / 3.0,
                point.x.powi(2) * normal.x / 2.0,
                point.y.powi(2) * normal.y / 2.0,
                point.z.powi(2) * normal.z / 2.0,
                point.x.powi(3) * normal.x / 3.0,
                point.y.powi(3) * normal.y / 3.0,
                point.z.powi(3) * normal.z / 3.0,
                point.x.powi(2) * point.y * normal.x / 2.0,
                point.x.powi(2) * point.z * normal.x / 2.0,
                point.y.powi(2) * point.z * normal.y / 2.0,
            ])
        };
        // Offset blends need the rail derivatives to evaluate their normals.
        // Integrating across v first reuses one section throughout the inner
        // quadrature. Keep the established path for cheaper un-offset supports.
        // Green's theorem permits either Q dv or -P du with the same targets.
        let mut support = face.surface();
        while let Surface3::Offset(offset) = support {
            support = offset.base();
        }
        let primitive_axis = usize::from(
            matches!(face.surface(), Surface3::Offset(_))
                && matches!(support, Surface3::RollingBall(_) | Surface3::Ruled(_)),
        );
        let primitive_domain = face.surface().domains()[primitive_axis];
        let domain_origin = match primitive_domain.bounds {
            DomainBounds::Bounded(range) => range.start,
            DomainBounds::LowerBounded(start) => start,
            DomainBounds::UpperBounded(end) => end,
            DomainBounds::Unbounded => 0.0,
        };
        let primitive_origin = if primitive_domain.contains(0.0, tolerance.parameter) {
            0.0
        } else {
            face.loops()
                .first()
                .and_then(|loop_id| topology.loop_coedges(*loop_id).ok())
                .and_then(|coedges| coedges.first().copied())
                .and_then(|coedge_id| topology.coedge(coedge_id).ok())
                .and_then(|coedge| {
                    coedge
                        .pcurve()
                        .evaluate(coedge.range().start)
                        .ok()
                        .map(|value| value.point[primitive_axis])
                })
                .unwrap_or(domain_origin)
        };
        for loop_id in face.loops() {
            // Lift every periodic p-curve loop into one continuous UV chart.
            // Topology may legitimately store both uses of a seam in the same
            // principal period because they are the same spatial edge. Green's
            // theorem, however, needs the two chart copies separated by one
            // period or their contributions cancel and the curved face appears
            // to have zero area and volume.
            let domains = face.surface().domains();
            let mut previous_end = None;
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let range = coedge.range();
                let raw_start = coedge.pcurve().evaluate(range.start)?.point;
                let chart_shift = if let Some(previous) = previous_end {
                    let delta: Vec2 = previous - raw_start;
                    let (point, du, dv) = face.surface().first_order(raw_start.x, raw_start.y)?;
                    let linear = tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0));
                    let shift = |value: f64, period: Option<f64>, tangent: Vec3| {
                        period
                            .map(|period| {
                                let candidate = (value / period).round() * period;
                                // Sewn implicit trims agree in model space, not
                                // necessarily to the much smaller parameter epsilon.
                                // Scale that existing tolerance by the local chart
                                // metric before recognizing a complete seam turn.
                                // At a collapsed pole retain the explicit UV span.
                                let speed = tangent.magnitude();
                                let epsilon = if speed > linear {
                                    (linear / speed).max(tolerance.parameter * 64.0)
                                } else {
                                    tolerance.parameter * 64.0
                                };
                                if (value - candidate).abs() <= epsilon {
                                    candidate
                                } else {
                                    0.0
                                }
                            })
                            .unwrap_or(0.0)
                    };
                    Vec2::new(
                        shift(delta.x, domains[0].period, du),
                        shift(delta.y, domains[1].period, dv),
                    )
                } else {
                    Vec2::new(0.0, 0.0)
                };
                let raw_end = coedge.pcurve().evaluate(range.end)?.point;
                previous_end = Some(raw_end + chart_shift);
                let breakpoints = coedge.pcurve().integration_breakpoints(range);
                let outer = adaptive_gauss_kronrod_partitioned(
                    range.start,
                    range.end,
                    |parameter| {
                        let (raw_point, derivative) = coedge.pcurve().first_order(parameter)?;
                        let weight = if primitive_axis == 0 {
                            derivative.y
                        } else {
                            -derivative.x
                        };
                        if weight == 0.0 {
                            return Ok([0.0; TERMS]);
                        }
                        let point = raw_point + chart_shift;
                        let inner = adaptive_simpson(
                            primitive_origin,
                            point[primitive_axis],
                            |parameter| {
                                if primitive_axis == 0 {
                                    surface_terms(parameter, point.y)
                                } else {
                                    surface_terms(point.x, parameter)
                                }
                            },
                            targets,
                            14,
                            "exact BREP surface primitive did not converge",
                        )?;
                        Ok(inner.map(|value| value * weight * orientation))
                    },
                    targets,
                    14,
                    "exact BREP boundary integration did not converge",
                    &breakpoints,
                )
                .map_err(|error| {
                    KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        format!(
                            "{error} (face support {}, boundary p-curve {}, parameter range [{}, {}])",
                            surface_kind(face.surface()),
                            curve2_kind(coedge.pcurve()),
                            range.start,
                            range.end,
                        ),
                    )
                })?;
                for index in 0..TERMS {
                    totals[index] += outer[index];
                }
            }
        }
    }
    let signed_volume = totals[1];
    if !signed_volume.is_finite() || signed_volume.abs() <= tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "exact BREP shell integration produced zero or non-finite volume",
        ));
    }
    let centroid = Point3::new(
        totals[2] / signed_volume,
        totals[3] / signed_volume,
        totals[4] / signed_volume,
    );
    let second_moment = [
        [totals[5], totals[8], totals[9]],
        [totals[8], totals[6], totals[10]],
        [totals[9], totals[10], totals[7]],
    ];
    let inertia_origin = [
        [
            second_moment[1][1] + second_moment[2][2],
            -second_moment[0][1],
            -second_moment[0][2],
        ],
        [
            -second_moment[1][0],
            second_moment[0][0] + second_moment[2][2],
            -second_moment[1][2],
        ],
        [
            -second_moment[2][0],
            -second_moment[2][1],
            second_moment[0][0] + second_moment[1][1],
        ],
    ];
    let center = [centroid.x, centroid.y, centroid.z];
    let center_squared = center.iter().map(|value| value * value).sum::<f64>();
    let mut inertia = inertia_origin;
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] -= signed_volume
                * (if row == column { center_squared } else { 0.0 } - center[row] * center[column]);
        }
    }
    Ok(MassProperties {
        volume: signed_volume.abs(),
        signed_volume,
        surface_area: totals[0],
        centroid,
        inertia,
    })
}

fn surface_kind(surface: &super::Surface3) -> &'static str {
    match surface {
        super::Surface3::Plane(_) => "plane",
        super::Surface3::Cylinder(_) => "cylinder",
        super::Surface3::Cone(_) => "cone",
        super::Surface3::Sphere(_) => "sphere",
        super::Surface3::Torus(_) => "torus",
        super::Surface3::Revolved(_) => "revolved",
        super::Surface3::Swept(_) => "swept",
        super::Surface3::Nurbs(_) => "NURBS",
        super::Surface3::Offset(_) => "offset",
        super::Surface3::RollingBall(_) => "rolling-ball",
        super::Surface3::Ruled(_) => "ruled",
    }
}

fn curve2_kind(curve: &super::Curve2) -> &'static str {
    match curve {
        super::Curve2::Line(_) => "line",
        super::Curve2::Circle(_) => "circle",
        super::Curve2::Nurbs(_) => "NURBS",
        super::Curve2::SurfaceIntersection(_) => "surface intersection",
        super::Curve2::SurfaceProjection(_) => "surface projection",
    }
}

fn polyhedral_shell_properties(
    topology: &Topology,
    shell_id: ShellId,
    tolerance: ModelTolerance,
) -> KernelResult<MassProperties> {
    let shell = topology.shell(shell_id)?;
    let mut signed_volume = 0.0;
    let mut surface_area = 0.0;
    let mut first_moment = Vec3::new(0.0, 0.0, 0.0);
    let mut second_moment = [[0.0; 3]; 3];
    for face_id in shell.faces() {
        let face = topology.face(*face_id)?;
        let Surface3::Plane(plane) = face.surface() else {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "analytical polyhedral integration requires planar faces",
            ));
        };
        let effective_normal = match face.orientation() {
            super::FaceOrientation::Forward => plane.normal(),
            super::FaceOrientation::Reversed => -plane.normal(),
        };
        let mut face_area = 0.0;
        for loop_id in face.loops() {
            let points = loop_points3(topology, *loop_id)?;
            if points.len() < 3 {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "polyhedral face loop needs at least three vertices",
                ));
            }
            let anchor = points[0];
            for index in 1..points.len() - 1 {
                let a = anchor;
                let b = points[index];
                let c = points[index + 1];
                let cross = (b - a).cross(c - a);
                face_area += cross.dot(effective_normal) * 0.5;
                let tetrahedron = a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0;
                signed_volume += tetrahedron;
                first_moment += (a.to_vec() + b.to_vec() + c.to_vec()) * (tetrahedron / 4.0);
                let coordinates = [[a.x, a.y, a.z], [b.x, b.y, b.z], [c.x, c.y, c.z]];
                for (axis, moments) in second_moment.iter_mut().enumerate() {
                    let values = [
                        coordinates[0][axis],
                        coordinates[1][axis],
                        coordinates[2][axis],
                    ];
                    moments[axis] += tetrahedron / 10.0
                        * (values[0] * values[0]
                            + values[1] * values[1]
                            + values[2] * values[2]
                            + values[0] * values[1]
                            + values[0] * values[2]
                            + values[1] * values[2]);
                }
                for first in 0..3 {
                    for second in first + 1..3 {
                        let diagonal_products = (0..3)
                            .map(|vertex| coordinates[vertex][first] * coordinates[vertex][second])
                            .sum::<f64>();
                        let cross_products = coordinates[0][first] * coordinates[1][second]
                            + coordinates[0][second] * coordinates[1][first]
                            + coordinates[0][first] * coordinates[2][second]
                            + coordinates[0][second] * coordinates[2][first]
                            + coordinates[1][first] * coordinates[2][second]
                            + coordinates[1][second] * coordinates[2][first];
                        let value = tetrahedron / 20.0 * (2.0 * diagonal_products + cross_products);
                        second_moment[first][second] += value;
                        second_moment[second][first] += value;
                    }
                }
            }
        }
        if face_area <= tolerance.linear.powi(2) {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "polyhedral face has zero or inconsistent oriented area",
            ));
        }
        surface_area += face_area;
    }
    if !signed_volume.is_finite() || signed_volume.abs() <= tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "polyhedral shell has zero or non-finite signed volume",
        ));
    }
    let centroid = Point3::from_vec(first_moment / signed_volume);
    let inertia_origin = [
        [
            second_moment[1][1] + second_moment[2][2],
            -second_moment[0][1],
            -second_moment[0][2],
        ],
        [
            -second_moment[1][0],
            second_moment[0][0] + second_moment[2][2],
            -second_moment[1][2],
        ],
        [
            -second_moment[2][0],
            -second_moment[2][1],
            second_moment[0][0] + second_moment[1][1],
        ],
    ];
    let center = [centroid.x, centroid.y, centroid.z];
    let center_squared = center.iter().map(|value| value * value).sum::<f64>();
    let mut inertia = inertia_origin;
    for row in 0..3 {
        for column in 0..3 {
            inertia[row][column] -= signed_volume
                * (if row == column { center_squared } else { 0.0 } - center[row] * center[column]);
        }
    }
    Ok(MassProperties {
        volume: signed_volume.abs(),
        signed_volume,
        surface_area,
        centroid,
        inertia,
    })
}

fn combine_signed_shell_properties(
    components: impl IntoIterator<Item = MassProperties>,
    tolerance: ModelTolerance,
) -> KernelResult<MassProperties> {
    let components = components.into_iter().collect::<Vec<_>>();
    let signed_volume = components
        .iter()
        .map(|properties| properties.signed_volume)
        .sum::<f64>();
    if signed_volume <= tolerance.linear.powi(3) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "body has no positive analytical volume after subtracting cavities",
        ));
    }
    let centroid = Point3::from_vec(
        components
            .iter()
            .fold(Vec3::new(0.0, 0.0, 0.0), |sum, properties| {
                sum + properties.centroid.to_vec() * properties.signed_volume
            })
            / signed_volume,
    );
    let mut inertia = [[0.0; 3]; 3];
    for properties in &components {
        let offset = properties.centroid - centroid;
        for row in 0..3 {
            for column in 0..3 {
                inertia[row][column] += properties.inertia[row][column]
                    + properties.signed_volume
                        * (if row == column {
                            offset.magnitude2()
                        } else {
                            0.0
                        } - offset[row] * offset[column]);
            }
        }
    }
    Ok(MassProperties {
        volume: signed_volume,
        signed_volume,
        surface_area: components
            .iter()
            .map(|properties| properties.surface_area)
            .sum(),
        centroid,
        inertia,
    })
}

fn loop_points3(topology: &Topology, loop_id: super::LoopId) -> KernelResult<Vec<Point3>> {
    topology
        .loop_coedges(loop_id)?
        .into_iter()
        .map(|coedge| {
            let vertex = topology.coedge_vertices(coedge)?[0];
            Ok(topology.vertex(vertex)?.point())
        })
        .collect()
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
                PrismCap {
                    face: first_id,
                    edges: &first_edges,
                },
                PrismCap {
                    face: second_id,
                    edges: &second_edges,
                },
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
    let scale = [terms(start)?, terms((start + end) * 0.5)?, terms(end)?]
        .iter()
        .flat_map(|values| values.iter().copied())
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let absolute = tolerance
        .linear_at_scale(scale)
        .max(f64::EPSILON * scale * 256.0);
    let targets = [absolute; 6];
    adaptive_simpson(
        start,
        end,
        terms,
        targets,
        24,
        "certified NURBS boundary integration did not converge",
    )
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

#[derive(Clone, Copy)]
struct PrismCap<'a> {
    face: FaceId,
    edges: &'a BTreeSet<super::EdgeId>,
}

fn validate_prism_side_incidence(
    topology: &Topology,
    face_ids: &BTreeSet<FaceId>,
    first_cap: PrismCap<'_>,
    second_cap: PrismCap<'_>,
    sweep: Vec3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let mut first_incidence = 0usize;
    let mut second_incidence = 0usize;
    for face_id in face_ids {
        if *face_id == first_cap.face || *face_id == second_cap.face {
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
        } else {
            // Incidence alone does not make a prism: a transverse cylindrical
            // limit can connect equal end sections while bowing between them.
            // Every side support must be invariant along the proposed sweep.
            let direction = sweep.normalize();
            match face.surface() {
                Surface3::Plane(plane) if plane.normal().dot(direction).abs() <= tolerance.angular => {}
                Surface3::Cylinder(cylinder) if cylinder.axis().normalize().cross(direction).magnitude() <= tolerance.angular => {}
                _ => return Ok(false),
            }
        }
        let edges = face_edges(topology, *face_id)?;
        let on_first = edges.intersection(first_cap.edges).count();
        let on_second = edges.intersection(second_cap.edges).count();
        if on_first == 0 || on_second == 0 {
            return Ok(false);
        }
        first_incidence += on_first;
        second_incidence += on_second;
    }
    Ok(first_incidence == first_cap.edges.len() && second_incidence == second_cap.edges.len())
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
            Curve3::Nurbs(_) | Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => {
                return Ok(None)
            }
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
            Curve3::Nurbs(_) | Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => {
                return Ok(None)
            }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrimLocation {
    Outside,
    Boundary,
    Inside,
}

fn face_trim_location(
    topology: &Topology,
    face_id: FaceId,
    point: Point3,
    tolerance: ModelTolerance,
) -> KernelResult<TrimLocation> {
    let face = topology.face(face_id)?;
    let Surface3::Plane(plane) = face.surface() else {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "polyhedral containment requires planar faces",
        ));
    };
    let projection = plane.project(point)?;
    if projection.distance > tolerance.linear {
        return Ok(TrimLocation::Outside);
    }
    let uv = Point2::new(projection.parameters.0, projection.parameters.1);
    let mut inside_outer = false;
    for loop_id in face.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let polygon = topology
            .loop_coedges(*loop_id)?
            .into_iter()
            .map(|coedge| {
                let value = topology.coedge(coedge)?;
                Ok(value.pcurve().evaluate(value.range().start)?.point)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if (0..polygon.len()).any(|index| {
            on_segment2(
                uv,
                polygon[index],
                polygon[(index + 1) % polygon.len()],
                tolerance.parameter.max(tolerance.linear),
            )
        }) {
            return Ok(TrimLocation::Boundary);
        }
        let inside = point_in_polygon2(uv, &polygon, tolerance.parameter.max(tolerance.linear));
        match loop_value.kind() {
            super::LoopKind::Outer => inside_outer = inside,
            super::LoopKind::Inner if inside => return Ok(TrimLocation::Outside),
            super::LoopKind::Inner => {}
        }
    }
    Ok(if inside_outer {
        TrimLocation::Inside
    } else {
        TrimLocation::Outside
    })
}

fn point_inside_polyhedral_shell(
    topology: &Topology,
    shell_id: ShellId,
    point: Point3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let directions = [
        Vec3::new(1.0, std::f64::consts::SQRT_2, std::f64::consts::PI).normalize(),
        Vec3::new(std::f64::consts::E, 1.0, std::f64::consts::SQRT_2).normalize(),
        Vec3::new(std::f64::consts::PI, std::f64::consts::E, 1.0).normalize(),
    ];
    for direction in directions {
        let mut parameters = Vec::new();
        let mut ambiguous = false;
        for face_id in topology.shell(shell_id)?.faces() {
            let face = topology.face(*face_id)?;
            let Surface3::Plane(plane) = face.surface() else {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "polyhedral containment requires planar faces",
                ));
            };
            let denominator = direction.dot(plane.normal());
            if denominator.abs() <= tolerance.angular {
                continue;
            }
            let parameter = (plane.origin() - point).dot(plane.normal()) / denominator;
            if parameter <= tolerance.linear {
                continue;
            }
            let hit = point + direction * parameter;
            match face_trim_location(topology, *face_id, hit, tolerance)? {
                TrimLocation::Inside => parameters.push(parameter),
                TrimLocation::Boundary => {
                    ambiguous = true;
                    break;
                }
                TrimLocation::Outside => {}
            }
        }
        if !ambiguous {
            parameters.sort_by(f64::total_cmp);
            parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.linear);
            return Ok(parameters.len() % 2 == 1);
        }
    }
    Err(KernelError::new(
        DiagnosticKind::NumericalFailure,
        "analytical shell containment ray repeatedly hit boundary topology",
    ))
}

fn validate_polyhedral_shell_intersections(
    topology: &Topology,
    shell_id: ShellId,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let faces = topology.shell(shell_id)?.faces();
    for first in 0..faces.len() {
        for second in first + 1..faces.len() {
            validate_polyhedral_face_pair(topology, faces[first], faces[second], tolerance)?;
        }
    }
    Ok(())
}

fn validate_polyhedral_shells_disjoint(
    topology: &Topology,
    first: ShellId,
    second: ShellId,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    for first_face in topology.shell(first)?.faces() {
        for second_face in topology.shell(second)?.faces() {
            validate_polyhedral_face_pair(topology, *first_face, *second_face, tolerance)?;
        }
    }
    Ok(())
}

fn validate_polyhedral_face_pair(
    topology: &Topology,
    first: FaceId,
    second: FaceId,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let first_face = topology.face(first)?;
    let second_face = topology.face(second)?;
    let (Surface3::Plane(first_plane), Surface3::Plane(second_plane)) =
        (first_face.surface(), second_face.surface())
    else {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "polyhedral intersection validation requires planar faces",
        ));
    };
    let first_edges = topology
        .face_edges(first)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let second_edges = topology
        .face_edges(second)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let first_vertices = face_vertices(topology, first);
    let second_vertices = face_vertices(topology, second);
    let parallel = first_plane
        .normal()
        .cross(second_plane.normal())
        .magnitude()
        <= tolerance.angular;
    if parallel {
        if (second_plane.origin() - first_plane.origin())
            .dot(first_plane.normal())
            .abs()
            > tolerance.linear
        {
            return Ok(());
        }
        for vertex in first_vertices.iter().copied() {
            let point = topology.vertex(vertex)?.point();
            if face_trim_location(topology, second, point, tolerance)? == TrimLocation::Inside {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "distinct coplanar BREP faces overlap",
                ));
            }
        }
        for vertex in second_vertices.iter().copied() {
            let point = topology.vertex(vertex)?.point();
            if face_trim_location(topology, first, point, tolerance)? == TrimLocation::Inside {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "distinct coplanar BREP faces overlap",
                ));
            }
        }
        return Ok(());
    }
    for (source, target, target_plane) in [
        (first, second, *second_plane),
        (second, first, *first_plane),
    ] {
        for edge_id in topology.face_edges(source)? {
            let edge = topology.edge(edge_id)?;
            let [start_id, end_id] = edge.vertices();
            let start = topology.vertex(start_id)?.point();
            let end = topology.vertex(end_id)?.point();
            let direction = end - start;
            let denominator = direction.dot(target_plane.normal());
            if denominator.abs() <= tolerance.angular {
                continue;
            }
            let parameter =
                (target_plane.origin() - start).dot(target_plane.normal()) / denominator;
            if parameter < -tolerance.parameter || parameter > 1.0 + tolerance.parameter {
                continue;
            }
            let point = start + direction * parameter.clamp(0.0, 1.0);
            let location = face_trim_location(topology, target, point, tolerance)?;
            if location != TrimLocation::Outside
                && !allowed_face_contact(
                    topology,
                    &first_edges,
                    &second_edges,
                    &first_vertices,
                    &second_vertices,
                    point,
                    tolerance.linear,
                )
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "polyhedral BREP faces intersect away from shared topology",
                ));
            }
        }
    }
    Ok(())
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
