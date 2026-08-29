use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::{
    Axis3, Compound, Curve2, DiagnosticKind, FaceHandle, Kernel, KernelError, KernelResult,
    ModelTolerance, PersistentId, Plane3, PolyhedralMesh, Profile, RigidTransform3,
    TessellationOptions, TrimmedCurve2, Wire2,
};

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

fn on_plane(plane: Plane3, point: Point2) -> Point3 {
    plane.origin() + plane.u() * point.x + plane.v() * point.y
}

fn rotate(point: Point3, axis: Axis3, angle: f64) -> Point3 {
    let direction = axis.direction();
    let relative = point - axis.origin();
    axis.origin()
        + relative * angle.cos()
        + direction.cross(relative) * angle.sin()
        + direction * direction.dot(relative) * (1.0 - angle.cos())
}

fn publish(mesh: PolyhedralMesh, tolerance: ModelTolerance) -> KernelResult<Compound> {
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.merged_polyhedron(&mesh)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

fn publish_torus(
    center: Point3,
    x_major: crate::Vec3,
    y_major: crate::Vec3,
    minor_radius: f64,
    tolerance: ModelTolerance,
) -> KernelResult<Compound> {
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.torus(center, x_major, y_major, minor_radius)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

fn distinct_values(values: impl IntoIterator<Item = f64>, tolerance: f64) -> Vec<f64> {
    let mut result = Vec::<f64>::new();
    for value in values {
        if !result
            .iter()
            .any(|candidate| (candidate - value).abs() <= tolerance)
        {
            result.push(value);
        }
    }
    result.sort_by(f64::total_cmp);
    result
}

/// Promote the two ubiquitous full-turn cases to exact Euler constructions:
/// a circular section becomes a torus, while an axis-aligned rectangular
/// section becomes a cylinder or annular cylinder. More general sections keep
/// using the faceted revolution path below.
fn exact_full_revolution(
    profile: &Profile,
    plane: Plane3,
    axis: Axis3,
) -> KernelResult<Option<Compound>> {
    if !profile.holes().is_empty() {
        return Ok(None);
    }
    let tolerance = profile.tolerance();
    let axis_direction = axis.direction();
    if axis_direction.dot(plane.normal()).abs() > tolerance.angular
        || plane.project(axis.origin())?.distance.abs()
            > tolerance.linear_at_scale(axis.origin().to_vec().magnitude())
    {
        return Ok(None);
    }

    if let [segment] = profile.outer().segments() {
        if let Curve2::Circle(circle) = segment.curve() {
            let section_center = on_plane(plane, circle.center());
            let center = axis.origin()
                + axis_direction * (section_center - axis.origin()).dot(axis_direction);
            let x_major = section_center - center;
            let major_radius = x_major.magnitude();
            if major_radius > circle.radius() + tolerance.linear {
                let y_major = axis_direction.cross(x_major);
                return publish_torus(center, x_major, y_major, circle.radius(), tolerance)
                    .map(Some);
            }
        }
    }

    let segments = profile.outer().segments();
    if segments.len() != 4
        || segments
            .iter()
            .any(|segment| !matches!(segment.curve(), Curve2::Line(_)))
    {
        return Ok(None);
    }
    let points = segments
        .iter()
        .map(TrimmedCurve2::start)
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .map(|point| on_plane(plane, point))
        .collect::<Vec<_>>();
    let axial = points
        .iter()
        .map(|point| (*point - axis.origin()).dot(axis_direction))
        .collect::<Vec<_>>();
    let radial_vectors = points
        .iter()
        .zip(&axial)
        .map(|(point, axial)| *point - (axis.origin() + axis_direction * *axial))
        .collect::<Vec<_>>();
    let radial_direction = radial_vectors
        .iter()
        .find(|vector| vector.magnitude() > tolerance.linear)
        .map(|vector| vector.normalize());
    let Some(radial_direction) = radial_direction else {
        return Ok(None);
    };
    if radial_vectors.iter().any(|vector| {
        vector.cross(radial_direction).magnitude() > tolerance.linear
            || vector.dot(radial_direction) < -tolerance.linear
    }) {
        return Ok(None);
    }
    let radial = radial_vectors
        .iter()
        .map(|vector| vector.dot(radial_direction))
        .collect::<Vec<_>>();
    let axial_values = distinct_values(axial.iter().copied(), tolerance.linear);
    let radial_values = distinct_values(radial.iter().copied(), tolerance.linear);
    if axial_values.len() != 2 || radial_values.len() != 2 {
        return Ok(None);
    }
    let is_corner = |point: usize| {
        axial_values
            .iter()
            .any(|value| (axial[point] - value).abs() <= tolerance.linear)
            && radial_values
                .iter()
                .any(|value| (radial[point] - value).abs() <= tolerance.linear)
    };
    if !(0..points.len()).all(is_corner) {
        return Ok(None);
    }
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        let changes_axial = (axial[index] - axial[next]).abs() > tolerance.linear;
        let changes_radial = (radial[index] - radial[next]).abs() > tolerance.linear;
        if changes_axial == changes_radial {
            return Ok(None);
        }
    }

    let inner_radius = radial_values[0];
    let outer_radius = radial_values[1];
    if outer_radius <= tolerance.linear || inner_radius < -tolerance.linear {
        return Ok(None);
    }
    let outer = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(0.0, 0.0), outer_radius)?],
        tolerance,
    )?;
    let holes = if inner_radius > tolerance.linear {
        vec![Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), inner_radius)?],
            tolerance,
        )?]
    } else {
        Vec::new()
    };
    let annulus = Profile::new(outer, holes, tolerance)?;
    let section_plane = Plane3::new(
        axis.origin() + axis_direction * axial_values[0],
        radial_direction,
        axis_direction.cross(radial_direction),
        tolerance,
    )?;
    annulus
        .extrude(
            section_plane,
            axis_direction * (axial_values[1] - axial_values[0]),
        )
        .map(Some)
}

