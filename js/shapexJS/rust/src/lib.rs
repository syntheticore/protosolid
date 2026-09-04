//! Private WebAssembly transport for ShapexJS.
//!
//! This crate belongs to the JavaScript package. It is intentionally not part
//! of Shapex and exposes no public application API: `../index.js` provides the
//! Rust-shaped JavaScript surface.

use js_sys::{Float64Array, Uint32Array, JSON};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use shapex::kernel::{
    Axis3, BooleanKind, Circle2, Curve2, Curve3, Line2, NurbsCurve2, Origin, ParameterRange,
    RigidTransform3, Surface3,
};
use shapex::{
    Compound, CreationRole, EdgeHandle, FaceHandle, FilletOptions, Matrix3, ModelTolerance, Plane3,
    Point2, Point3, Profile, ProfileBoundary, Resolution, Solid, TessellationOptions, TriangleMesh,
    TrimmedCurve2, Vec2, Vec3, VertexHandle, Wire2,
};
use wasm_bindgen::prelude::*;

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn id_string(id: shapex::kernel::PersistentId) -> String {
    format!("{}:{:016x}", id.namespace, id.local)
}

fn f64_array(values: &[f64]) -> Float64Array {
    Float64Array::from(values)
}

fn point_values(point: Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn vector_values(vector: Vec3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    let json = JSON::stringify(&value)
        .map_err(|_| JsValue::from_str("value cannot be serialized"))?
        .as_string()
        .ok_or_else(|| JsValue::from_str("value did not serialize to text"))?;
    serde_json::from_str(&json).map_err(js_error)
}

#[derive(Clone, Copy, Deserialize)]
struct ToleranceDto {
    linear: f64,
    relative: f64,
    angular: f64,
    parameter: f64,
}

impl From<ToleranceDto> for ModelTolerance {
    fn from(value: ToleranceDto) -> Self {
        Self::new(value.linear, value.relative, value.angular, value.parameter)
    }
}

#[derive(Deserialize)]
struct RangeDto {
    start: f64,
    end: f64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum CurveDto {
    Line {
        origin: [f64; 2],
        direction: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    Nurbs {
        degree: usize,
        control_points: Vec<[f64; 2]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}

impl CurveDto {
    fn build(self) -> Result<Curve2, JsValue> {
        match self {
            Self::Line { origin, direction } => Line2::new(
                Point2::new(origin[0], origin[1]),
                Vec2::new(direction[0], direction[1]),
            )
            .map(Into::into)
            .map_err(js_error),
            Self::Circle { center, radius } => {
                Circle2::new(Point2::new(center[0], center[1]), radius)
                    .map(Into::into)
                    .map_err(js_error)
            }
            Self::Nurbs {
                degree,
                control_points,
                weights,
                knots,
            } => NurbsCurve2::new(
                degree,
                control_points
                    .into_iter()
                    .map(|point| Point2::new(point[0], point[1]))
                    .collect(),
                weights,
                knots,
            )
            .map(Into::into)
            .map_err(js_error),
        }
    }
}

#[derive(Deserialize)]
struct TrimmedCurveDto {
    curve: CurveDto,
    range: RangeDto,
}

impl TrimmedCurveDto {
    fn build(self) -> Result<TrimmedCurve2, JsValue> {
        TrimmedCurve2::new(
            self.curve.build()?,
            ParameterRange::new(self.range.start, self.range.end).map_err(js_error)?,
        )
        .map_err(js_error)
    }
}

#[derive(Deserialize)]
struct WireDto {
    segments: Vec<TrimmedCurveDto>,
    tolerance: ToleranceDto,
}

impl WireDto {
    fn build(self) -> Result<Wire2, JsValue> {
        let tolerance = self.tolerance.into();
        let segments = self
            .segments
            .into_iter()
            .map(TrimmedCurveDto::build)
            .collect::<Result<Vec<_>, _>>()?;
        Wire2::new(segments, tolerance).map_err(js_error)
    }
}

#[derive(Deserialize)]
struct ProfileDto {
    outer: WireDto,
    holes: Vec<WireDto>,
    tolerance: ToleranceDto,
}

impl ProfileDto {
    fn build(self) -> Result<Profile, JsValue> {
        let tolerance = self.tolerance.into();
        let outer = self.outer.build()?;
        let holes = self
            .holes
            .into_iter()
            .map(WireDto::build)
            .collect::<Result<Vec<_>, _>>()?;
        Profile::new(outer, holes, tolerance).map_err(js_error)
    }
}

#[derive(Deserialize)]
struct SectionDto {
    profile: ProfileDto,
    plane: [f64; 9],
}

fn profile(value: JsValue) -> Result<Profile, JsValue> {
    from_js::<ProfileDto>(value)?.build()
}

fn tolerance(values: &[f64]) -> Result<ModelTolerance, JsValue> {
    let [linear, relative, angular, parameter] = values else {
        return Err(JsValue::from_str("model tolerance needs four values"));
    };
    let value = ModelTolerance::new(*linear, *relative, *angular, *parameter);
    value.validate().map_err(js_error)?;
    Ok(value)
}

fn plane(values: &[f64], model_tolerance: ModelTolerance) -> Result<Plane3, JsValue> {
    let [ox, oy, oz, ux, uy, uz, vx, vy, vz] = values else {
        return Err(JsValue::from_str("plane needs nine values"));
    };
    Plane3::new(
        Point3::new(*ox, *oy, *oz),
        Vec3::new(*ux, *uy, *uz),
        Vec3::new(*vx, *vy, *vz),
        model_tolerance,
    )
    .map_err(js_error)
}

fn axis(values: &[f64]) -> Result<Axis3, JsValue> {
    let [ox, oy, oz, dx, dy, dz] = values else {
        return Err(JsValue::from_str("axis needs six values"));
    };
    Axis3::new(Point3::new(*ox, *oy, *oz), Vec3::new(*dx, *dy, *dz)).map_err(js_error)
}

fn transform(values: &[f64], model_tolerance: ModelTolerance) -> Result<RigidTransform3, JsValue> {
    let [xx, xy, xz, yx, yy, yz, zx, zy, zz, tx, ty, tz] = values else {
        return Err(JsValue::from_str("rigid transform needs twelve values"));
    };
    RigidTransform3::new(
        Matrix3::from_cols(
            Vec3::new(*xx, *xy, *xz),
            Vec3::new(*yx, *yy, *yz),
            Vec3::new(*zx, *zy, *zz),
        ),
        Vec3::new(*tx, *ty, *tz),
        model_tolerance,
    )
    .map_err(js_error)
}

fn tessellation_options(
    linear: f64,
    angular: f64,
    maximum_depth: u8,
) -> Result<TessellationOptions, JsValue> {
    TessellationOptions::new(linear, angular, maximum_depth).map_err(js_error)
}

#[wasm_bindgen(js_name = profileArea)]
pub fn profile_area(description: JsValue) -> Result<f64, JsValue> {
    profile(description).map(|profile| profile.area())
}

#[wasm_bindgen(js_name = wireProperties)]
pub fn wire_properties(description: JsValue) -> Result<Float64Array, JsValue> {
    let wire = from_js::<WireDto>(description)?.build()?;
    Ok(f64_array(&[
        wire.signed_area(),
        wire.area(),
        if wire.is_counter_clockwise() {
            1.0
        } else {
            0.0
        },
    ]))
}

#[wasm_bindgen(js_name = extrude)]
pub fn extrude(
    description: JsValue,
    plane_values: &[f64],
    vector: &[f64],
) -> Result<NativeCompound, JsValue> {
    let profile = profile(description)?;
    let [x, y, z] = vector else {
        return Err(JsValue::from_str("extrusion vector needs three values"));
    };
    let plane = plane(plane_values, profile.tolerance())?;
    profile
        .extrude(plane, Vec3::new(*x, *y, *z))
        .map(|inner| NativeCompound { inner })
        .map_err(js_error)
}

#[wasm_bindgen(js_name = revolve)]
pub fn revolve(
    description: JsValue,
    plane_values: &[f64],
    axis_values: &[f64],
    angle: f64,
) -> Result<NativeCompound, JsValue> {
    let profile = profile(description)?;
    profile
        .revolve(
            plane(plane_values, profile.tolerance())?,
            axis(axis_values)?,
            angle,
        )
        .map(|inner| NativeCompound { inner })
        .map_err(js_error)
}

#[wasm_bindgen(js_name = loft)]
pub fn loft(sections: JsValue, align_sections: bool) -> Result<NativeCompound, JsValue> {
    let sections = from_js::<Vec<SectionDto>>(sections)?;
    if sections.len() < 2 {
        return Err(JsValue::from_str("a loft needs at least two profiles"));
    }
    let built = sections
        .into_iter()
        .map(|section| {
            let profile = section.profile.build()?;
            let plane = plane(&section.plane, profile.tolerance())?;
            Ok((plane, profile))
        })
        .collect::<Result<Vec<_>, JsValue>>()?;
    let (first_plane, first_profile) = &built[0];
    first_profile
        .loft_with_alignment(
            *first_plane,
            built
                .iter()
                .skip(1)
                .map(|(plane, profile)| (*plane, profile)),
            align_sections,
        )
        .map(|inner| NativeCompound { inner })
        .map_err(js_error)
}

#[wasm_bindgen(js_name = NativeMesh)]
pub struct NativeMesh {
    positions: Vec<f64>,
    normals: Vec<f64>,
    triangles: Vec<u32>,
}

impl From<TriangleMesh> for NativeMesh {
    fn from(mesh: TriangleMesh) -> Self {
        Self {
            positions: mesh.positions.into_iter().flat_map(point_values).collect(),
            normals: mesh.normals.into_iter().flat_map(vector_values).collect(),
            triangles: mesh
                .triangles
                .into_iter()
                .flatten()
                .map(|index| index as u32)
                .collect(),
        }
    }
}

#[wasm_bindgen(js_class = NativeMesh)]
impl NativeMesh {
    #[wasm_bindgen(getter)]
    pub fn positions(&self) -> Float64Array {
        f64_array(&self.positions)
    }
    #[wasm_bindgen(getter)]
    pub fn normals(&self) -> Float64Array {
        f64_array(&self.normals)
    }
    #[wasm_bindgen(getter)]
    pub fn triangles(&self) -> Uint32Array {
        Uint32Array::from(self.triangles.as_slice())
    }
}

#[wasm_bindgen(js_name = NativePolyline)]
pub struct NativePolyline {
    parameters: Vec<f64>,
    points: Vec<f64>,
}

#[wasm_bindgen(js_class = NativePolyline)]
impl NativePolyline {
    #[wasm_bindgen(getter)]
    pub fn parameters(&self) -> Float64Array {
        f64_array(&self.parameters)
    }
    #[wasm_bindgen(getter)]
    pub fn points(&self) -> Float64Array {
        f64_array(&self.points)
    }
}

#[wasm_bindgen(js_name = NativeFace)]
pub struct NativeFace {
    inner: FaceHandle,
}

#[wasm_bindgen(js_class = NativeFace)]
impl NativeFace {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        id_string(self.inner.persistent_id())
    }

    pub fn tessellate(
        &self,
        linear: f64,
        angular: f64,
        maximum_depth: u8,
    ) -> Result<NativeMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular, maximum_depth)?)
            .map(Into::into)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = surfaceKind)]
    pub fn surface_kind(&self) -> String {
        match self.inner.surface() {
            Surface3::Plane(_) => "plane",
            Surface3::Cylinder(_) => "cylinder",
            Surface3::Sphere(_) => "sphere",
            Surface3::Cone(_) => "cone",
            Surface3::Torus(_) => "torus",
            Surface3::Revolved(_) => "revolved",
            Surface3::Nurbs(_) => "nurbs",
            Surface3::Offset(_) => "offset",
            Surface3::RollingBall(_) => "rolling-ball",
        }
        .to_owned()
    }

    #[wasm_bindgen(js_name = surfaceFrame)]
    pub fn surface_frame(&self) -> Float64Array {
        let values = match self.inner.surface() {
            Surface3::Plane(surface) => vec![
                surface.origin().x,
                surface.origin().y,
                surface.origin().z,
                surface.u().x,
                surface.u().y,
                surface.u().z,
                surface.v().x,
                surface.v().y,
                surface.v().z,
            ],
            Surface3::Cylinder(surface) => vec![
                surface.origin().x,
                surface.origin().y,
                surface.origin().z,
                surface.axis().x,
                surface.axis().y,
                surface.axis().z,
            ],
            Surface3::Sphere(surface) => point_values(surface.center()).to_vec(),
            _ => Vec::new(),
        };
        f64_array(&values)
    }

    #[wasm_bindgen(js_name = boundaryEdges)]
    pub fn boundary_edges(&self) -> Result<NativeFaceEdges, JsValue> {
        self.inner
            .boundary_edges()
            .map(|edges| NativeFaceEdges { edges })
            .map_err(js_error)
    }
}

