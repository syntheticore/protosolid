//! Fillets whose contact rails leave a support face: across the far edge of
//! a face narrower than the blend, or through a hole in its band.
//!
//! The local rewrite of `edge_blend` needs both rails inside their faces. Here
//! the material between the edge and the rolling ball is built as a solid
//! wedge instead and united with, or subtracted from, the body. The body's
//! other faces then trim the blend wherever they reach into it.
//!
//! A united wedge can bridge a gap of its support face, such as a bore or the
//! drop past a narrow ledge. What stands over the gap is taken out again,
//! which continues the gap's walls up to the blend. A subtracted wedge that
//! undercuts material standing on its support is refilled the same way.

use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::history::BlendRole;
use crate::{
    topology_error, unsupported, Axis3, BooleanKind, CoedgeId, Compound, Curve3, DerivationRole,
    EdgeHandle, EdgeId, FaceHandle, FaceId, KernelResult, LoopKind, ModelTolerance, PersistentId,
    Plane3, Point2, Point3, Profile, Resolution, SplitKeep, Surface3, Topology, TrimmedCurve2,
    Vec2, Vec3, VertexId, Wire2,
};

/// The dihedral of an edge at one of its points.
struct EdgeSide {
    faces: [FaceId; 2],
    concave: bool,
    normals: [Vec3; 2],
    /// Directions into each face, across the edge.
    inward: [Vec3; 2],
}

/// The trace of a support in the plane across the edge.
enum Trace {
    Line { inward: Vec2, normal: Vec2 },
    /// `outside` is 1 where the material normal points away from the center.
    Circle { center: Point2, radius: f64, outside: f64 },
}

/// The wedge's cross-section: from the edge along the first support to its
/// contact, around the ball, and back along the second support.
struct Section {
    profile: Profile,
    center: Point2,
    /// The farthest the profile reaches from the edge.
    reach: f64,
}

/// What one edge contributes to a tool.
struct Part {
    edge: PersistentId,
    faces: [FaceId; 2],
    /// The wedge owning the faces below.
    solid: Compound,
    blend: Vec<FaceHandle>,
    strips: [Vec<FaceHandle>; 2],
    /// The contact rails, where the ball leaves either support.
    rails: Vec<EdgeHandle>,
    stretch: Stretch,
    /// A bound on the wedge's extent across its edge.
    reach: f64,
}

/// Widens the footprint of a wedge on a planar support by half, away from
/// its edge.
#[derive(Clone, Copy)]
enum Stretch {
    Line { origin: Point3, inward: [Vec3; 2] },
    Circle { center: Point3, axis: Vec3, radius: f64 },
}

impl Stretch {
    fn apply(self, side: usize, point: Point3) -> Point3 {
        match self {
            Self::Line { origin, inward } => {
                point + inward[side] * ((point - origin).dot(inward[side]) * 0.5)
            }
            Self::Circle { center, axis, radius } => {
                let offset = point - center;
                let radial = offset - axis * offset.dot(axis);
                let distance = radial.magnitude();
                point + radial * ((distance - radius) * 0.5 / distance)
            }
        }
    }
}

struct Tool {
    solid: Compound,
    concave: bool,
    parts: Vec<Part>,
}

/// How a wedge ends at a vertex of its edge.
#[derive(Clone, Copy)]
enum End {
    /// Squarely, where a tangent neighbor continues it or the edge is closed.
    Square,
    /// On the plane of the face the edge runs into.
    Plane(Plane3),
}

