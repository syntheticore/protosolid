use crate::Point3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub signed_volume: f64,
    pub surface_area: f64,
    pub centroid: Point3,
    /// Unit-density inertia tensor about the centroid, in model axes.
    pub inertia: [[f64; 3]; 3],
}