#[wasm_bindgen(js_name = NativeFaceEdges)]
pub struct NativeFaceEdges {
    edges: Vec<EdgeHandle>,
}

#[wasm_bindgen(js_class = NativeFaceEdges)]
impl NativeFaceEdges {
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.edges.len()
    }
    pub fn get(&self, index: usize) -> Option<NativeEdge> {
        self.edges
            .get(index)
            .cloned()
            .map(|inner| NativeEdge { inner })
    }
}

#[wasm_bindgen(js_name = NativeEdge)]
pub struct NativeEdge {
    inner: EdgeHandle,
}

#[wasm_bindgen(js_class = NativeEdge)]
impl NativeEdge {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        id_string(self.inner.persistent_id())
    }

    pub fn endpoints(&self) -> Result<Float64Array, JsValue> {
        let values = self
            .inner
            .endpoints()
            .map_err(js_error)?
            .into_iter()
            .flat_map(point_values)
            .collect::<Vec<_>>();
        Ok(f64_array(&values))
    }

    #[wasm_bindgen(js_name = curveKind)]
    pub fn curve_kind(&self) -> String {
        match self.inner.curve() {
            Curve3::Line(_) => "line",
            Curve3::Circle(_) => "circle",
            Curve3::Nurbs(_) => "nurbs",
            Curve3::SurfaceIntersection(_) => "surface-intersection",
            Curve3::SurfaceCurve(_) => "surface-curve",
        }
        .to_owned()
    }

    #[wasm_bindgen(js_name = curveData)]
    pub fn curve_data(&self) -> Float64Array {
        let values = match self.inner.curve() {
            Curve3::Line(line) => vec![
                line.origin().x,
                line.origin().y,
                line.origin().z,
                line.direction().x,
                line.direction().y,
                line.direction().z,
            ],
            Curve3::Circle(circle) => vec![
                circle.center().x,
                circle.center().y,
                circle.center().z,
                circle.x_radius().x,
                circle.x_radius().y,
                circle.x_radius().z,
                circle.y_radius().x,
                circle.y_radius().y,
                circle.y_radius().z,
            ],
            _ => Vec::new(),
        };
        f64_array(&values)
    }

    #[wasm_bindgen(js_name = connectedFaces)]
    pub fn connected_faces(&self) -> Result<NativeConnectedFaces, JsValue> {
        self.inner
            .connected_faces()
            .map(|faces| NativeConnectedFaces { faces })
            .map_err(js_error)
    }

    pub fn tessellate(
        &self,
        linear: f64,
        angular: f64,
        maximum_depth: u8,
    ) -> Result<NativePolyline, JsValue> {
        let polyline = self
            .inner
            .tessellate(tessellation_options(linear, angular, maximum_depth)?)
            .map_err(js_error)?;
        Ok(NativePolyline {
            parameters: polyline.parameters,
            points: polyline.points.into_iter().flat_map(point_values).collect(),
        })
    }
}