pub(crate) fn fillet_overflow(
    source: &Compound,
    edges: &[EdgeHandle],
    radius: f64,
    follow_tangent_edges: bool,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let mut selected = edges.iter().map(EdgeHandle::topology_id).collect::<BTreeSet<_>>();
    if follow_tangent_edges {
        follow_tangents(topology, &mut selected, tolerance)?;
    }
    let mut tools = Vec::new();
    let mut corners = BTreeMap::<VertexId, (bool, Vec<Part>)>::new();
    for edge in &selected {
        let side = edge_side(topology, *edge, 0.5, tolerance)?;
        let value = topology.edge(*edge)?;
        let mut ends = [End::Square; 2];
        let mut reentrant = [false; 2];
        if value.vertices()[0] != value.vertices()[1] {
            for index in 0..2 {
                (ends[index], reentrant[index]) =
                    wedge_end(topology, *edge, index, &side, &selected, tolerance)?;
            }
        }
        let part = wedge(topology, *edge, radius, ends, [false; 2], tolerance)?;
        tools.push(Tool { solid: part.solid.clone(), concave: side.concave, parts: vec![part] });
        for index in (0..2).filter(|index| reentrant[*index]) {
            let mut extended = [false; 2];
            extended[index] = true;
            corners.entry(value.vertices()[index]).or_insert((side.concave, Vec::new())).1
                .push(wedge(topology, *edge, radius, ends, extended, tolerance)?);
        }
    }
    // Blends meeting across an edge of the other convexity share the
    // intersection of their continuations. It follows the wedges themselves,
    // whose ends it closes.
    for (concave, mut corner) in corners.into_values() {
        let second = corner.pop().unwrap();
        let first = corner.pop().unwrap();
        tools.push(Tool {
            solid: first.solid.intersection(&second.solid)?,
            concave,
            parts: vec![first, second],
        });
    }

    let mut result = source.clone();
    for tool in &tools {
        let kind = if tool.concave { BooleanKind::Union } else { BooleanKind::Difference };
        let mut next = result.boolean(&tool.solid, kind)?;
        // A support strip that survives lies on no face of the body: the
        // wedge bridges a gap of that support there.
        let bridges = |body: &Compound, part: &Part, side: usize| {
            part.strips[side].iter().any(|face| !descendants(body, face).is_empty())
        };
        for part in &tool.parts {
            for side in 0..2 {
                if bridges(&next, part, side) {
                    next = clear_gaps(source, &result, &next, tool, part, side)?;
                }
            }
        }
        if tool.parts.iter().any(|part| (0..2).any(|side| bridges(&next, part, side))) {
            return Err(unsupported("blend bridges a gap of its support face that the face's outline does not close"));
        }
        result = next;
    }
    // A ball too large to touch either face of its edge rolls on neither.
    for tool in &tools {
        if let [part] = &tool.parts[..] {
            if part.rails.iter().all(|rail| matches!(result.resolve(rail), Resolution::Deleted | Resolution::Unrelated)) {
                return Err(unsupported("blend radius leaves no contact with either face of its edge"));
            }
        }
    }
    let generated = tools.iter().flat_map(|tool| &tool.parts).flat_map(|part| {
        part.blend.iter().flat_map(|face| descendants(&result, face))
            .map(|face| (face.persistent_id(), part.edge))
    }).collect::<Vec<_>>();
    Ok(result.with_generated(generated, DerivationRole::Fillet(BlendRole::BlendFace)))
}

fn descendants(result: &Compound, face: &FaceHandle) -> Vec<FaceHandle> {
    match result.resolve(face) {
        Resolution::Preserved(face) | Resolution::Modified(face) | Resolution::Merged(face) => vec![face],
        Resolution::Split(faces) => faces,
        Resolution::Deleted | Resolution::Unrelated => Vec::new(),
    }
}

/// The unit tangent of `edge` at a vertex, pointing away from the edge.
fn outgoing_tangent(topology: &Topology, edge: EdgeId, vertex: VertexId) -> KernelResult<Vec3> {
    let value = topology.edge(edge)?;
    let range = value.range();
    let (parameter, sign) = if value.vertices()[0] == vertex {
        (range.start, -range.span().signum())
    } else {
        (range.end, range.span().signum())
    };
    Ok(value.curve().evaluate(parameter)?.first_derivative.normalize() * sign)
}

fn continues_tangentially(
    topology: &Topology,
    first: EdgeId,
    second: EdgeId,
    vertex: VertexId,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let first = outgoing_tangent(topology, first, vertex)?;
    let second = outgoing_tangent(topology, second, vertex)?;
    Ok(first.dot(second) < 0.0 && first.cross(second).magnitude() <= tolerance.angular * 8.0)
}

