use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::{
    Axis3, CoedgeId, Compound, Curve2, Curve3, DiagnosticKind, EdgeId, FaceHandle, FaceId,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, Line3, ParameterRange,
    PersistentId, Plane3, Profile, RigidTransform3, Surface3,
    SurfaceIntersectionCurve, SurfaceProjectionPcurve2, Topology,
    ShellJoin, SplitKeep, TrimmedCurve2, VertexId, Wire2, box_center, invalid, nearest_uv, unsupported,
    whole_periods,
};
use crate::topology_error;

mod blend_offset;
mod dissolve;
mod draft;
mod move_faces;
mod offset;
mod offset_edge;
mod replace;
mod shell;
mod support;

use move_faces::*;
use offset::*;
use offset_edge::*;
use shell::*;
use support::*;

pub(crate) use dissolve::{dissolve_faces, split};
pub(crate) use draft::draft;
pub(crate) use move_faces::move_faces;
pub(crate) use offset::SupportEdit;
pub(crate) use offset_edge::{exact_pcurve, expanded_offset_bounds, extended_support, face_parameter_bounds};
pub(crate) use replace::replace_face;
pub(crate) use shell::{offset_faces, shell};
pub(crate) use support::{face_patch, support_patches};

fn not_implemented(operation: &str) -> KernelError {
    unsupported(format!(
        "exact analytical {operation} is not implemented for the supplied geometry"
    ))
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
            let section_center = plane.point_at(circle.center());
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
        .map(|point| plane.point_at(point))
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
    let result = annulus.extrude(
            section_plane,
            axis_direction * (axial_values[1] - axial_values[0]),
        )?;
    let roles = result.faces().map(|face| {
        let (values, value) = match result.origin(face) {
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionStart) }) => (&axial, axial_values[0]),
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionEnd) }) => (&axial, axial_values[1]),
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionSide { boundary, .. }) }) =>
                (&radial, if matches!(boundary, super::ProfileBoundary::Outer) { outer_radius } else { inner_radius }),
            _ => unreachable!(),
        };
        let segment = (0..4).find(|i| (values[*i] - value).abs() <= tolerance.linear &&
            (values[(*i + 1) % 4] - value).abs() <= tolerance.linear).unwrap();
        (face.persistent_id(), super::CreationRole::ProfileSide { generators: vec![[0, 0, profile.generator(0, segment)]], chart: None })
    }).collect::<Vec<_>>();
    Ok(Some(result.with_creation_roles(roles)))
}