#[wasm_bindgen(js_name = NativeConnectedFaces)]
pub struct NativeConnectedFaces {
    faces: Vec<FaceHandle>,
}

#[wasm_bindgen(js_class = NativeConnectedFaces)]
impl NativeConnectedFaces {
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.faces.len()
    }
    pub fn get(&self, index: usize) -> Option<NativeFace> {
        self.faces
            .get(index)
            .cloned()
            .map(|inner| NativeFace { inner })
    }
}

#[wasm_bindgen(js_name = NativeVertex)]
pub struct NativeVertex {
    inner: VertexHandle,
}

#[wasm_bindgen(js_class = NativeVertex)]
impl NativeVertex {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        id_string(self.inner.persistent_id())
    }
    #[wasm_bindgen(getter)]
    pub fn point(&self) -> Float64Array {
        f64_array(&point_values(self.inner.point()))
    }
}

#[wasm_bindgen(js_name = NativeSolid)]
pub struct NativeSolid {
    inner: Solid,
}

#[wasm_bindgen(js_class = NativeSolid)]
impl NativeSolid {
    pub fn volume(&self) -> f64 {
        self.inner.volume()
    }
    #[wasm_bindgen(js_name = surfaceArea)]
    pub fn surface_area(&self) -> f64 {
        self.inner.surface_area()
    }
    pub fn centroid(&self) -> Float64Array {
        f64_array(&point_values(self.inner.centroid()))
    }
    #[wasm_bindgen(getter, js_name = faceCount)]
    pub fn face_count(&self) -> usize {
        self.inner.faces().len()
    }
    #[wasm_bindgen(getter, js_name = edgeCount)]
    pub fn edge_count(&self) -> usize {
        self.inner.edges().len()
    }
    #[wasm_bindgen(getter, js_name = vertexCount)]
    pub fn vertex_count(&self) -> usize {
        self.inner.vertices().len()
    }
    pub fn face(&self, index: usize) -> Option<NativeFace> {
        self.inner
            .faces()
            .nth(index)
            .cloned()
            .map(|inner| NativeFace { inner })
    }
    pub fn edge(&self, index: usize) -> Option<NativeEdge> {
        self.inner
            .edges()
            .nth(index)
            .cloned()
            .map(|inner| NativeEdge { inner })
    }
    pub fn vertex(&self, index: usize) -> Option<NativeVertex> {
        self.inner
            .vertices()
            .nth(index)
            .cloned()
            .map(|inner| NativeVertex { inner })
    }
    pub fn tessellate(
        &self,
        linear: f64,
        angular: f64,
        maximum_depth: u8,
    ) -> Result<NativeMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular, maximum_depth)?)
            .map(Into::into)
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name = toCompound)]
    pub fn to_compound(&self) -> NativeCompound {
        NativeCompound {
            inner: self.inner.to_compound(),
        }
    }
}