fn follow_tangents(
    topology: &Topology,
    selected: &mut BTreeSet<EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let mut pending = selected.iter().copied().collect::<Vec<_>>();
    while let Some(edge) = pending.pop() {
        let concave = edge_side(topology, edge, 0.5, tolerance)?.concave;
        for vertex in topology.edge(edge)?.vertices() {
            for next in topology.vertex_edges(vertex)? {
                if selected.contains(&next)
                    || !continues_tangentially(topology, edge, next, vertex, tolerance)?
                    || !edge_side(topology, next, 0.5, tolerance).is_ok_and(|side| side.concave == concave)
                {
                    continue;
                }
                selected.insert(next);
                pending.push(next);
            }
        }
    }
    Ok(())
}

fn edge_side(
    topology: &Topology,
    edge: EdgeId,
    fraction: f64,
    tolerance: ModelTolerance,
) -> KernelResult<EdgeSide> {
    let value = topology.edge(edge)?;
    let &[first, second] = &value.coedges[..] else {
        return Err(unsupported("blend edge must separate two faces"));
    };
    let range = value.range();
    let sample = value.curve().evaluate(range.start + range.span() * fraction)?;
    let tangent = sample.first_derivative.normalize();
    let at = |id: CoedgeId| -> KernelResult<(FaceId, Vec3, Vec3)> {
        let coedge = topology.coedge(id)?;
        let face = topology.loop_(coedge.loop_id())?.face();
        let normal = topology.face(face)?.oriented_normal_near(sample.point, tolerance)?.normalize();
        // Material lies to the left of a boundary traversal.
        Ok((face, normal, normal.cross(tangent * coedge.range().span().signum()).normalize()))
    };
    let (first, second) = (at(first)?, at(second)?);
    let turn = first.2.dot(second.1);
    if first.0 == second.0 || turn.abs() <= tolerance.angular {
        return Err(unsupported("blend edge has no well-defined convex or concave dihedral"));
    }
    Ok(EdgeSide {
        faces: [first.0, second.0],
        concave: turn > 0.0,
        normals: [first.1, second.1],
        inward: [first.2, second.2],
    })
}

/// How the wedge of `edge` ends at its vertex `index`, and whether it meets
/// another blend there across an edge of the other convexity.
fn wedge_end(
    topology: &Topology,
    edge: EdgeId,
    index: usize,
    side: &EdgeSide,
    selected: &BTreeSet<EdgeId>,
    tolerance: ModelTolerance,
) -> KernelResult<(End, bool)> {
    let vertex = topology.edge(edge)?.vertices()[index];
    let incident = topology.vertex_edges(vertex)?;
    let others = incident.iter().copied().filter(|other| *other != edge && selected.contains(other))
        .collect::<Vec<_>>();
    if others.len() > 1 {
        return Err(unsupported("overflowing blends do not yet meet at corners of three edges"));
    }
    if let [other] = others[..] {
        if continues_tangentially(topology, edge, other, vertex, tolerance)? {
            return Ok((End::Square, false));
        }
    }
    let ending = topology.vertex_faces(vertex)?.into_iter().filter(|face| !side.faces.contains(face))
        .collect::<Vec<_>>();
    let [ending] = ending[..] else {
        return Err(unsupported("overflowing blend must end on a single face"));
    };
    let Surface3::Plane(plane) = topology.face(ending)?.surface() else {
        return Err(unsupported("overflowing blend must end on a planar face"));
    };
    let mut reentrant = false;
    if let [other] = others[..] {
        let third = incident.iter().copied().find(|third| *third != edge && *third != other);
        let (Some(third), 3) = (third, incident.len()) else {
            return Err(unsupported("overflowing blends must meet at a corner of three faces"));
        };
        if edge_side(topology, other, 0.5, tolerance)?.concave != side.concave {
            return Err(unsupported("overflowing blends of mixed convexity do not yet meet at a corner"));
        }
        reentrant = edge_side(topology, third, 0.5, tolerance)?.concave != side.concave;
    }
    Ok((End::Plane(*plane), reentrant))
}