fn publish_from(
    mut mesh: PolyhedralMesh,
    tolerance: ModelTolerance,
    parent: &Compound,
) -> KernelResult<Compound> {
    tag_source_edges(&mut mesh, parent, tolerance)?;
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.merged_polyhedron(&mesh)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [parent])
}

fn publish_from_unmerged(
    mut mesh: PolyhedralMesh,
    tolerance: ModelTolerance,
    parent: &Compound,
) -> KernelResult<Compound> {
    tag_source_edges(&mut mesh, parent, tolerance)?;
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.polyhedron(&mesh)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [parent])
}

fn point_polyline_distance(point: Point3, polyline: &[Point3]) -> f64 {
    polyline
        .windows(2)
        .map(|segment| {
            let direction = segment[1] - segment[0];
            let parameter = if direction.magnitude2() > 0.0 {
                (point - segment[0]).dot(direction) / direction.magnitude2()
            } else {
                0.0
            };
            point.distance(segment[0] + direction * parameter.clamp(0.0, 1.0))
        })
        .fold(f64::INFINITY, f64::min)
}

fn tag_source_edges(
    mesh: &mut PolyhedralMesh,
    source: &Compound,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let options = construction_tessellation(tolerance)?;
    let edges = source
        .edges()
        .map(|edge| {
            let faces = edge
                .connected_faces()?
                .into_iter()
                .map(|face| face.persistent_id())
                .collect::<BTreeSet<_>>();
            edge.tessellate(options)
                .map(|polyline| (edge.persistent_id(), faces, polyline.points))
        })
        .collect::<KernelResult<Vec<_>>>()?;
    mesh.assign_edge_sources(|first, second, adjacent_faces| {
        let adjacent_faces = adjacent_faces.iter().copied().collect::<BTreeSet<_>>();
        edges
            .iter()
            .filter(|(_, faces, polyline)| {
                (faces.len() >= 2 && faces.is_subset(&adjacent_faces))
                    || (point_polyline_distance(first, polyline) <= tolerance.linear * 10.0
                        && point_polyline_distance(second, polyline) <= tolerance.linear * 10.0)
            })
            .map(|(name, _, _)| *name)
            .collect()
    });
    Ok(())
}

fn welded_index(points: &mut Vec<Point3>, point: Point3, tolerance: f64) -> usize {
    points
        .iter()
        .position(|candidate| candidate.distance(point) <= tolerance)
        .unwrap_or_else(|| {
            points.push(point);
            points.len() - 1
        })
}

fn construction_tessellation(tolerance: ModelTolerance) -> KernelResult<TessellationOptions> {
    TessellationOptions::new(tolerance.linear.max(0.5), 0.8, 4)
}

fn construction_curve_tolerance(tolerance: ModelTolerance) -> ModelTolerance {
    ModelTolerance {
        linear: tolerance.linear.max(0.05),
        ..tolerance
    }
}