#[wasm_bindgen(js_name = NativeFaceList)]
pub struct NativeFaceList {
    values: Vec<FaceHandle>,
}

#[wasm_bindgen(js_class = NativeFaceList)]
impl NativeFaceList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }
    pub fn add(&mut self, value: &NativeFace) {
        self.values.push(value.inner.clone())
    }
}

#[wasm_bindgen(js_name = NativeEdgeList)]
pub struct NativeEdgeList {
    values: Vec<EdgeHandle>,
}

#[wasm_bindgen(js_class = NativeEdgeList)]
impl NativeEdgeList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }
    pub fn add(&mut self, value: &NativeEdge) {
        self.values.push(value.inner.clone())
    }
}

#[wasm_bindgen(js_name = NativeSolidList)]
pub struct NativeSolidList {
    values: Vec<Solid>,
}

#[wasm_bindgen(js_class = NativeSolidList)]
impl NativeSolidList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }
    pub fn add(&mut self, value: &NativeSolid) {
        self.values.push(value.inner.clone())
    }
}

#[wasm_bindgen(js_name = NativeResolution)]
pub struct NativeResolution {
    kind: String,
    faces: Vec<FaceHandle>,
    edges: Vec<EdgeHandle>,
    vertices: Vec<VertexHandle>,
}

