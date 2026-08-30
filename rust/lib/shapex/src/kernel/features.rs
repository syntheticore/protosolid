use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::{
    Axis3, Compound, Curve2, DiagnosticKind, FaceHandle, Kernel, KernelError, KernelResult, Plane3,
    Profile, RigidTransform3, TrimmedCurve2, Wire2,
};

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

fn unsupported(operation: &str) -> KernelError {
    KernelError::new(
        DiagnosticKind::UnsupportedGeometry,
        format!(
            "exact analytical {operation} is not implemented; polyhedral BREP reconstruction has been removed"
        ),
    )
}

fn on_plane(plane: Plane3, point: Point2) -> Point3 {
    plane.origin() + plane.u() * point.x + plane.v() * point.y
}

fn publish_torus(
    center: Point3,
    x_major: crate::Vec3,
    y_major: crate::Vec3,
    minor_radius: f64,
    tolerance: super::ModelTolerance,
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

/// Recognize full revolutions that have a closed-form analytical BREP.
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
            if x_major.magnitude() > circle.radius() + tolerance.linear {
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
    let Some(radial_direction) = radial_vectors
        .iter()
        .find(|vector| vector.magnitude() > tolerance.linear)
        .map(|vector| vector.normalize())
    else {
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

pub(crate) fn shell(
    _source: &Compound,
    _openings: &[FaceHandle],
    distance: f64,
) -> KernelResult<Compound> {
    if !distance.is_finite() {
        return Err(invalid("shell thickness must be finite"));
    }
    Err(unsupported("shelling"))
}

pub(crate) fn split(_source: &Compound, _plane: Plane3, keep: &str) -> KernelResult<Compound> {
    if !matches!(keep, "left" | "right" | "both") {
        return Err(invalid("split keep mode must be left, right, or both"));
    }
    Err(unsupported("splitting"))
}

pub(crate) fn move_faces(
    _source: &Compound,
    faces: &[FaceHandle],
    _transform: RigidTransform3,
) -> KernelResult<Compound> {
    if faces.is_empty() {
        return Err(invalid("moving faces needs at least one face"));
    }
    Err(unsupported("face movement"))
}

pub(crate) fn replace_face(
    _source: &Compound,
    _target: &FaceHandle,
    _replacement: &FaceHandle,
) -> KernelResult<Compound> {
    Err(unsupported("face replacement"))
}

impl Profile {
    pub fn revolve(&self, plane: Plane3, axis: Axis3, angle: f64) -> KernelResult<Compound> {
        if !angle.is_finite() || angle.abs() <= self.tolerance().parameter {
            return Err(invalid("revolution angle must be finite and nonzero"));
        }
        if angle.abs() > std::f64::consts::TAU + self.tolerance().parameter {
            return Err(invalid("revolution angle may not exceed one full turn"));
        }
        let full = (angle.abs() - std::f64::consts::TAU).abs() <= self.tolerance().parameter;
        if full {
            if let Some(exact) = exact_full_revolution(self, plane, axis)? {
                return Ok(exact);
            }
        }
        Err(unsupported("revolution"))
    }

    pub fn loft<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
    ) -> KernelResult<Compound> {
        self.loft_with_alignment(plane, sections, true)
    }

    pub fn loft_with_alignment<'a>(
        &'a self,
        _plane: Plane3,
        _sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        _align_sections: bool,
    ) -> KernelResult<Compound> {
        Err(unsupported("lofting"))
    }
}