pub(crate) fn shell(
    source: &Compound,
    openings: &[FaceHandle],
    distance: f64,
) -> KernelResult<Compound> {
    let tolerance = source.tolerance();
    if !distance.is_finite() || distance.abs() <= tolerance.linear {
        return Err(invalid(
            "shell thickness must be finite and exceed model tolerance",
        ));
    }
    if source.solids().len() != 1 {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "shell currently requires exactly one solid",
        ));
    }
    let opening_names = openings
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<PersistentId>>();
    let solid = source
        .solids()
        .next()
        .ok_or_else(|| invalid("shell source contains no solid"))?;
    let mesh = solid.tessellate(construction_tessellation(tolerance)?)?;
    let source_faces = source
        .faces()
        .map(|face| (face.persistent_id(), face.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut points = Vec::<Point3>::new();
    let mut vertex_faces = Vec::<BTreeSet<PersistentId>>::new();
    let mut triangles = Vec::<[usize; 3]>::new();
    let mut triangle_sources = Vec::<PersistentId>::new();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        let name = source.face_name(*face)?;
        if opening_names.contains(&name) {
            continue;
        }
        let welded = triangle
            .map(|index| welded_index(&mut points, mesh.positions[index], tolerance.linear));
        vertex_faces.resize_with(points.len(), BTreeSet::new);
        for vertex in welded {
            vertex_faces[vertex].insert(name);
        }
        triangles.push(welded);
        triangle_sources.push(name);
    }
    if triangles.is_empty() {
        return Err(invalid("shell openings remove every boundary face"));
    }
    let vertex_normals = points
        .iter()
        .zip(&vertex_faces)
        .map(|(point, faces)| {
            let mut normals = Vec::<crate::Vec3>::new();
            for name in faces {
                let normal = source_faces
                    .get(name)
                    .ok_or_else(|| invalid("shell tessellation refers to an unknown face"))?
                    .normal_at(*point, tolerance)?
                    .normalize();
                if normals
                    .iter()
                    .all(|candidate| candidate.dot(normal) < 1.0 - tolerance.angular)
                {
                    normals.push(normal);
                }
            }
            Ok(normals)
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let mut boundary = BTreeMap::<(usize, usize), Vec<(usize, usize, PersistentId)>>::new();
    for ([a, b, c], source_name) in triangles.iter().zip(&triangle_sources) {
        for (start, end) in [(*a, *b), (*b, *c), (*c, *a)] {
            let key = if start < end {
                (start, end)
            } else {
                (end, start)
            };
            boundary
                .entry(key)
                .or_default()
                .push((start, end, *source_name));
        }
    }
    for normals in &vertex_normals {
        if normals.is_empty() {
            return Err(invalid(
                "shell boundary contains an undefined vertex normal",
            ));
        }
    }
    let offset = points
        .iter()
        .zip(&vertex_normals)
        .map(|(point, normals)| {
            let displacement = match normals.as_slice() {
                [normal] => *normal * distance,
                [first, second] => {
                    let dot = first.dot(*second);
                    let denominator = 1.0 - dot * dot;
                    if denominator <= tolerance.angular * tolerance.angular {
                        return Err(invalid("parallel shell faces have inconsistent normals"));
                    }
                    let coefficient = distance * (1.0 - dot) / denominator;
                    (*first + *second) * coefficient
                }
                normals => {
                    let basis = crate::Matrix3::from_cols(normals[0], normals[1], normals[2]);
                    let inverse = basis.transpose().invert().ok_or_else(|| {
                        invalid("shell corner face planes do not have a unique intersection")
                    })?;
                    inverse * crate::Vec3::new(distance, distance, distance)
                }
            };
            Ok(*point + displacement)
        })
        .collect::<KernelResult<Vec<_>>>()?;
    if openings.is_empty() {
        let original = triangles
            .iter()
            .zip(&triangle_sources)
            .map(|(triangle, source)| (triangle.map(|index| points[index]), vec![*source]));
        let shifted = triangles
            .iter()
            .zip(&triangle_sources)
            .map(|(triangle, source)| (triangle.map(|index| offset[index]), vec![*source]));
        let original = PolyhedralMesh::from_tagged_triangle_soup(original, tolerance)?;
        let shifted = PolyhedralMesh::from_tagged_triangle_soup(shifted, tolerance)?;
        let (mut outer, mut cavity) = if distance > 0.0 {
            (shifted, original.into_cavity())
        } else {
            (original, shifted.into_cavity())
        };
        tag_source_edges(&mut outer, source, tolerance)?;
        tag_source_edges(&mut cavity, source, tolerance)?;
        let mut kernel = Kernel::new(tolerance)?;
        let mut transaction = kernel.begin_operation();
        transaction.hollow_polyhedron(&outer, &[cavity])?;
        let committed = transaction.finish()?;
        return Compound::from_operation(kernel, committed, [source]);
    }
    let mut soup = Vec::<([Point3; 3], Vec<PersistentId>)>::new();
    for ([a, b, c], source_name) in triangles.iter().zip(&triangle_sources) {
        let original = [points[*a], points[*b], points[*c]];
        let shifted = [offset[*a], offset[*b], offset[*c]];
        if distance > 0.0 {
            soup.push((shifted, vec![*source_name]));
            soup.push(([original[0], original[2], original[1]], vec![*source_name]));
        } else {
            soup.push((original, vec![*source_name]));
            soup.push(([shifted[0], shifted[2], shifted[1]], vec![*source_name]));
        }
    }
    for uses in boundary.values().filter(|uses| uses.len() == 1) {
        let (a, b, source_name) = uses[0];
        if distance > 0.0 {
            soup.push(([offset[b], offset[a], points[a]], vec![source_name]));
            soup.push(([offset[b], points[a], points[b]], vec![source_name]));
        } else {
            soup.push(([points[b], points[a], offset[a]], vec![source_name]));
            soup.push(([points[b], offset[a], offset[b]], vec![source_name]));
        }
    }
    publish_from_unmerged(
        PolyhedralMesh::from_tagged_triangle_soup(soup, tolerance)?,
        tolerance,
        source,
    )
}

pub(crate) fn coincident_opposite_face_names(
    first: &Compound,
    second: &Compound,
) -> KernelResult<Option<(PersistentId, PersistentId)>> {
    if first.solids().len() != 1 || second.solids().len() != 1 {
        return Ok(None);
    }
    let tolerance = first.tolerance();
    let options = construction_tessellation(tolerance)?;
    for first_face in first.faces() {
        let first_boundary = face_boundary_points(first_face, options, tolerance.linear)?;
        for second_face in second.faces() {
            let second_boundary = face_boundary_points(second_face, options, tolerance.linear)?;
            if !same_point_set(&first_boundary, &second_boundary, tolerance.linear * 8.0) {
                continue;
            }
            let Some(sample) = first_boundary.first().copied() else {
                continue;
            };
            if first_face
                .normal_at(sample, tolerance)?
                .dot(second_face.normal_at(sample, tolerance)?)
                <= -1.0 + tolerance.angular
            {
                return Ok(Some((
                    first_face.persistent_id(),
                    second_face.persistent_id(),
                )));
            }
        }
    }
    Ok(None)
}

pub(crate) fn coincident_opposite_face_patch_names(
    first: &Compound,
    second: &Compound,
) -> KernelResult<Option<(bool, PersistentId, PersistentId)>> {
    if first.solids().len() != 1 || second.solids().len() != 1 {
        return Ok(None);
    }
    let tolerance = first.tolerance();
    let options = construction_tessellation(tolerance)?;
    let mut candidates = Vec::new();
    for first_face in first
        .faces()
        .filter(|face| matches!(face.surface(), super::Surface3::Plane(_)))
    {
        let first_boundary = face_boundary_points(first_face, options, tolerance.linear)?;
        let first_area = face_mesh_area(first_face, options)?;
        for second_face in second
            .faces()
            .filter(|face| matches!(face.surface(), super::Surface3::Plane(_)))
        {
            let second_boundary = face_boundary_points(second_face, options, tolerance.linear)?;
            if same_point_set(&first_boundary, &second_boundary, tolerance.linear * 8.0) {
                continue;
            }
            let Some(sample) = first_boundary.first().copied() else {
                continue;
            };
            let first_plane = match first_face.surface() {
                super::Surface3::Plane(plane) => plane,
                _ => unreachable!(),
            };
            if second_boundary.iter().any(|point| {
                first_plane
                    .project(*point)
                    .map(|projection| projection.distance.abs() > tolerance.linear * 8.0)
                    .unwrap_or(true)
            }) || first_face
                .normal_at(sample, tolerance)?
                .dot(second_face.normal_at(sample, tolerance)?)
                > -1.0 + tolerance.angular
            {
                continue;
            }
            let second_area = face_mesh_area(second_face, options)?;
            if first_area >= second_area {
                candidates.push((
                    first_area - second_area,
                    true,
                    first_face.persistent_id(),
                    second_face.persistent_id(),
                ));
            } else {
                candidates.push((
                    second_area - first_area,
                    false,
                    second_face.persistent_id(),
                    first_face.persistent_id(),
                ));
            }
        }
    }
    candidates.sort_by(|first, second| first.0.total_cmp(&second.0));
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, target_is_first, target, source)| (target_is_first, target, source)))
}