#[wasm_bindgen(js_class = NativeResolution)]
impl NativeResolution {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        self.faces.len() + self.edges.len() + self.vertices.len()
    }
    pub fn face(&self, index: usize) -> Option<NativeFace> {
        self.faces
            .get(index)
            .cloned()
            .map(|inner| NativeFace { inner })
    }
    pub fn edge(&self, index: usize) -> Option<NativeEdge> {
        self.edges
            .get(index)
            .cloned()
            .map(|inner| NativeEdge { inner })
    }
    pub fn vertex(&self, index: usize) -> Option<NativeVertex> {
        self.vertices
            .get(index)
            .cloned()
            .map(|inner| NativeVertex { inner })
    }
}

fn face_resolution(value: Resolution<FaceHandle>) -> NativeResolution {
    let (kind, faces) = match value {
        Resolution::Preserved(value) => ("preserved", vec![value]),
        Resolution::Modified(value) => ("modified", vec![value]),
        Resolution::Split(values) => ("split", values),
        Resolution::Merged(value) => ("merged", vec![value]),
        Resolution::Deleted => ("deleted", Vec::new()),
        Resolution::Unrelated => ("unrelated", Vec::new()),
    };
    NativeResolution {
        kind: kind.into(),
        faces,
        edges: Vec::new(),
        vertices: Vec::new(),
    }
}

