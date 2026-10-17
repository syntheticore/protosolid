use std::ops::Range;
use std::cell::OnceCell;
use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3};
use super::trim::{face_boundary_curves, face_parameter_bounds, point_in_trim, coincident_supports, FaceCurve};
use super::bounds::Bounds;
use super::{Compound, Curve3, FaceId, FaceOrientation, IntersectionKind, IntersectionSet,
    KernelResult, Line3, ModelTolerance, ParameterRange, ShellId, Surface3, Topology};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointClassification { Inside, Outside, Boundary }

struct ClassificationFace {
    id: FaceId,
    bounds: Bounds,
    parameters: [ParameterRange; 2],
    boundary: Vec<FaceCurve<super::CoedgeId>>,
}

struct ClassificationRegion {
    faces: Range<usize>,
    orientation: OnceCell<KernelResult<f64>>,
    shell: ShellId,
}

/// Shared point and support-side classification on trimmed BREP geometry.
pub(super) struct BrepClassifier<'a> {
    topology: &'a Topology,
    faces: Vec<ClassificationFace>,
    tolerance: ModelTolerance,
    regions: Vec<ClassificationRegion>,
    material_set: bool,
}

impl<'a> BrepClassifier<'a> {
    pub(super) fn new(topology: &'a Topology, shell: ShellId, tolerance: ModelTolerance)
        -> KernelResult<Self>
    {
        Self::from_shells(topology, [vec![shell]], tolerance, false)
    }

    pub(super) fn for_compound(compound: &'a Compound) -> KernelResult<Self> {
        let topology = compound.topology();
        let shells = compound.solids().map(|solid|
            topology.body(solid.topology_id()).map(|body| body.shells().to_vec())
        ).collect::<KernelResult<Vec<_>>>()?;
        Self::from_shells(topology, shells, compound.tolerance(), true)
    }

    fn from_shells(
        topology: &'a Topology,
        bodies: impl IntoIterator<Item = Vec<ShellId>>,
        tolerance: ModelTolerance,
        material_set: bool,
    ) -> KernelResult<Self> {
        let mut faces = Vec::new();
        let mut regions = Vec::new();
        for body in bodies {
            let shell_id = body[0];
            let start = faces.len();
            for shell in body {
                for id in topology.shell(shell)?.faces() {
                    let boundary = face_boundary_curves(topology, *id, tolerance)?;
                    faces.push(ClassificationFace {
                        id: *id,
                        bounds: super::bounds::face(topology, *id)?.expanded(tolerance.linear),
                        parameters: face_parameter_bounds(&boundary, tolerance)?,
                        boundary,
                    });
                }
            }
            regions.push(ClassificationRegion {
                faces: start..faces.len(),
                shell: shell_id,
                orientation: OnceCell::new(),
            });
        }
        Ok(Self { topology, faces, tolerance, regions, material_set })
    }

    /// Oriented material sides on coincident trimmed supports, as used when
    /// Boolean operations arrange shared faces without crossing a thin wall.
    pub(super) fn sides_on_support(
        &self, support: &Surface3, point: Point3, normal: Vec3,
    ) -> KernelResult<(bool, bool)> {
        let mut covered = false;
        let mut sides = (false, false);
        for face in &self.faces {
            let value = self.topology.face(face.id)?;
            let surface = value.surface();
            if !coincident_supports(surface, support, self.tolerance)? {
                continue;
            }
            let projection = surface.project(point, self.tolerance)?;
            // Equivalent supports can have different bounded spline domains.
            // A closest point clamped to one domain's edge does not cover a
            // point on the other's continuation.
            if projection.distance > self.tolerance.linear_at_scale(point.to_vec().magnitude()) {
                continue;
            }
            if !self.in_trim(face, projection.parameters)? { continue; }
            let sign = if value.orientation() == FaceOrientation::Forward { 1.0 } else { -1.0 };
            let forward = surface.normal(projection.parameters.0, projection.parameters.1)?.dot(normal) * sign > 0.0;
            sides.0 |= forward;
            sides.1 |= !forward;
            covered = true;
        }
        if covered { Ok(sides) } else {
            let inside = self.contains(point)?;
            Ok((inside, inside))
        }
    }

    pub(super) fn covers_on_support(&self, support: &Surface3, point: Point3) -> KernelResult<bool> {
        for face in &self.faces {
            let surface = self.topology.face(face.id)?.surface();
            if coincident_supports(support, surface, self.tolerance)? {
                let projection = surface.project(point, self.tolerance)?;
                if projection.distance > self.tolerance.linear_at_scale(point.to_vec().magnitude()) {
                    continue;
                }
                if self.in_trim(face, projection.parameters)? { return Ok(true); }
            }
        }
        Ok(false)
    }

    fn in_trim(&self, face: &ClassificationFace, parameters: (f64, f64)) -> KernelResult<bool> {
        let surface = self.topology.face(face.id)?.surface();
        let mut uv = Point2::new(parameters.0, parameters.1);
        for axis in 0..2 {
            if let Some(period) = surface.domains()[axis].period {
                let center = (face.parameters[axis].start + face.parameters[axis].end) * 0.5;
                uv[axis] += ((center - uv[axis]) / period).round() * period;
            }
        }
        point_in_trim(uv, &face.boundary)
    }

    pub(super) fn contains(&self, point: Point3) -> KernelResult<bool> {
        self.classify(point).map(|location| location != PointClassification::Outside)
    }

    pub(super) fn sides_at_point(&self, point: Point3, normal: Vec3) -> KernelResult<(bool, bool)> {
        let step = self.tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 16.0;
        let offset = normal.normalize() * step;
        Ok((self.classify(point - offset)? == PointClassification::Inside,
            self.classify(point + offset)? == PointClassification::Inside))
    }

    pub(super) fn classify(&self, point: Point3) -> KernelResult<PointClassification> {
        // A body's closed shells describe one material region, including its
        // cavities. Separate solids form a union, including overlapping solids.
        let mut boundary = false;
        for region in &self.regions {
            let faces = &self.faces[region.faces.clone()];
            let inside = self.region_contains(region, faces, point)?;
            if inside == PointClassification::Inside { return Ok(inside); }
            boundary |= inside == PointClassification::Boundary;
        }
        Ok(if boundary { PointClassification::Boundary } else { PointClassification::Outside })
    }


    fn region_contains(&self, region: &ClassificationRegion, faces: &[ClassificationFace], point: Point3) -> KernelResult<PointClassification> {
        let bounds = faces.iter().map(|face| face.bounds).reduce(Bounds::union).unwrap();
        if (0..3).any(|axis| point[axis] < bounds.0[axis] || point[axis] > bounds.1[axis]) {
            return Ok(PointClassification::Outside);
        }
        let directions = [
            Vec3::new(1.0, std::f64::consts::SQRT_2, std::f64::consts::PI),
            Vec3::new(std::f64::consts::E, 1.0, std::f64::consts::SQRT_2),
            Vec3::new(std::f64::consts::PI, std::f64::consts::E, 1.0),
        ];
        for direction in directions {
            if let Some(inside) = self.ray_contains(faces, point, direction.normalize())? {
                return Ok(inside);
            }
        }
        let orientation = if self.material_set { 1.0 } else {
            *region.orientation.get_or_init(||
                super::physical::shell_signed_volume(self.topology, region.shell, self.tolerance)
                    .map(f64::signum)
            ).as_ref().map_err(Clone::clone)?
        };
        self.nearest_boundary_contains(faces, point, orientation)
    }

    fn ray_contains(&self, faces: &[ClassificationFace], point: Point3, direction: Vec3) -> KernelResult<Option<PointClassification>> {
        let ray: Curve3 = Line3::new(point, direction)?.into();
        let mut crossings = Vec::new();
        for face in faces {
            if !ray_meets_bounds(face.bounds, point, direction) {
                continue;
            }
            let surface = self.topology.face(face.id)?.surface();
            if !matches!(surface, Surface3::Plane(_) | Surface3::Cylinder(_)
                | Surface3::Cone(_) | Surface3::Sphere(_)) {
                return Ok(None);
            }
            let hits = match ray.intersect_surface(surface, self.tolerance)? {
                IntersectionSet::Empty => continue,
                IntersectionSet::Coincident => return Ok(None),
                IntersectionSet::Isolated(hits) => hits,
            };
            for hit in hits {
                if hit.curve_parameter < -self.tolerance.linear
                    || !self.in_trim(face, hit.surface_parameters)? {
                    continue;
                }
                if hit.curve_parameter.abs() <= self.tolerance.linear {
                    return Ok(Some(PointClassification::Boundary));
                }
                if hit.kind == IntersectionKind::Tangent {
                    return Ok(None);
                }
                crossings.push(hit.curve_parameter);
            }
        }
        crossings.sort_by(f64::total_cmp);
        // A shared edge/seam can produce two hits at one parameter. Choose a
        // different ray instead of assigning multiplicity at that junction.
        if crossings.windows(2).any(|pair| (pair[0] - pair[1]).abs() <= self.tolerance.linear) {
            return Ok(None);
        }
        Ok(Some(if crossings.len() % 2 == 1 { PointClassification::Inside } else { PointClassification::Outside }))
    }

    /// The closest point on a closed boundary determines its material side.
    /// Interior candidates use the exact support; constrained minima include
    /// every trim edge and endpoint. At shared edges, sum the incident normals
    /// instead of depending on which face happens to be visited first.
    fn nearest_boundary_contains(
        &self, faces: &[ClassificationFace], point: Point3, orientation: f64,
    ) -> KernelResult<PointClassification> {
        let linear = self.tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0));
        let mut minimum = BoundaryMinimum { point, distance: f64::INFINITY, normal: Vec3::zero() };
        let mut ordered = faces.iter().map(|face| (bounds_distance(face.bounds, point), face))
            .collect::<Vec<_>>();
        ordered.sort_by(|first, second| first.0.total_cmp(&second.0));
        for (lower, face) in ordered {
            if lower > minimum.distance + linear { break; }
            let value = self.topology.face(face.id)?;
            let surface = value.surface();
            let sign = orientation * if value.orientation() == FaceOrientation::Forward { 1.0 } else { -1.0 };
            if let Ok(projection) = surface.project(point, self.tolerance) {
                if self.in_trim(face, projection.parameters)? {
                    if projection.distance <= linear { return Ok(PointClassification::Boundary); }
                    minimum.consider(projection.point,
                        surface.normal(projection.parameters.0, projection.parameters.1)? * sign, point, linear);
                    // These supports have a global analytical projection. Once
                    // it lies in the trim, an edge cannot be a closer minimum.
                    if matches!(surface, Surface3::Plane(_) | Surface3::Cylinder(_)
                        | Surface3::Cone(_) | Surface3::Sphere(_) | Surface3::Torus(_)) { continue; }
                }
            }
            for boundary in &face.boundary {
                let projection = boundary.curve.project_on_trim(point, boundary.range, self.tolerance)?;
                if projection.distance <= linear { return Ok(PointClassification::Boundary); }
                let uv = boundary.pcurve.evaluate(projection.parameter)?.point;
                // An edge minimum belongs to every incident face. Obtain its
                // normal from topology, rather than hoping independent face
                // projections converge to numerically identical minima.
                let edge = self.topology.edge(self.topology.coedge(boundary.provenance)?.edge())?;
                let normal = edge.coedges().iter().try_fold(Vec3::zero(), |sum, id| {
                    let coedge = self.topology.coedge(*id)?;
                    let face = self.topology.face(self.topology.loop_(coedge.loop_id())?.face())?;
                    let uv = coedge.pcurve().evaluate(projection.parameter)?.point;
                    let sign = if face.orientation() == FaceOrientation::Forward { orientation } else { -orientation };
                    Ok::<_, super::KernelError>(sum + face.surface().normal(uv.x, uv.y)? * sign)
                })?;
                minimum.consider(projection.point, normal, point, linear);
                let uv = refine_face_projection(surface, point, uv, face.parameters, self.tolerance)?;
                if self.in_trim(face, (uv.x, uv.y))? {
                    let (position, _, _) = surface.first_order(uv.x, uv.y)?;
                    if position.distance(point) <= linear { return Ok(PointClassification::Boundary); }
                    minimum.consider(position, surface.normal(uv.x, uv.y)? * sign, point, linear);
                }
            }
        }
        if !minimum.distance.is_finite() || minimum.normal.magnitude2() == 0.0 {
            return Err(super::KernelError::new(super::DiagnosticKind::NumericalFailure,
                "BREP closest boundary has no oriented normal"));
        }
        Ok(if (point - minimum.point).dot(minimum.normal) < 0.0 {
            PointClassification::Inside
        } else { PointClassification::Outside })
    }
}

