use std::collections::BTreeSet;

use cgmath::prelude::*;

use crate::{Matrix3, Point3, Quaternion, Vec3};

use super::validation::ValidationMode;
use super::{
    ChangeKind, Cone3, Curve3, Cylinder3, DiagnosticKind, FaceId, Frame3, Kernel, KernelError,
    KernelResult, Line3, ModelTolerance, NurbsCurve3, NurbsSurface3, OffsetSurface3,
    OperationHistory, Plane3, RollingBallSurface3, Sphere3, Surface3, SurfaceCurve3,
    SurfaceIntersectionCurve, Torus3,
};

/// A validated orientation-preserving Euclidean transform.
///
/// Restricting this type to rigid transforms means analytic circles and cylinders
/// remain exact analytic geometry. General affine transforms can be introduced
/// separately with explicit promotion of conics rather than silently deforming
/// their invariants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidTransform3 {
    rotation: Matrix3,
    translation: Vec3,
}

impl RigidTransform3 {
    pub fn new(
        rotation: Matrix3,
        translation: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let columns = [rotation.x, rotation.y, rotation.z];
        let finite = columns
            .iter()
            .chain(std::iter::once(&translation))
            .all(|vector| vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite());
        let orthonormal = columns
            .iter()
            .all(|column| (column.magnitude() - 1.0).abs() <= tolerance.angular)
            && columns[0].dot(columns[1]).abs() <= tolerance.angular
            && columns[0].dot(columns[2]).abs() <= tolerance.angular
            && columns[1].dot(columns[2]).abs() <= tolerance.angular;
        if !finite || !orthonormal || (rotation.determinant() - 1.0).abs() > tolerance.angular {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "a rigid transform requires a finite, right-handed orthonormal rotation",
            ));
        }
        Ok(Self {
            rotation,
            translation,
        })
    }

    pub fn from_quaternion(
        rotation: Quaternion,
        translation: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !rotation.s.is_finite()
            || !rotation.v.x.is_finite()
            || !rotation.v.y.is_finite()
            || !rotation.v.z.is_finite()
            || rotation.magnitude2() == 0.0
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "transform quaternion must be finite and nonzero",
            ));
        }
        Self::new(Matrix3::from(rotation.normalize()), translation, tolerance)
    }

    pub fn identity() -> Self {
        Self {
            rotation: Matrix3::identity(),
            translation: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    pub fn rotation(self) -> Matrix3 {
        self.rotation
    }
    pub fn translation(self) -> Vec3 {
        self.translation
    }
    pub fn transform_point(self, point: Point3) -> Point3 {
        Point3::from_vec(self.rotation * point.to_vec() + self.translation)
    }
    pub fn transform_vector(self, vector: Vec3) -> Vec3 {
        self.rotation * vector
    }

    pub fn inverse(self) -> Self {
        let rotation = self.rotation.transpose();
        Self {
            rotation,
            translation: -(rotation * self.translation),
        }
    }

    pub fn then(self, next: Self) -> Self {
        Self {
            rotation: next.rotation * self.rotation,
            translation: next.rotation * self.translation + next.translation,
        }
    }
}

impl Kernel {
    /// Rigidly transform a body without changing its topology or persistent names.
    /// The transformed copy is physically re-certified before it becomes visible.
    pub fn transform_body(
        &mut self,
        body: super::BodyId,
        transform: RigidTransform3,
    ) -> KernelResult<OperationHistory> {
        let mut history = OperationHistory::default();
        let kind = if transform == RigidTransform3::identity() {
            ChangeKind::Unchanged
        } else {
            ChangeKind::Modified
        };
        for name in self.topology.body_entity_names(body)? {
            history.changed(name, kind, [name]);
        }
        if kind == ChangeKind::Unchanged {
            return Ok(history);
        }
        let mut working = self.topology.clone();
        let shell_ids = working.body(body)?.shells().to_vec();
        let mut faces = BTreeSet::<FaceId>::new();
        let mut edges = BTreeSet::new();
        let mut vertices = BTreeSet::new();
        for shell_id in shell_ids {
            for face_id in working.shell(shell_id)?.faces() {
                faces.insert(*face_id);
                for loop_id in working.face(*face_id)?.loops() {
                    let Some(first) = working.loop_(*loop_id)?.first() else {
                        continue;
                    };
                    let mut current = first;
                    loop {
                        let coedge = working.coedge(current)?;
                        edges.insert(coedge.edge());
                        vertices.extend(working.edge(coedge.edge())?.vertices());
                        current = coedge.next();
                        if current == first {
                            break;
                        }
                    }
                }
            }
        }
        for vertex_id in vertices {
            let vertex = working.vertices.get_mut(vertex_id)?;
            vertex.point = transform.transform_point(vertex.point);
        }
        for edge_id in edges {
            let edge = working.edges.get_mut(edge_id)?;
            edge.curve = transform_curve(edge.curve.clone(), transform, self.tolerance)?;
        }
        for face_id in faces {
            let face = working.faces.get_mut(face_id)?;
            face.surface = transform_surface(face.surface.clone(), transform, self.tolerance)?;
        }
        super::validation::validate(&working, self.tolerance, ValidationMode::Published)
            .into_result()?;
        self.topology = working;
        Ok(history)
    }
}

fn transform_curve(
    curve: Curve3,
    transform: RigidTransform3,
    tolerance: ModelTolerance,
) -> KernelResult<Curve3> {
    Ok(match curve {
        Curve3::Line(line) => Line3::new(
            transform.transform_point(line.origin()),
            transform.transform_vector(line.direction()),
        )?
        .into(),
        Curve3::Circle(circle) => super::Circle3::new(
            transform.transform_point(circle.center()),
            transform.transform_vector(circle.x_radius()),
            transform.transform_vector(circle.y_radius()),
            tolerance,
        )?
        .into(),
        Curve3::Nurbs(curve) => NurbsCurve3::new(
            curve.degree(),
            curve
                .control_points()
                .iter()
                .copied()
                .map(|point| transform.transform_point(point))
                .collect(),
            curve.weights().to_vec(),
            curve.knots().to_vec(),
        )?
        .into(),
        Curve3::SurfaceIntersection(curve) => SurfaceIntersectionCurve::new(
            transform_surface(curve.first_surface().clone(), transform, tolerance)?,
            transform_surface(curve.second_surface().clone(), transform, tolerance)?,
            curve.first_pcurve().clone(),
            curve.second_pcurve().clone(),
            curve.range(),
            tolerance,
            curve.achieved_error(),
        )?
        .into(),
        Curve3::SurfaceCurve(curve) => SurfaceCurve3::new(
            transform_surface(curve.surface().clone(), transform, tolerance)?,
            curve.pcurve().clone(),
            curve.range(),
            tolerance,
        )?
        .into(),
    })
}

pub(crate) fn transform_surface(
    surface: Surface3,
    transform: RigidTransform3,
    tolerance: ModelTolerance,
) -> KernelResult<Surface3> {
    Ok(match surface {
        Surface3::Plane(plane) => Plane3::new(
            transform.transform_point(plane.origin()),
            transform.transform_vector(plane.u()),
            transform.transform_vector(plane.v()),
            tolerance,
        )?
        .into(),
        Surface3::Cylinder(cylinder) => Cylinder3::new(
            transform.transform_point(cylinder.origin()),
            transform.transform_vector(cylinder.axis()),
            transform.transform_vector(cylinder.x_radius()),
            transform.transform_vector(cylinder.y_radius()),
            tolerance,
        )?
        .into(),
        Surface3::Cone(cone) => Cone3::new(
            transform.transform_point(cone.apex()),
            transform.transform_vector(cone.axis()),
            transform.transform_vector(cone.x_radius()),
            transform.transform_vector(cone.y_radius()),
            tolerance,
        )?
        .into(),
        Surface3::Sphere(sphere) => Sphere3::new(
            transform.transform_point(sphere.center()),
            transform.transform_vector(sphere.x_radius()),
            transform.transform_vector(sphere.y_radius()),
            transform.transform_vector(sphere.z_radius()),
            tolerance,
        )?
        .into(),
        Surface3::Torus(torus) => Torus3::new(
            transform.transform_point(torus.center()),
            transform.transform_vector(torus.x_major()),
            transform.transform_vector(torus.y_major()),
            torus.minor_radius(),
            tolerance,
        )?
        .into(),
        Surface3::Swept(surface) => {
            let plane = |p: super::Plane3| {
                super::Plane3::new(
                    transform.transform_point(p.origin()),
                    transform.transform_vector(p.u()),
                    transform.transform_vector(p.v()),
                    tolerance,
                )
            };
            Surface3::Swept(super::SweptSurface3::new(
                plane(surface.profile_plane())?,
                surface.profile().clone(),
                plane(surface.rail_plane())?,
                surface.rail().clone(),
                tolerance,
            )?)
        }
        Surface3::Revolved(surface) => {
            let frame = surface.frame();
            super::RevolvedSurface3::new(
                Frame3::new(
                    transform.transform_point(frame.origin()),
                    transform.transform_vector(frame.x()),
                    transform.transform_vector(frame.y()),
                    tolerance,
                )?,
                surface.profile().clone(),
                surface.profile_range(),
                tolerance,
            )?
            .into()
        }
        Surface3::Nurbs(surface) => NurbsSurface3::new(
            surface.degrees(),
            surface
                .control_points()
                .iter()
                .map(|row| {
                    row.iter()
                        .copied()
                        .map(|point| transform.transform_point(point))
                        .collect()
                })
                .collect(),
            surface.weights().to_vec(),
            (surface.knots().0.to_vec(), surface.knots().1.to_vec()),
        )?
        .into(),
        Surface3::Offset(surface) => OffsetSurface3::new(
            transform_surface(surface.base().clone(), transform, tolerance)?,
            surface.distance(),
            tolerance,
        )?
        .into(),
        Surface3::RollingBall(surface) | Surface3::Ruled(surface) => {
            RollingBallSurface3::with_section(
                transform_curve(surface.center_curve().clone(), transform, tolerance)?,
                transform_surface(surface.first_surface().clone(), transform, tolerance)?,
                transform_surface(surface.second_surface().clone(), transform, tolerance)?,
                surface.first_pcurve().clone(),
                surface.second_pcurve().clone(),
                surface.center_range(),
                surface.radius(),
                tolerance,
                surface.is_ruled(),
            )?
            .into()
        }
    })
}