fn face_mesh_area(face: &FaceHandle, options: TessellationOptions) -> KernelResult<f64> {
    let mesh = face.tessellate(options)?;
    Ok(mesh
        .triangles
        .iter()
        .map(|triangle| {
            let [a, b, c] = *triangle;
            (mesh.positions[b] - mesh.positions[a])
                .cross(mesh.positions[c] - mesh.positions[a])
                .magnitude()
                * 0.5
        })
        .sum())
}

fn face_boundary_points(
    face: &FaceHandle,
    options: TessellationOptions,
    tolerance: f64,
) -> KernelResult<Vec<Point3>> {
    let mut points = Vec::new();
    for edge in face.boundary_edges()? {
        for point in edge.tessellate(options)?.points {
            if !points
                .iter()
                .any(|candidate: &Point3| candidate.distance(point) <= tolerance)
            {
                points.push(point);
            }
        }
    }
    Ok(points)
}

fn same_point_set(first: &[Point3], second: &[Point3], tolerance: f64) -> bool {
    first.len() == second.len()
        && first.iter().all(|point| {
            second
                .iter()
                .any(|candidate| candidate.distance(*point) <= tolerance)
        })
}

fn clip_soup(
    soup: Vec<([Point3; 3], Vec<PersistentId>)>,
    normal: crate::Vec3,
    point: Point3,
    tolerance: f64,
) -> KernelResult<Vec<([Point3; 3], Vec<PersistentId>)>> {
    let distance = |value: Point3| normal.dot(value - point);
    let mut output = Vec::new();
    let mut cuts = Vec::new();
    for (triangle, sources) in soup {
        let mut polygon = Vec::new();
        let mut intersections = Vec::new();
        for index in 0..3 {
            let start = triangle[index];
            let end = triangle[(index + 1) % 3];
            let start_distance = distance(start);
            let end_distance = distance(end);
            let start_inside = start_distance <= tolerance;
            let end_inside = end_distance <= tolerance;
            if start_inside {
                polygon.push(start);
            }
            if start_inside != end_inside {
                let parameter = start_distance / (start_distance - end_distance);
                let intersection = start + (end - start) * parameter;
                polygon.push(intersection);
                intersections.push(intersection);
            }
        }
        for index in 1..polygon.len().saturating_sub(1) {
            output.push((
                [polygon[0], polygon[index], polygon[index + 1]],
                sources.clone(),
            ));
        }
        if intersections.len() == 2 {
            cuts.push(([intersections[0], intersections[1]], sources));
        }
    }
    let mut cut_points = Vec::<Point3>::new();
    let mut cut_edges = BTreeSet::<(usize, usize)>::new();
    let mut cap_sources = BTreeSet::new();
    for ([start, end], sources) in cuts {
        cap_sources.extend(sources);
        let start = welded_index(&mut cut_points, start, tolerance);
        let end = welded_index(&mut cut_points, end, tolerance);
        if start != end {
            let edge = if start < end {
                (start, end)
            } else {
                (end, start)
            };
            cut_edges.insert(edge);
        }
    }
    let mut adjacency = BTreeMap::<usize, Vec<usize>>::new();
    for (start, end) in &cut_edges {
        adjacency.entry(*start).or_default().push(*end);
        adjacency.entry(*end).or_default().push(*start);
    }
    while let Some(start_edge) = cut_edges.iter().next().copied() {
        let mut ring = vec![start_edge.0, start_edge.1];
        cut_edges.remove(&start_edge);
        let mut previous = start_edge.0;
        let mut current = start_edge.1;
        while current != ring[0] {
            let next = adjacency
                .get(&current)
                .and_then(|neighbors| {
                    neighbors.iter().copied().find(|next| {
                        *next != previous
                            && cut_edges.contains(&(current.min(*next), current.max(*next)))
                    })
                })
                .ok_or_else(|| invalid("clipping boundary does not form closed loops"))?;
            cut_edges.remove(&(current.min(next), current.max(next)));
            previous = current;
            current = next;
            if current != ring[0] {
                ring.push(current);
            }
            if ring.len() > cut_points.len() + 1 {
                return Err(invalid("clipping boundary loop does not close"));
            }
        }
        let mut ring = ring
            .into_iter()
            .map(|index| cut_points[index])
            .collect::<Vec<_>>();
        let center = Point3::from_vec(
            ring.iter()
                .fold(crate::Vec3::new(0.0, 0.0, 0.0), |sum, point| {
                    sum + point.to_vec()
                })
                / ring.len() as f64,
        );
        let ring_normal = (0..ring.len()).fold(crate::Vec3::new(0.0, 0.0, 0.0), |sum, index| {
            sum + (ring[index] - center).cross(ring[(index + 1) % ring.len()] - center)
        });
        if ring_normal.dot(normal) < 0.0 {
            ring.reverse();
        }
        for index in 0..ring.len() {
            let triangle = [center, ring[index], ring[(index + 1) % ring.len()]];
            output.push((triangle, cap_sources.iter().copied().collect()));
        }
    }
    Ok(output)
}