struct BoundaryMinimum {
    point: Point3,
    distance: f64,
    normal: Vec3,
}

impl BoundaryMinimum {
    fn consider(&mut self, position: Point3, normal: Vec3, query: Point3, linear: f64) {
        let distance = position.distance(query);
        if distance < self.distance - linear {
            self.distance = distance;
            self.point = position;
            self.normal = normal;
        } else if (distance - self.distance).abs() <= linear && position.distance(self.point) <= linear {
            self.normal += normal;
        }
    }
}

fn bounds_distance(bounds: Bounds, point: Point3) -> f64 {
    (0..3).map(|axis| (bounds.0[axis] - point[axis]).max(point[axis] - bounds.1[axis]).max(0.0).powi(2))
        .sum::<f64>().sqrt()
}

/// Constrained descent from a trim minimum explores the adjacent face interior.
/// A support's global projection may select a different periodic branch or a
/// point outside the retained trim, particularly on extended blend supports.
fn refine_face_projection(
    surface: &Surface3, point: Point3, mut uv: Point2,
    bounds: [ParameterRange; 2], tolerance: ModelTolerance,
) -> KernelResult<Point2> {
    let constrain = |mut candidate: Point2| {
        for axis in 0..2 {
            candidate[axis] = candidate[axis].clamp(bounds[axis].start.min(bounds[axis].end),
                bounds[axis].start.max(bounds[axis].end));
        }
        candidate
    };
    uv = constrain(uv);
    for _ in 0..64 {
        let (position, du, dv) = surface.first_order(uv.x, uv.y)?;
        let residual = point - position;
        let a = du.magnitude2();
        let b = du.dot(dv);
        let c = dv.magnitude2();
        let determinant = a * c - b * b;
        if determinant <= a * c * f64::EPSILON * 64.0 { break; }
        let step = crate::Vec2::new((residual.dot(du) * c - residual.dot(dv) * b) / determinant,
            (residual.dot(dv) * a - residual.dot(du) * b) / determinant);
        let mut damping = 1.0;
        let mut next = uv;
        for _ in 0..24 {
            let candidate = constrain(uv + step * damping);
            let (position, _, _) = surface.first_order(candidate.x, candidate.y)?;
            if position.distance2(point) < residual.magnitude2() {
                next = candidate;
                break;
            }
            damping *= 0.5;
        }
        let movement = du * (next.x - uv.x) + dv * (next.y - uv.y);
        uv = next;
        if movement.magnitude() <= tolerance.linear_at_scale(position.to_vec().magnitude().max(1.0)) {
            break;
        }
    }
    Ok(uv)
}