/// The wedge between `edge` and its rolling ball, with the faces it lays on
/// the ball and on either support. An `extended` end continues well past its
/// vertex instead of ending there.
fn wedge(
    topology: &Topology,
    edge: EdgeId,
    radius: f64,
    ends: [End; 2],
    extended: [bool; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Part> {
    let value = topology.edge(edge)?;
    let range = value.range();
    let start = value.curve().evaluate(range.start)?;
    let side = edge_side(topology, edge, 0.0, tolerance)?;
    let surfaces = [
        topology.face(side.faces[0])?.surface().clone(),
        topology.face(side.faces[1])?.surface().clone(),
    ];
    let direction = start.first_derivative.normalize() * range.span().signum();
    let (solid, on_ball, stretch, reach) = match value.curve() {
        Curve3::Line(_) => {
            let x = side.inward[0];
            let y = direction.cross(x);
            let flat = |vector: Vec3| Vec2::new(vector.dot(x), vector.dot(y));
            let mut traces = Vec::new();
            for index in 0..2 {
                traces.push(match &surfaces[index] {
                    Surface3::Plane(_) => Trace::Line { inward: flat(side.inward[index]), normal: flat(side.normals[index]) },
                    Surface3::Cylinder(cylinder)
                        if cylinder.axis().normalize().cross(direction).magnitude() <= tolerance.angular =>
                    {
                        let center = Point2::from_vec(flat(cylinder.origin() - start.point));
                        Trace::Circle {
                            center,
                            radius: cylinder.radius(),
                            outside: flat(side.normals[index]).dot(-center.to_vec()).signum(),
                        }
                    }
                    _ => return Err(unsupported("overflowing blend along a line needs supports extruded along it")),
                });
            }
            let section = section(Point2::origin(), [&traces[0], &traces[1]], radius, side.concave, tolerance)?;
            let length = value.curve().evaluate(range.end)?.point.distance(start.point);
            let mut overshoot = [0.0; 2];
            let mut cuts = Vec::new();
            for index in 0..2 {
                let End::Plane(plane) = ends[index] else { continue };
                let along = plane.normal().dot(direction).abs();
                let across = plane.normal().cross(direction).magnitude();
                if along <= 1.0e-3 {
                    return Err(unsupported("overflowing blend ends on a face along its edge"));
                }
                if extended[index] {
                    overshoot[index] = section.reach * (across / along + 4.0);
                } else if across > tolerance.angular {
                    overshoot[index] = section.reach * (across / along + 1.0);
                    cuts.push(plane);
                }
            }
            let plane = Plane3::new(start.point - direction * overshoot[0], x, y, tolerance)?;
            let mut solid = section.profile.extrude(plane, direction * (length + overshoot[0] + overshoot[1]))?;
            let middle = start.point + direction * (length * 0.5);
            for cut in cuts {
                let keep = if (middle - cut.origin()).dot(cut.normal()) < 0.0 { SplitKeep::Left } else { SplitKeep::Right };
                solid = solid.split(cut, keep)?;
            }
            let center = start.point + x * section.center.x + y * section.center.y;
            (solid, Box::new(move |point: Point3| {
                let offset = point - center;
                (offset - direction * offset.dot(direction)).magnitude()
            }) as Box<dyn Fn(Point3) -> f64>, Stretch::Line { origin: start.point, inward: side.inward }, section.reach)
        }
        Curve3::Circle(circle) => {
            let axis = circle.normal().normalize();
            let origin = circle.center();
            let radial = (start.point - origin).normalize();
            let flat = |vector: Vec3| Vec2::new(vector.dot(radial), vector.dot(axis));
            let on_axis = |point: Point3, normal: Vec3| {
                let offset = point - origin;
                normal.normalize().cross(axis).magnitude() <= tolerance.angular
                    && (offset - axis * offset.dot(axis)).magnitude() <= tolerance.linear_at_scale(circle.radius())
            };
            let corner = Point2::new(circle.radius(), 0.0);
            let mut traces = Vec::new();
            for index in 0..2 {
                let line = Trace::Line { inward: flat(side.inward[index]), normal: flat(side.normals[index]) };
                let circle = |center: Point2, radius: f64| Trace::Circle {
                    center, radius, outside: flat(side.normals[index]).dot(corner - center).signum(),
                };
                traces.push(match &surfaces[index] {
                    Surface3::Plane(plane) if plane.normal().cross(axis).magnitude() <= tolerance.angular => line,
                    Surface3::Cylinder(cylinder) if on_axis(cylinder.origin(), cylinder.axis()) => line,
                    Surface3::Cone(cone) if on_axis(cone.apex(), cone.axis()) => line,
                    Surface3::Sphere(sphere) if on_axis(sphere.center(), axis) =>
                        circle(Point2::new(0.0, (sphere.center() - origin).dot(axis)), sphere.radius()),
                    Surface3::Torus(torus) if on_axis(torus.center(), torus.normal()) =>
                        circle(Point2::new(torus.major_radius(), (torus.center() - origin).dot(axis)), torus.minor_radius()),
                    _ => return Err(unsupported("overflowing blend along a circle needs supports revolved about its axis")),
                });
            }
            let section = section(corner, [&traces[0], &traces[1]], radius, side.concave, tolerance)?;
            let closed = value.vertices()[0] == value.vertices()[1];
            for end in ends {
                // Only a meridian plane ends the revolution squarely.
                if let End::Plane(plane) = end {
                    if plane.normal().dot(axis).abs() > tolerance.angular
                        || (origin - plane.origin()).dot(plane.normal()).abs() > tolerance.linear_at_scale(circle.radius())
                    {
                        return Err(unsupported("overflowing blend along an arc must end on a plane through its axis"));
                    }
                }
            }
            if extended.contains(&true) {
                return Err(unsupported("overflowing blends along arcs do not yet meet at reentrant corners"));
            }
            let angle = if closed {
                std::f64::consts::TAU
            } else {
                range.span().abs() * direction.dot(axis.cross(radial)).signum()
            };
            let plane = Plane3::new(origin, radial, axis, tolerance)?;
            let solid = section.profile.revolve(plane, Axis3::new(origin, axis)?, angle)?;
            let (ring, height) = (section.center.x, section.center.y);
            (solid, Box::new(move |point: Point3| {
                let offset = point - origin;
                let along = offset.dot(axis);
                ((offset - axis * along).magnitude() - ring).hypot(along - height)
            }) as Box<dyn Fn(Point3) -> f64>, Stretch::Circle { center: origin, axis, radius: circle.radius() }, section.reach)
        }
        _ => return Err(unsupported("overflowing blend needs a straight or circular edge")),
    };

    // Identify the wedge's faces by the surfaces carrying all their edges.
    let allowed = tolerance.linear_at_scale(radius.max(start.point.to_vec().magnitude())) * 64.0;
    let mut blend = Vec::new();
    let mut strips = [Vec::new(), Vec::new()];
    for face in solid.faces() {
        let mut points = Vec::new();
        for boundary in face.boundary_edges()? {
            points.push(boundary.curve().evaluate(boundary.range()?.middle())?.point);
        }
        if points.iter().all(|point| (on_ball(*point) - radius).abs() <= allowed) {
            blend.push(face.clone());
            continue;
        }
        for index in 0..2 {
            let mut on_support = true;
            for point in &points {
                on_support &= surfaces[index].project(*point, tolerance)?.distance <= allowed;
            }
            if on_support {
                strips[index].push(face.clone());
            }
        }
    }
    if blend.is_empty() || strips.iter().any(Vec::is_empty) {
        return Err(topology_error("blend wedge lost its face on the ball or on a support"));
    }
    let mut rails = Vec::new();
    for face in &blend {
        for boundary in face.boundary_edges()? {
            for strip in strips.iter().flatten() {
                if strip.boundary_edges()?.contains(&boundary) {
                    rails.push(boundary.clone());
                }
            }
        }
    }
    Ok(Part { edge: value.name(), faces: side.faces, solid, blend, strips, rails, stretch, reach })
}

/// Roll a ball of `radius` into the corner of two traces through `corner`.
fn section(
    corner: Point2,
    traces: [&Trace; 2],
    radius: f64,
    concave: bool,
    tolerance: ModelTolerance,
) -> KernelResult<Section> {
    // The ball's center lies on both traces offset to its side: into the
    // material of a convex edge, away from it on a concave one.
    let away = if concave { 1.0 } else { -1.0 };
    let offset = |trace: &Trace| match *trace {
        Trace::Line { inward, normal } => Trace::Line { inward, normal: normal * (away * radius) },
        Trace::Circle { center, radius: trace_radius, outside } =>
            Trace::Circle { center, radius: trace_radius + away * outside * radius, outside },
    };
    let no_ball = || unsupported("blend radius does not fit between its supports");
    let candidates: Vec<Point2> = match (offset(traces[0]), offset(traces[1])) {
        (Trace::Line { inward: first, normal: shift }, Trace::Line { inward: second, normal: other }) => {
            let determinant = first.perp_dot(second);
            if determinant.abs() <= tolerance.angular { return Err(no_ball()); }
            vec![corner + shift + first * ((other - shift).perp_dot(second) / determinant)]
        }
        (Trace::Line { inward, normal }, Trace::Circle { center, radius, .. })
        | (Trace::Circle { center, radius, .. }, Trace::Line { inward, normal }) => {
            let start = corner + normal;
            let foot = start + inward * (center - start).dot(inward);
            let half = radius * radius - foot.distance2(center);
            if radius <= 0.0 || half < 0.0 { return Err(no_ball()); }
            vec![foot + inward * half.sqrt(), foot - inward * half.sqrt()]
        }
        (Trace::Circle { center: first, radius: first_radius, .. }, Trace::Circle { center: second, radius: second_radius, .. }) => {
            let between = second - first;
            let distance = between.magnitude();
            if first_radius <= 0.0 || second_radius <= 0.0 || distance <= tolerance.linear { return Err(no_ball()); }
            let along = (distance * distance + first_radius * first_radius - second_radius * second_radius) / (2.0 * distance);
            let half = first_radius * first_radius - along * along;
            if half < 0.0 { return Err(no_ball()); }
            let unit = between / distance;
            let across = Vec2::new(-unit.y, unit.x) * half.sqrt();
            vec![first + unit * along + across, first + unit * along - across]
        }
    };
    let center = candidates.into_iter()
        .min_by(|first, second| first.distance2(corner).total_cmp(&second.distance2(corner)))
        .unwrap();
    let contacts = traces.map(|trace| match *trace {
        Trace::Line { normal, .. } => center - normal * (away * radius),
        Trace::Circle { center: trace_center, radius: trace_radius, .. } =>
            trace_center + (center - trace_center).normalize() * trace_radius,
    });
    let along = |trace: &Trace, from: Point2, to: Point2| match *trace {
        Trace::Line { .. } => TrimmedCurve2::line(from, to),
        Trace::Circle { center, radius, .. } => TrimmedCurve2::arc_through(
            from, center + ((from - center) + (to - center)).normalize() * radius, to,
        ),
    };
    let middle = center + ((contacts[0] - center) + (contacts[1] - center)).normalize() * radius;
    let wire = Wire2::new([
        along(traces[0], corner, contacts[0])?,
        TrimmedCurve2::arc_through(contacts[0], middle, contacts[1])?,
        along(traces[1], contacts[1], corner)?,
    ], tolerance)?;
    Ok(Section {
        profile: Profile::new(wire, [], tolerance)?,
        center,
        reach: contacts[0].distance(corner).max(contacts[1].distance(corner)) * 1.5,
    })
}

/// Remove what the united wedge laid over the gaps of its support `side`,
/// or restore what the subtracted wedge took from beneath them, in `bridged`.
fn clear_gaps(
    source: &Compound,
    before: &Compound,
    bridged: &Compound,
    tool: &Tool,
    part: &Part,
    side: usize,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let face = topology.face(part.faces[side])?;
    let Surface3::Plane(support) = face.surface() else {
        return Err(unsupported("blend bridges a gap of a curved support face"));
    };
    // Columns stand on the support, on the wedge's side of it.
    let normal = support.normal() * face.orientation.sign() * if tool.concave { 1.0 } else { -1.0 };
    let u = support.u().normalize();
    let plane = Plane3::new(support.origin(), u, normal.cross(u), tolerance)?;
    let region = planar_profile(topology, part.faces[side], plane, &|point| point, tolerance)?;
    let mut cleared = bridged.clone();
    for strip in &part.strips[side] {
        // Reach past the contact rail, where the wedge thins out to nothing.
        let footprint = planar_profile(
            part.solid.topology(), strip.topology_id(), plane, &|point| part.stretch.apply(side, point), tolerance,
        )?;
        for gap in footprint.boolean_planar(&region, BooleanKind::Difference)? {
            if tool.concave {
                // At a grazing contact the excess wedge can be thinner than
                // model tolerance. Cut across the support plane so the rim
                // and its extension are arranged as one wall. A through
                // column is safe only when it removes no preexisting material.
                let through = Plane3::new(plane.origin() - normal * part.reach, u, plane.v(), tolerance)?;
                let column = gap.extrude(through, normal * (2.0 * part.reach))?;
                if before.intersection(&column).is_ok_and(|overlap| overlap.solids().next().is_none()) {
                    cleared = cleared.difference(&column)?;
                    continue;
                }
            }
            let column = gap.extrude(plane, normal * part.reach)?;
            let bridge = tool.solid.intersection(&column)?;
            // Undo only the wedge's change within this gap. The bridge must
            // be empty of preexisting material for an additive fillet and
            // completely filled by it for a subtractive fillet.
            let interference = if tool.concave {
                bridge.intersection(before)?
            } else {
                bridge.difference(before)?
            };
            if interference.solids().next().is_some() {
                return Err(unsupported("blend crosses a support gap where clearing it would change preexisting material"));
            }
            let kind = if tool.concave { BooleanKind::Difference } else { BooleanKind::Union };
            cleared = cleared.boolean(&bridge, kind)?;
        }
    }
    Ok(cleared)
}

/// The outline of a planar face with straight and circular edges in `plane`,
/// after moving its points by `map`.
fn planar_profile(
    topology: &Topology,
    face: FaceId,
    plane: Plane3,
    map: &dyn Fn(Point3) -> Point3,
    tolerance: ModelTolerance,
) -> KernelResult<Profile> {
    let flat = |point: Point3| -> KernelResult<Point2> {
        let (u, v) = plane.project(point)?.parameters;
        Ok(Point2::new(u, v))
    };
    let mut outer = None;
    let mut holes = Vec::new();
    for id in topology.face(face)?.loops() {
        let mut segments = Vec::new();
        for coedge in topology.loop_coedges(*id)? {
            let coedge = topology.coedge(coedge)?;
            let range = coedge.range();
            if range.span() == 0.0 {
                continue;
            }
            let curve = topology.edge(coedge.edge())?.curve();
            let at = |parameter: f64| flat(map(curve.evaluate(parameter)?.point));
            let (start, end) = (at(range.start)?, at(range.end)?);
            segments.push(match curve {
                Curve3::Line(_) => TrimmedCurve2::line(start, end)?,
                Curve3::Circle(circle) if start.distance(end) <= tolerance.linear => {
                    let center = flat(circle.center())?;
                    TrimmedCurve2::circle(center, center.distance(start))?
                }
                Curve3::Circle(_) => TrimmedCurve2::arc_through(start, at(range.middle())?, end)?,
                _ => return Err(unsupported("blend bridges a gap of a support face without straight or circular edges")),
            });
        }
        let wire = Wire2::new(segments, tolerance)?;
        match topology.loop_(*id)?.kind() {
            LoopKind::Outer => outer = Some(wire),
            LoopKind::Inner => holes.push(wire),
        }
    }
    let outer = outer.ok_or_else(|| topology_error("planar face has no outer boundary"))?;
    Profile::from_face_wires(outer, holes, tolerance)
}