pub(crate) fn split(source: &Compound, plane: Plane3, keep: &str) -> KernelResult<Compound> {
    if source.solids().len() != 1 {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "split currently requires exactly one solid",
        ));
    }
    let tolerance = source.tolerance();
    let mesh = source
        .solids()
        .next()
        .ok_or_else(|| invalid("missing solid"))?
        .tessellate(construction_tessellation(tolerance)?)?;
    let soup = mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .map(|(triangle, face)| {
            source.face_name(*face).map(|name| {
                (
                    [
                        mesh.positions[triangle[0]],
                        mesh.positions[triangle[1]],
                        mesh.positions[triangle[2]],
                    ],
                    vec![name],
                )
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let left = clip_soup(
        soup.clone(),
        plane.normal(),
        plane.origin(),
        tolerance.linear,
    )?;
    let right = clip_soup(soup, -plane.normal(), plane.origin(), tolerance.linear)?;
    let make = |triangles: Vec<([Point3; 3], Vec<PersistentId>)>| {
        if triangles.is_empty() {
            let removed = source.solids().cloned().collect::<Vec<_>>();
            source.without_solids(&removed)
        } else {
            publish_from(
                PolyhedralMesh::from_tagged_triangle_soup(triangles, tolerance)?,
                tolerance,
                source,
            )
        }
    };
    match keep {
        "left" => make(left),
        "right" => make(right),
        "both" => {
            let left = make(left)?;
            let right = make(right)?;
            left.combined(&[right])
        }
        _ => Err(invalid("split keep mode must be left, right, or both")),
    }
}

pub(crate) fn move_faces(
    source: &Compound,
    faces: &[FaceHandle],
    transform: RigidTransform3,
) -> KernelResult<Compound> {
    if source.solids().len() != 1 || faces.is_empty() {
        return Err(invalid(
            "moving faces needs one solid and at least one face",
        ));
    }
    let tolerance = source.tolerance();
    let selected = faces
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    let mesh = source
        .solids()
        .next()
        .ok_or_else(|| invalid("missing solid"))?
        .tessellate(construction_tessellation(tolerance)?)?;
    let mut moved_points = Vec::<Point3>::new();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.triangle_faces) {
        if selected.contains(&source.face_name(*face)?) {
            for index in triangle {
                let point = mesh.positions[*index];
                if !moved_points
                    .iter()
                    .any(|candidate| candidate.distance(point) <= tolerance.linear)
                {
                    moved_points.push(point);
                }
            }
        }
    }
    let soup = mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_faces)
        .map(|(triangle, face)| {
            source.face_name(*face).map(|name| {
                (
                    triangle.map(|index| {
                        let point = mesh.positions[index];
                        if moved_points
                            .iter()
                            .any(|candidate| candidate.distance(point) <= tolerance.linear)
                        {
                            transform.transform_point(point)
                        } else {
                            point
                        }
                    }),
                    vec![name],
                )
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    publish_from(
        PolyhedralMesh::from_tagged_triangle_soup(soup, tolerance)?,
        tolerance,
        source,
    )
}

pub(crate) fn replace_face(
    source: &Compound,
    target: &FaceHandle,
    replacement: &FaceHandle,
) -> KernelResult<Compound> {
    let (super::Surface3::Plane(target_plane), super::Surface3::Plane(replacement_plane)) =
        (target.surface(), replacement.surface())
    else {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "face replacement currently requires two planar faces",
        ));
    };
    let target_basis =
        crate::Matrix3::from_cols(target_plane.u(), target_plane.v(), target_plane.normal());
    let replacement_basis = crate::Matrix3::from_cols(
        replacement_plane.u(),
        replacement_plane.v(),
        replacement_plane.normal(),
    );
    let rotation = replacement_basis * target_basis.transpose();
    let translation =
        replacement_plane.origin().to_vec() - rotation * target_plane.origin().to_vec();
    let transform = RigidTransform3::new(rotation, translation, source.tolerance())?;
    move_faces(source, std::slice::from_ref(target), transform)
}

fn cap_triangles(points: &[Point3], reverse: bool) -> KernelResult<Vec<[Point3; 3]>> {
    if points.len() < 3 {
        return Err(invalid(
            "a feature cap needs at least three boundary points",
        ));
    }
    let origin = points[0];
    let normal = (1..points.len() - 1)
        .find_map(|index| {
            let value = (points[index] - origin).cross(points[index + 1] - origin);
            (value.magnitude2() > f64::EPSILON).then_some(value.normalize())
        })
        .ok_or_else(|| invalid("feature cap boundary is collinear"))?;
    let u = (points[1] - origin).normalize();
    let v = normal.cross(u);
    let mut flat = Vec::with_capacity(points.len() * 2);
    for point in points {
        let relative = *point - origin;
        flat.extend([relative.dot(u), relative.dot(v)]);
    }
    let indices = earcutr::earcut(&flat, &Vec::new(), 2);
    if indices.len() < 3 {
        return Err(invalid("feature cap triangulation failed"));
    }
    Ok(indices
        .chunks_exact(3)
        .map(|triangle| {
            let mut values = [
                points[triangle[0]],
                points[triangle[1]],
                points[triangle[2]],
            ];
            if reverse {
                values.swap(1, 2);
            }
            values
        })
        .collect())
}

fn resample_closed(points: &[Point3], count: usize) -> KernelResult<Vec<Point3>> {
    if points.len() < 3 || count < 3 {
        return Err(invalid(
            "a loft section needs at least three boundary points",
        ));
    }
    let mut cumulative = vec![0.0];
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        cumulative.push(cumulative[index] + points[index].distance(points[next]));
    }
    let perimeter = *cumulative
        .last()
        .ok_or_else(|| invalid("loft section has no perimeter"))?;
    if perimeter <= f64::EPSILON {
        return Err(invalid("loft section perimeter is zero"));
    }
    let mut output = Vec::with_capacity(count);
    for sample in 0..count {
        let distance = perimeter * sample as f64 / count as f64;
        let segment = cumulative
            .windows(2)
            .position(|range| distance >= range[0] && distance <= range[1])
            .unwrap_or(points.len() - 1);
        let length = cumulative[segment + 1] - cumulative[segment];
        let parameter = if length > 0.0 {
            (distance - cumulative[segment]) / length
        } else {
            0.0
        };
        let next = (segment + 1) % points.len();
        output.push(points[segment] + (points[next] - points[segment]) * parameter);
    }
    Ok(output)
}

fn polygon_normal(points: &[Point3]) -> crate::Vec3 {
    let center = Point3::from_vec(
        points
            .iter()
            .map(|point| point.to_vec())
            .fold(crate::Vec3::new(0.0, 0.0, 0.0), |sum, point| sum + point)
            / points.len().max(1) as f64,
    );
    (0..points.len()).fold(crate::Vec3::new(0.0, 0.0, 0.0), |normal, index| {
        normal + (points[index] - center).cross(points[(index + 1) % points.len()] - center)
    })
}

impl Profile {
    /// Revolve a closed profile around an arbitrary 3D axis. The published
    /// result is a certified faceted BREP constructed entirely through Euler
    /// operators; analytic surface promotion can refine it without changing
    /// this high-level API.
    pub fn revolve(&self, plane: Plane3, axis: Axis3, angle: f64) -> KernelResult<Compound> {
        if !angle.is_finite() || angle.abs() <= self.tolerance().parameter {
            return Err(invalid("revolution angle must be finite and nonzero"));
        }
        if angle.abs() > std::f64::consts::TAU + self.tolerance().parameter {
            return Err(invalid("revolution angle may not exceed one full turn"));
        }
        if !self.holes().is_empty() {
            let outer = Profile::new(self.outer().clone(), [], self.tolerance())?;
            let mut result = outer.revolve(plane, axis, angle)?;
            for hole in self.holes() {
                let tool = Profile::new(hole.clone(), [], self.tolerance())?
                    .revolve(plane, axis, angle)?;
                result = result.difference(&tool)?;
            }
            return Ok(result);
        }
        let full = (angle.abs() - std::f64::consts::TAU).abs() <= self.tolerance().parameter;
        if full {
            if let Some(exact) = exact_full_revolution(self, plane, axis)? {
                return Ok(exact);
            }
        }
        let boundary = self
            .outer()
            .polyline(construction_curve_tolerance(self.tolerance()))?;
        let base = boundary
            .into_iter()
            .map(|point| on_plane(plane, point))
            .collect::<Vec<_>>();
        let steps = (angle.abs() / 0.1).ceil().max(3.0) as usize;
        let layers = if full { steps } else { steps + 1 };
        let mut rings = Vec::with_capacity(layers);
        for step in 0..layers {
            let parameter = step as f64 / steps as f64;
            rings.push(
                base.iter()
                    .map(|point| rotate(*point, axis, angle * parameter))
                    .collect::<Vec<_>>(),
            );
        }
        let mut triangles = Vec::new();
        let angular_segments = if full { layers } else { layers - 1 };
        for layer in 0..angular_segments {
            let next = (layer + 1) % layers;
            for index in 0..base.len() {
                let successor = (index + 1) % base.len();
                triangles.push([
                    rings[layer][index],
                    rings[layer][successor],
                    rings[next][successor],
                ]);
                triangles.push([
                    rings[layer][index],
                    rings[next][successor],
                    rings[next][index],
                ]);
            }
        }
        if !full {
            triangles.extend(cap_triangles(&rings[0], true)?);
            let end = rings
                .last()
                .ok_or_else(|| invalid("revolution produced no angular layers"))?;
            triangles.extend(cap_triangles(end, false)?);
        }
        publish(
            PolyhedralMesh::from_triangle_soup(triangles, self.tolerance())?,
            self.tolerance(),
        )
    }

    /// Loft this profile through subsequent planar sections. Sections are
    /// resampled by arc length and published as one certified Euler-built BREP.
    pub fn loft<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
    ) -> KernelResult<Compound> {
        self.loft_with_alignment(plane, sections, true)
    }

    pub fn loft_with_alignment<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        align_sections: bool,
    ) -> KernelResult<Compound> {
        let mut profiles = vec![(plane, self)];
        profiles.extend(sections);
        if profiles.len() < 2 {
            return Err(invalid("a loft needs at least two profiles"));
        }
        for (_, profile) in &profiles {
            if profile.tolerance() != self.tolerance() {
                return Err(invalid("loft profiles must use the same model tolerance"));
            }
        }
        let hole_count = profiles[0].1.holes().len();
        if profiles
            .iter()
            .any(|(_, profile)| profile.holes().len() != hole_count)
        {
            return Err(invalid(
                "all loft sections must have the same number of holes",
            ));
        }
        if hole_count > 0 {
            let outers = profiles
                .iter()
                .map(|(section_plane, profile)| {
                    Profile::new(profile.outer().clone(), [], self.tolerance())
                        .map(|profile| (*section_plane, profile))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let mut result = outers[0].1.loft_with_alignment(
                outers[0].0,
                outers
                    .iter()
                    .skip(1)
                    .map(|(plane, profile)| (*plane, profile))
                    .collect::<Vec<_>>(),
                align_sections,
            )?;
            for hole_index in 0..hole_count {
                let holes = profiles
                    .iter()
                    .map(|(section_plane, profile)| {
                        Profile::new(profile.holes()[hole_index].clone(), [], self.tolerance())
                            .map(|profile| (*section_plane, profile))
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                let tool = holes[0].1.loft_with_alignment(
                    holes[0].0,
                    holes
                        .iter()
                        .skip(1)
                        .map(|(plane, profile)| (*plane, profile))
                        .collect::<Vec<_>>(),
                    align_sections,
                )?;
                result = result.difference(&tool)?;
            }
            return Ok(result);
        }
        let sampling_tolerance = construction_curve_tolerance(self.tolerance());
        let sampled = profiles
            .iter()
            .map(|(section_plane, profile)| {
                profile.outer().polyline(sampling_tolerance).map(|points| {
                    points
                        .into_iter()
                        .map(|point| on_plane(*section_plane, point))
                        .collect::<Vec<_>>()
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let count = sampled.iter().map(Vec::len).max().unwrap_or(0).max(3);
        let mut layers = sampled
            .iter()
            .map(|points| resample_closed(points, count))
            .collect::<KernelResult<Vec<_>>>()?;
        let reference_normal = polygon_normal(&layers[0]);
        for layer in layers.iter_mut().skip(1) {
            if polygon_normal(layer).dot(reference_normal) < 0.0 {
                layer.reverse();
            }
        }
        if align_sections {
            for layer in 1..layers.len() {
                let shift = (0..count)
                    .min_by(|first, second| {
                        let cost = |shift: usize| {
                            (0..count)
                                .map(|index| {
                                    layers[layer - 1][index]
                                        .distance2(layers[layer][(index + shift) % count])
                                })
                                .sum::<f64>()
                        };
                        cost(*first).total_cmp(&cost(*second))
                    })
                    .unwrap_or(0);
                layers[layer].rotate_left(shift);
            }
        }
        let mut triangles = Vec::new();
        for layer in 0..layers.len() - 1 {
            for index in 0..count {
                let next = (index + 1) % count;
                triangles.push([
                    layers[layer][index],
                    layers[layer][next],
                    layers[layer + 1][next],
                ]);
                triangles.push([
                    layers[layer][index],
                    layers[layer + 1][next],
                    layers[layer + 1][index],
                ]);
            }
        }
        triangles.extend(cap_triangles(&layers[0], true)?);
        let end = layers
            .last()
            .ok_or_else(|| invalid("loft produced no sections"))?;
        triangles.extend(cap_triangles(end, false)?);
        publish(
            PolyhedralMesh::from_triangle_soup(triangles, self.tolerance())?,
            self.tolerance(),
        )
    }
}