fn edge_resolution(value: Resolution<EdgeHandle>) -> NativeResolution {
    let (kind, edges) = match value {
        Resolution::Preserved(value) => ("preserved", vec![value]),
        Resolution::Modified(value) => ("modified", vec![value]),
        Resolution::Split(values) => ("split", values),
        Resolution::Merged(value) => ("merged", vec![value]),
        Resolution::Deleted => ("deleted", Vec::new()),
        Resolution::Unrelated => ("unrelated", Vec::new()),
    };
    NativeResolution {
        kind: kind.into(),
        faces: Vec::new(),
        edges,
        vertices: Vec::new(),
    }
}

fn vertex_resolution(value: Resolution<VertexHandle>) -> NativeResolution {
    let (kind, vertices) = match value {
        Resolution::Preserved(value) => ("preserved", vec![value]),
        Resolution::Modified(value) => ("modified", vec![value]),
        Resolution::Split(values) => ("split", values),
        Resolution::Merged(value) => ("merged", vec![value]),
        Resolution::Deleted => ("deleted", Vec::new()),
        Resolution::Unrelated => ("unrelated", Vec::new()),
    };
    NativeResolution {
        kind: kind.into(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices,
    }
}

#[wasm_bindgen(js_name = NativeCreationRole)]
pub struct NativeCreationRole {
    kind: String,
    boundary: Option<String>,
    hole_index: Option<usize>,
    segment: Option<usize>,
    part: Option<usize>,
}

#[wasm_bindgen(js_class = NativeCreationRole)]
impl NativeCreationRole {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn boundary(&self) -> Option<String> {
        self.boundary.clone()
    }
    #[wasm_bindgen(getter, js_name = holeIndex)]
    pub fn hole_index(&self) -> Option<usize> {
        self.hole_index
    }
    #[wasm_bindgen(getter)]
    pub fn segment(&self) -> Option<usize> {
        self.segment
    }
    #[wasm_bindgen(getter)]
    pub fn part(&self) -> Option<usize> {
        self.part
    }
}

fn creation_role(role: &CreationRole) -> NativeCreationRole {
    match role {
        CreationRole::ExtrusionStart => NativeCreationRole {
            kind: "extrusion-start".into(),
            boundary: None,
            hole_index: None,
            segment: None,
            part: None,
        },
        CreationRole::ExtrusionEnd => NativeCreationRole {
            kind: "extrusion-end".into(),
            boundary: None,
            hole_index: None,
            segment: None,
            part: None,
        },
        CreationRole::ExtrusionSide {
            boundary,
            segment,
            part,
        } => {
            let (boundary, hole_index) = match boundary {
                ProfileBoundary::Outer => ("outer".into(), None),
                ProfileBoundary::Hole(index) => ("hole".into(), Some(*index)),
            };
            NativeCreationRole {
                kind: "extrusion-side".into(),
                boundary: Some(boundary),
                hole_index,
                segment: Some(*segment),
                part: Some(*part),
            }
        }
    }
}

#[wasm_bindgen(js_name = NativeCompound)]
pub struct NativeCompound {
    inner: Compound,
}

#[wasm_bindgen(js_class = NativeCompound)]
impl NativeCompound {
    pub fn volume(&self) -> f64 {
        self.inner.volume()
    }
    #[wasm_bindgen(js_name = surfaceArea)]
    pub fn surface_area(&self) -> f64 {
        self.inner.surface_area()
    }
    pub fn centroid(&self) -> Float64Array {
        f64_array(&point_values(self.inner.centroid()))
    }
    #[wasm_bindgen(getter, js_name = solidCount)]
    pub fn solid_count(&self) -> usize {
        self.inner.solids().len()
    }
    #[wasm_bindgen(getter, js_name = faceCount)]
    pub fn face_count(&self) -> usize {
        self.inner.faces().count()
    }
    #[wasm_bindgen(getter, js_name = edgeCount)]
    pub fn edge_count(&self) -> usize {
        self.inner.edges().count()
    }
    #[wasm_bindgen(getter, js_name = vertexCount)]
    pub fn vertex_count(&self) -> usize {
        self.inner.vertices().count()
    }
    pub fn solid(&self, index: usize) -> Option<NativeSolid> {
        self.inner
            .solids()
            .nth(index)
            .cloned()
            .map(|inner| NativeSolid { inner })
    }
    pub fn face(&self, index: usize) -> Option<NativeFace> {
        self.inner
            .faces()
            .nth(index)
            .cloned()
            .map(|inner| NativeFace { inner })
    }
    pub fn edge(&self, index: usize) -> Option<NativeEdge> {
        self.inner
            .edges()
            .nth(index)
            .cloned()
            .map(|inner| NativeEdge { inner })
    }
    pub fn vertex(&self, index: usize) -> Option<NativeVertex> {
        self.inner
            .vertices()
            .nth(index)
            .cloned()
            .map(|inner| NativeVertex { inner })
    }
    pub fn tessellate(
        &self,
        linear: f64,
        angular: f64,
        maximum_depth: u8,
    ) -> Result<NativeMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular, maximum_depth)?)
            .map(Into::into)
            .map_err(js_error)
    }
    pub fn boolean(&self, other: &NativeCompound, operation: &str) -> Result<Self, JsValue> {
        let kind = match operation {
            "union" => BooleanKind::Union,
            "difference" => BooleanKind::Difference,
            "intersection" => BooleanKind::Intersection,
            _ => return Err(JsValue::from_str("unknown Boolean operation")),
        };
        self.inner
            .boolean(&other.inner, kind)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    pub fn combined(&self, other: &NativeCompound) -> Result<Self, JsValue> {
        self.inner
            .combined(std::slice::from_ref(&other.inner))
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    pub fn shell(&self, openings: &NativeFaceList, distance: f64) -> Result<Self, JsValue> {
        self.inner
            .shell(&openings.values, distance)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    pub fn fillet(
        &self,
        edges: &NativeEdgeList,
        radius: f64,
        follow_tangent_edges: bool,
    ) -> Result<Self, JsValue> {
        self.inner
            .fillet_with_options(
                &edges.values,
                FilletOptions::new(radius).with_tangent_propagation(follow_tangent_edges),
            )
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    pub fn split(
        &self,
        plane_values: &[f64],
        keep: &str,
        tolerance_values: &[f64],
    ) -> Result<Self, JsValue> {
        let tolerance = tolerance(tolerance_values)?;
        self.inner
            .split(plane(plane_values, tolerance)?, keep)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name = withoutSolids)]
    pub fn without_solids(&self, removed: &NativeSolidList) -> Result<Self, JsValue> {
        self.inner
            .without_solids(&removed.values)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name = moveFaces)]
    pub fn move_faces(
        &self,
        faces: &NativeFaceList,
        transform_values: &[f64],
        tolerance_values: &[f64],
    ) -> Result<Self, JsValue> {
        let tolerance = tolerance(tolerance_values)?;
        self.inner
            .move_faces(&faces.values, transform(transform_values, tolerance)?)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name = replaceFace)]
    pub fn replace_face(
        &self,
        target: &NativeFace,
        replacement: &NativeFace,
    ) -> Result<Self, JsValue> {
        self.inner
            .replace_face(&target.inner, &replacement.inner)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    pub fn transformed(
        &self,
        transform_values: &[f64],
        tolerance_values: &[f64],
    ) -> Result<Self, JsValue> {
        let tolerance = tolerance(tolerance_values)?;
        self.inner
            .transformed(transform(transform_values, tolerance)?)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }
    #[wasm_bindgen(js_name = resolveFace)]
    pub fn resolve_face(&self, value: &NativeFace) -> NativeResolution {
        face_resolution(self.inner.resolve(&value.inner))
    }
    #[wasm_bindgen(js_name = resolveEdge)]
    pub fn resolve_edge(&self, value: &NativeEdge) -> NativeResolution {
        edge_resolution(self.inner.resolve(&value.inner))
    }
    #[wasm_bindgen(js_name = resolveVertex)]
    pub fn resolve_vertex(&self, value: &NativeVertex) -> NativeResolution {
        vertex_resolution(self.inner.resolve(&value.inner))
    }
    #[wasm_bindgen(js_name = isSemanticEdge)]
    pub fn is_semantic_edge(&self, value: &NativeEdge) -> bool {
        self.inner.is_semantic_edge(&value.inner)
    }
    #[wasm_bindgen(js_name = creationRoleFace)]
    pub fn creation_role_face(&self, value: &NativeFace) -> Option<NativeCreationRole> {
        match self.inner.origin(&value.inner) {
            Some(Origin::Created { role: Some(role) }) => Some(creation_role(role)),
            _ => None,
        }
    }
    #[wasm_bindgen(js_name = creationRoleEdge)]
    pub fn creation_role_edge(&self, value: &NativeEdge) -> Option<NativeCreationRole> {
        match self.inner.origin(&value.inner) {
            Some(Origin::Created { role: Some(role) }) => Some(creation_role(role)),
            _ => None,
        }
    }
    #[wasm_bindgen(js_name = facesGeneratedFromFace)]
    pub fn faces_generated_from_face(&self, value: &NativeFace) -> NativeConnectedFaces {
        NativeConnectedFaces {
            faces: self.inner.faces_generated_from(&value.inner),
        }
    }
    #[wasm_bindgen(js_name = facesGeneratedFromEdge)]
    pub fn faces_generated_from_edge(&self, value: &NativeEdge) -> NativeConnectedFaces {
        NativeConnectedFaces {
            faces: self.inner.faces_generated_from(&value.inner),
        }
    }
}