fn ray_meets_bounds(bounds: Bounds, point: Point3, direction: Vec3) -> bool {
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        let first = (bounds.0[axis] - point[axis]) / direction[axis];
        let second = (bounds.1[axis] - point[axis]) / direction[axis];
        near = near.max(first.min(second));
        far = far.min(first.max(second));
    }
    far >= near
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Kernel;

    #[test]
    fn material_and_shell_queries_classify_a_torus_analytically() {
        let tolerance = ModelTolerance::default();
        let mut kernel = Kernel::new(tolerance).unwrap();
        let mut transaction = kernel.begin_operation();
        transaction.torus(Point3::origin(), Vec3::unit_x() * 4.0,
            Vec3::unit_y() * 4.0, 1.25).unwrap();
        let operation = transaction.finish().unwrap();
        let compound = Compound::from_operation(kernel, operation, []).unwrap();
        let classifier = BrepClassifier::for_compound(&compound).unwrap();
        assert!(classifier.contains(Point3::new(4.0, 0.0, 0.0)).unwrap());
        assert!(!classifier.contains(Point3::origin()).unwrap());
        assert_eq!(classifier.classify(Point3::new(5.25, 0.0, 0.0)).unwrap(), PointClassification::Boundary);
        assert_eq!(classifier.sides_at_point(Point3::new(5.25, 0.0, 0.0), Vec3::unit_x()).unwrap(), (true, false));
        let topology = compound.topology();
        let shell = topology.bodies().next().unwrap().1.shells()[0];
        let shell = BrepClassifier::new(topology, shell, tolerance).unwrap();
        assert!(shell.contains(Point3::new(4.0, 0.0, 0.0)).unwrap());
    }
}
