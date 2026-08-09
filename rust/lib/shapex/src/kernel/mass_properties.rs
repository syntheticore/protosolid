use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::{
    BodyId, DiagnosticKind, Kernel, KernelError, KernelResult, TessellationOptions, TriangleMesh,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub signed_volume: f64,
    pub surface_area: f64,
    pub centroid: Point3,
    /// Unit-density inertia tensor about the centroid, in model axes.
    pub inertia: [[f64; 3]; 3],
}

impl Kernel {
    /// Numerically integrate mass properties from a tolerance-controlled BREP mesh.
    /// Density is one in model units. This is an approximation for curved faces;
    /// physical publication uses a separate certification path with error bounds.
    pub fn approximate_mass_properties(
        &self,
        body: BodyId,
        options: TessellationOptions,
    ) -> KernelResult<MassProperties> {
        let mesh = self.tessellate_body(body, options)?;
        mass_properties_from_mesh(&mesh)
    }
}

pub(crate) fn mass_properties_from_mesh(mesh: &TriangleMesh) -> KernelResult<MassProperties> {
    if mesh.triangles.is_empty() {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "mass properties require a nonempty closed triangle mesh",
        ));
    }
    let mut signed_volume = 0.0;
    let mut area = 0.0;
    let mut centroid_moment = Vec3::new(0.0, 0.0, 0.0);
    let mut second_moment = [[0.0_f64; 3]; 3];
    for triangle in &mesh.triangles {
        let a = mesh.positions[triangle[0]];
        let b = mesh.positions[triangle[1]];
        let c = mesh.positions[triangle[2]];
        let cross = (b - a).cross(c - a);
        area += cross.magnitude() * 0.5;
        let tetrahedron_volume = a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0;
        signed_volume += tetrahedron_volume;
        centroid_moment += (a.to_vec() + b.to_vec() + c.to_vec()) * (tetrahedron_volume / 4.0);
        let coordinates = [[a.x, a.y, a.z], [b.x, b.y, b.z], [c.x, c.y, c.z]];
        for (axis, moments) in second_moment.iter_mut().enumerate() {
            let values = [
                coordinates[0][axis],
                coordinates[1][axis],
                coordinates[2][axis],
            ];
            moments[axis] += tetrahedron_volume / 10.0
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
                let value = tetrahedron_volume / 20.0 * (2.0 * diagonal_products + cross_products);
                second_moment[first][second] += value;
                second_moment[second][first] += value;
            }
        }
    }
    if !signed_volume.is_finite() || !area.is_finite() || signed_volume == 0.0 {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "closed mesh has zero or non-finite signed volume",
        ));
    }
    let centroid = Point3::from_vec(centroid_moment / signed_volume);
    if !centroid.x.is_finite() || !centroid.y.is_finite() || !centroid.z.is_finite() {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "mass-property integration produced a non-finite centroid",
        ));
    }
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
            let shift = signed_volume
                * (if row == column { center_squared } else { 0.0 } - center[row] * center[column]);
            inertia[row][column] -= shift;
        }
    }
    Ok(MassProperties {
        volume: signed_volume.abs(),
        signed_volume,
        surface_area: area,
        centroid,
        inertia,
    })
}
