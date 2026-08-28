//! Thin WebAssembly transport for ProtoSolid.
//!
//! This crate deliberately lives outside Shapex. It translates JavaScript
//! values at the boundary and exposes only Shapex's owned, high-level API.

use js_sys::Float64Array;
use shapex::kernel::{
    Axis3, BooleanKind, Curve3, NurbsCurve2, Origin, ParameterRange, RigidTransform3,
    Surface3,
};
use shapex::{
    Compound, CreationRole, EdgeHandle, FaceHandle, Matrix3, ModelTolerance, Plane3, Point2,
    Profile, ProfileBoundary, Resolution, Solid, TessellationOptions, TriangleMesh, TrimmedCurve2,
    Vec3, Wire2,
};
use wasm_bindgen::prelude::*;

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn id_string(id: shapex::kernel::PersistentId) -> String {
    format!("{}:{:016x}", id.namespace, id.local)
}

fn array(values: &[f64]) -> Float64Array {
    Float64Array::from(values)
}

fn point_values(point: shapex::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn vector_values(vector: shapex::Vec3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

#[wasm_bindgen(js_name = Wire2)]
pub struct WasmWire2 {
    segments: Vec<TrimmedCurve2>,
}

#[wasm_bindgen(js_class = Wire2)]
impl WasmWire2 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = addLine)]
    pub fn add_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), JsValue> {
        self.segments
            .push(TrimmedCurve2::line(Point2::new(x1, y1), Point2::new(x2, y2)).map_err(js_error)?);
        Ok(())
    }

    #[wasm_bindgen(js_name = addCircle)]
    pub fn add_circle(&mut self, x: f64, y: f64, radius: f64) -> Result<(), JsValue> {
        self.segments
            .push(TrimmedCurve2::circle(Point2::new(x, y), radius).map_err(js_error)?);
        Ok(())
    }

    #[wasm_bindgen(js_name = addArcThrough)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_arc_through(
        &mut self,
        start_x: f64,
        start_y: f64,
        middle_x: f64,
        middle_y: f64,
        end_x: f64,
        end_y: f64,
    ) -> Result<(), JsValue> {
        self.segments.push(
            TrimmedCurve2::arc_through(
                Point2::new(start_x, start_y),
                Point2::new(middle_x, middle_y),
                Point2::new(end_x, end_y),
            )
            .map_err(js_error)?,
        );
        Ok(())
    }

    #[wasm_bindgen(js_name = addNurbs)]
    pub fn add_nurbs(
        &mut self,
        degree: usize,
        control_points: &[f64],
        weights: &[f64],
        knots: &[f64],
    ) -> Result<(), JsValue> {
        if control_points.len() % 2 != 0 || control_points.len() / 2 != weights.len() {
            return Err(JsValue::from_str(
                "a planar NURBS needs one XY control point per weight",
            ));
        }
        let points = control_points
            .chunks_exact(2)
            .map(|point| Point2::new(point[0], point[1]))
            .collect::<Vec<_>>();
        let range = ParameterRange::new(
            *knots.get(degree).ok_or_else(|| JsValue::from_str("invalid NURBS knots"))?,
            *knots
                .get(points.len())
                .ok_or_else(|| JsValue::from_str("invalid NURBS knots"))?,
        )
        .map_err(js_error)?;
        let curve = NurbsCurve2::new(degree, points, weights.to_vec(), knots.to_vec())
            .map_err(js_error)?;
        self.segments
            .push(TrimmedCurve2::new(curve, range).map_err(js_error)?);
        Ok(())
    }
}

impl WasmWire2 {
    fn build(self, tolerance: ModelTolerance) -> Result<Wire2, JsValue> {
        Wire2::new(self.segments, tolerance).map_err(js_error)
    }
}

impl Default for WasmWire2 {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_name = Profile)]
pub struct WasmProfile2 {
    outer: Option<WasmWire2>,
    holes: Vec<WasmWire2>,
}

#[wasm_bindgen(js_class = Profile)]
impl WasmProfile2 {
    fn take_profile(&mut self, tolerance: ModelTolerance) -> Result<Profile, JsValue> {
        let outer = self
            .outer
            .take()
            .ok_or_else(|| JsValue::from_str("a bridge profile can only be consumed once"))?
            .build(tolerance)?;
        let holes = std::mem::take(&mut self.holes)
            .into_iter()
            .map(|wire| wire.build(tolerance))
            .collect::<Result<Vec<_>, _>>()?;
        Profile::new(outer, holes, tolerance).map_err(js_error)
    }

    #[wasm_bindgen(constructor)]
    pub fn new(outer: WasmWire2) -> Self {
        Self {
            outer: Some(outer),
            holes: Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = addHole)]
    pub fn add_hole(&mut self, hole: WasmWire2) {
        self.holes.push(hole);
    }

    #[wasm_bindgen(js_name = extrude)]
    #[allow(clippy::too_many_arguments)]
    pub fn extrude(
        &mut self,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
        u_x: f64,
        u_y: f64,
        u_z: f64,
        v_x: f64,
        v_y: f64,
        v_z: f64,
        vector_x: f64,
        vector_y: f64,
        vector_z: f64,
    ) -> Result<WasmCompound, JsValue> {
        let tolerance = ModelTolerance::default();
        let profile = self.take_profile(tolerance)?;
        let plane = Plane3::new(
            shapex::Point3::new(origin_x, origin_y, origin_z),
            Vec3::new(u_x, u_y, u_z),
            Vec3::new(v_x, v_y, v_z),
            tolerance,
        )
        .map_err(js_error)?;
        let compound = profile
            .extrude(plane, Vec3::new(vector_x, vector_y, vector_z))
            .map_err(js_error)?;
        Ok(WasmCompound { inner: compound })
    }

    #[wasm_bindgen(js_name = revolve)]
    #[allow(clippy::too_many_arguments)]
    pub fn revolve(
        &mut self,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
        u_x: f64,
        u_y: f64,
        u_z: f64,
        v_x: f64,
        v_y: f64,
        v_z: f64,
        axis_x: f64,
        axis_y: f64,
        axis_z: f64,
        direction_x: f64,
        direction_y: f64,
        direction_z: f64,
        angle: f64,
    ) -> Result<WasmCompound, JsValue> {
        let tolerance = ModelTolerance::default();
        let profile = self.take_profile(tolerance)?;
        let plane = Plane3::new(
            shapex::Point3::new(origin_x, origin_y, origin_z),
            Vec3::new(u_x, u_y, u_z),
            Vec3::new(v_x, v_y, v_z),
            tolerance,
        )
        .map_err(js_error)?;
        let axis = Axis3::new(
            shapex::Point3::new(axis_x, axis_y, axis_z),
            Vec3::new(direction_x, direction_y, direction_z),
        )
        .map_err(js_error)?;
        let compound = profile.revolve(plane, axis, angle).map_err(js_error)?;
        Ok(WasmCompound { inner: compound })
    }
}

#[wasm_bindgen(js_name = Loft)]
pub struct WasmLoft {
    sections: Vec<(Plane3, Profile)>,
}

#[wasm_bindgen(js_class = Loft)]
impl WasmLoft {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = addProfile)]
    #[allow(clippy::too_many_arguments)]
    pub fn add_profile(
        &mut self,
        mut profile: WasmProfile2,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
        u_x: f64,
        u_y: f64,
        u_z: f64,
        v_x: f64,
        v_y: f64,
        v_z: f64,
    ) -> Result<(), JsValue> {
        let tolerance = ModelTolerance::default();
        let profile = profile.take_profile(tolerance)?;
        let plane = Plane3::new(
            shapex::Point3::new(origin_x, origin_y, origin_z),
            Vec3::new(u_x, u_y, u_z),
            Vec3::new(v_x, v_y, v_z),
            tolerance,
        )
        .map_err(js_error)?;
        self.sections.push((plane, profile));
        Ok(())
    }

    pub fn build(&mut self, align_sections: bool) -> Result<WasmCompound, JsValue> {
        let sections = std::mem::take(&mut self.sections);
        let Some((plane, first)) = sections.first() else {
            return Err(JsValue::from_str("a loft needs at least two profiles"));
        };
        if sections.len() < 2 {
            return Err(JsValue::from_str("a loft needs at least two profiles"));
        }
        let compound = first
            .loft_with_alignment(
                *plane,
                sections.iter().skip(1).map(|(plane, profile)| (*plane, profile)),
                align_sections,
            )
            .map_err(js_error)?;
        Ok(WasmCompound { inner: compound })
    }
}

impl Default for WasmLoft {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_name = Mesh)]
pub struct WasmMesh {
    positions: Vec<f64>,
    normals: Vec<f64>,
}

impl From<TriangleMesh> for WasmMesh {
    fn from(mesh: TriangleMesh) -> Self {
        let mut positions = Vec::with_capacity(mesh.triangles.len() * 9);
        let mut normals = Vec::with_capacity(mesh.triangles.len() * 9);
        for triangle in mesh.triangles {
            for index in triangle {
                positions.extend(point_values(mesh.positions[index]));
                normals.extend(vector_values(mesh.normals[index]));
            }
        }
        Self { positions, normals }
    }
}

#[wasm_bindgen(js_class = Mesh)]
impl WasmMesh {
    #[wasm_bindgen(getter)]
    pub fn positions(&self) -> Float64Array {
        array(&self.positions)
    }

    #[wasm_bindgen(getter)]
    pub fn normals(&self) -> Float64Array {
        array(&self.normals)
    }
}

#[wasm_bindgen(js_name = Face)]
pub struct WasmFace {
    inner: FaceHandle,
}

#[wasm_bindgen(js_name = FaceList)]
pub struct WasmFaceList {
    faces: Vec<FaceHandle>,
}

#[wasm_bindgen(js_class = FaceList)]
impl WasmFaceList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { faces: Vec::new() }
    }

    pub fn add(&mut self, face: &WasmFace) {
        self.faces.push(face.inner.clone());
    }
}

impl Default for WasmFaceList {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_name = FaceResolution)]
pub struct WasmFaceResolution {
    kind: String,
    faces: Vec<FaceHandle>,
}

#[wasm_bindgen(js_class = FaceResolution)]
impl WasmFaceResolution {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn count(&self) -> usize {
        self.faces.len()
    }

    pub fn face(&self, index: usize) -> Option<WasmFace> {
        self.faces
            .get(index)
            .cloned()
            .map(|inner| WasmFace { inner })
    }
}

#[wasm_bindgen(js_class = Face)]
impl WasmFace {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        id_string(self.inner.persistent_id())
    }

    pub fn tessellate(&self, linear: f64, angular: f64) -> Result<WasmMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular))
            .map(Into::into)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = planeFrame)]
    pub fn plane_frame(&self) -> Float64Array {
        let Surface3::Plane(plane) = self.inner.surface() else {
            return array(&[]);
        };
        array(&[
            plane.origin().x,
            plane.origin().y,
            plane.origin().z,
            plane.u().x,
            plane.u().y,
            plane.u().z,
            plane.v().x,
            plane.v().y,
            plane.v().z,
        ])
    }

    #[wasm_bindgen(js_name = axisFrame)]
    pub fn axis_frame(&self) -> Float64Array {
        let Surface3::Cylinder(cylinder) = self.inner.surface() else {
            return array(&[]);
        };
        array(&[
            cylinder.origin().x,
            cylinder.origin().y,
            cylinder.origin().z,
            cylinder.axis().x,
            cylinder.axis().y,
            cylinder.axis().z,
        ])
    }

    #[wasm_bindgen(js_name = centerPoint)]
    pub fn center_point(&self) -> Float64Array {
        let Surface3::Sphere(sphere) = self.inner.surface() else {
            return array(&[]);
        };
        array(&point_values(sphere.center()))
    }

    #[wasm_bindgen(js_name = boundaryEdgeIds)]
    pub fn boundary_edge_ids(&self) -> Result<Box<[JsValue]>, JsValue> {
        self.inner
            .boundary_edges()
            .map(|edges| {
                edges
                    .into_iter()
                    .map(|edge| JsValue::from_str(&id_string(edge.persistent_id())))
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            })
            .map_err(js_error)
    }
}

#[wasm_bindgen(js_name = Edge)]
pub struct WasmEdge {
    inner: EdgeHandle,
}

#[wasm_bindgen(js_name = EdgeList)]
pub struct WasmEdgeList {
    edges: Vec<EdgeHandle>,
}

#[wasm_bindgen(js_class = EdgeList)]
impl WasmEdgeList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { edges: Vec::new() }
    }

    pub fn add(&mut self, edge: &WasmEdge) {
        self.edges.push(edge.inner.clone());
    }
}

impl Default for WasmEdgeList {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_class = Edge)]
impl WasmEdge {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        id_string(self.inner.persistent_id())
    }

    pub fn tessellate(&self, linear: f64, angular: f64) -> Result<Float64Array, JsValue> {
        let polyline = self
            .inner
            .tessellate(tessellation_options(linear, angular))
            .map_err(js_error)?;
        let points = polyline
            .points
            .into_iter()
            .flat_map(point_values)
            .collect::<Vec<_>>();
        Ok(array(&points))
    }

    pub fn endpoints(&self) -> Result<Float64Array, JsValue> {
        let points = self.inner.endpoints().map_err(js_error)?;
        Ok(array(
            &points
                .into_iter()
                .flat_map(point_values)
                .collect::<Vec<_>>(),
        ))
    }

    #[wasm_bindgen(js_name = connectedFaceIds)]
    pub fn connected_face_ids(&self) -> Result<Box<[JsValue]>, JsValue> {
        self.inner
            .connected_faces()
            .map(|faces| {
                faces
                    .into_iter()
                    .map(|face| JsValue::from_str(&id_string(face.persistent_id())))
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            })
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = lineAxis)]
    pub fn line_axis(&self) -> Float64Array {
        let Curve3::Line(line) = self.inner.curve() else {
            return array(&[]);
        };
        array(&[
            line.origin().x,
            line.origin().y,
            line.origin().z,
            line.direction().x,
            line.direction().y,
            line.direction().z,
        ])
    }
}

#[wasm_bindgen(js_name = Solid)]
pub struct WasmSolid {
    inner: Solid,
}

#[wasm_bindgen(js_name = SolidList)]
pub struct WasmSolidList {
    solids: Vec<Solid>,
}

#[wasm_bindgen(js_class = SolidList)]
impl WasmSolidList {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { solids: Vec::new() }
    }

    pub fn add(&mut self, solid: &WasmSolid) {
        self.solids.push(solid.inner.clone());
    }
}

impl Default for WasmSolidList {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen(js_class = Solid)]
impl WasmSolid {
    #[wasm_bindgen(getter, js_name = faceCount)]
    pub fn face_count(&self) -> usize {
        self.inner.faces().len()
    }

    #[wasm_bindgen(getter, js_name = edgeCount)]
    pub fn edge_count(&self) -> usize {
        self.inner.edges().len()
    }

    pub fn face(&self, index: usize) -> Option<WasmFace> {
        self.inner
            .faces()
            .nth(index)
            .cloned()
            .map(|inner| WasmFace { inner })
    }

    pub fn edge(&self, index: usize) -> Option<WasmEdge> {
        self.inner
            .edges()
            .nth(index)
            .cloned()
            .map(|inner| WasmEdge { inner })
    }

    pub fn volume(&self) -> f64 {
        self.inner.volume()
    }

    pub fn center(&self) -> Float64Array {
        array(&point_values(self.inner.centroid()))
    }

    pub fn tessellate(&self, linear: f64, angular: f64) -> Result<WasmMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular))
            .map(Into::into)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = toCompound)]
    pub fn to_compound(&self) -> WasmCompound {
        WasmCompound {
            inner: self.inner.to_compound(),
        }
    }
}

#[wasm_bindgen(js_name = Compound)]
pub struct WasmCompound {
    inner: Compound,
}

#[wasm_bindgen(js_name = CreationRole)]
pub struct WasmCreationRole {
    kind: &'static str,
    boundary: Option<&'static str>,
    hole: Option<usize>,
    segment: Option<usize>,
    part: Option<usize>,
}

#[wasm_bindgen(js_class = CreationRole)]
impl WasmCreationRole {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.to_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn boundary(&self) -> Option<String> {
        self.boundary.map(str::to_owned)
    }

    #[wasm_bindgen(getter, js_name = holeIndex)]
    pub fn hole_index(&self) -> Option<usize> {
        self.hole
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

#[wasm_bindgen(js_class = Compound)]
impl WasmCompound {
    #[wasm_bindgen(getter, js_name = solidCount)]
    pub fn solid_count(&self) -> usize {
        self.inner.solids().len()
    }

    pub fn solid(&self, index: usize) -> Option<WasmSolid> {
        self.inner
            .solids()
            .nth(index)
            .cloned()
            .map(|inner| WasmSolid { inner })
    }

    pub fn volume(&self) -> f64 {
        self.inner.volume()
    }

    pub fn center(&self) -> Float64Array {
        array(&point_values(self.inner.centroid()))
    }

    pub fn tessellate(&self, linear: f64, angular: f64) -> Result<WasmMesh, JsValue> {
        self.inner
            .tessellate(tessellation_options(linear, angular))
            .map(Into::into)
            .map_err(js_error)
    }

    pub fn boolean(&self, other: &WasmCompound, operation: &str) -> Result<Self, JsValue> {
        let kind = match operation {
            "join" => BooleanKind::Union,
            "cut" => BooleanKind::Difference,
            "intersect" => BooleanKind::Intersection,
            _ => return Err(JsValue::from_str("unknown Boolean operation")),
        };
        self.inner
            .boolean(&other.inner, kind)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    pub fn combined(&self, other: &WasmCompound) -> Result<Self, JsValue> {
        self.inner
            .combined(std::slice::from_ref(&other.inner))
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    pub fn shell(&self, openings: &WasmFaceList, distance: f64) -> Result<Self, JsValue> {
        self.inner
            .shell(&openings.faces, distance)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    pub fn fillet(&self, edges: &WasmEdgeList, radius: f64) -> Result<Self, JsValue> {
        self.inner
            .fillet(&edges.edges, radius)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split(
        &self,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
        u_x: f64,
        u_y: f64,
        u_z: f64,
        v_x: f64,
        v_y: f64,
        v_z: f64,
        keep: &str,
    ) -> Result<Self, JsValue> {
        let plane = Plane3::new(
            shapex::Point3::new(origin_x, origin_y, origin_z),
            Vec3::new(u_x, u_y, u_z),
            Vec3::new(v_x, v_y, v_z),
            ModelTolerance::default(),
        ).map_err(js_error)?;
        self.inner.split(plane, keep).map(|inner| Self { inner }).map_err(js_error)
    }

    #[wasm_bindgen(js_name = withoutSolids)]
    pub fn without_solids(&self, removed: &WasmSolidList) -> Result<Self, JsValue> {
        self.inner
            .without_solids(&removed.solids)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = moveFaces)]
    #[allow(clippy::too_many_arguments)]
    pub fn move_faces(
        &self,
        faces: &WasmFaceList,
        x_x: f64, x_y: f64, x_z: f64,
        y_x: f64, y_y: f64, y_z: f64,
        z_x: f64, z_y: f64, z_z: f64,
        t_x: f64, t_y: f64, t_z: f64,
    ) -> Result<Self, JsValue> {
        let transform = RigidTransform3::new(
            Matrix3::from_cols(
                Vec3::new(x_x, x_y, x_z),
                Vec3::new(y_x, y_y, y_z),
                Vec3::new(z_x, z_y, z_z),
            ),
            Vec3::new(t_x, t_y, t_z),
            ModelTolerance::default(),
        ).map_err(js_error)?;
        self.inner.move_faces(&faces.faces, transform)
            .map(|inner| Self { inner }).map_err(js_error)
    }

    #[wasm_bindgen(js_name = replaceFace)]
    pub fn replace_face(
        &self,
        target: &WasmFace,
        replacement: &WasmFace,
    ) -> Result<Self, JsValue> {
        self.inner
            .replace_face(&target.inner, &replacement.inner)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = resolveFace)]
    pub fn resolve_face(&self, face: &WasmFace) -> WasmFaceResolution {
        let (kind, faces) = match self.inner.resolve(&face.inner) {
            Resolution::Preserved(face) => ("preserved", vec![face]),
            Resolution::Modified(face) => ("modified", vec![face]),
            Resolution::Split(faces) => ("split", faces),
            Resolution::Merged(face) => ("merged", vec![face]),
            Resolution::Deleted => ("deleted", Vec::new()),
            Resolution::Unrelated => ("unrelated", Vec::new()),
        };
        WasmFaceResolution {
            kind: kind.to_owned(),
            faces,
        }
    }

    #[wasm_bindgen(js_name = transformed)]
    #[allow(clippy::too_many_arguments)]
    pub fn transformed(
        &self,
        x_x: f64,
        x_y: f64,
        x_z: f64,
        y_x: f64,
        y_y: f64,
        y_z: f64,
        z_x: f64,
        z_y: f64,
        z_z: f64,
        t_x: f64,
        t_y: f64,
        t_z: f64,
    ) -> Result<Self, JsValue> {
        let transform = RigidTransform3::new(
            Matrix3::from_cols(
                Vec3::new(x_x, x_y, x_z),
                Vec3::new(y_x, y_y, y_z),
                Vec3::new(z_x, z_y, z_z),
            ),
            Vec3::new(t_x, t_y, t_z),
            ModelTolerance::default(),
        )
        .map_err(js_error)?;
        self.inner
            .transformed(transform)
            .map(|inner| Self { inner })
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = isSemanticEdge)]
    pub fn is_semantic_edge(&self, edge: &WasmEdge) -> bool {
        self.inner.is_semantic_edge(&edge.inner)
    }

    #[wasm_bindgen(js_name = creationRole)]
    pub fn creation_role(&self, face: &WasmFace) -> Option<WasmCreationRole> {
        match self.inner.origin(&face.inner) {
            Some(Origin::Created {
                role: Some(CreationRole::ExtrusionStart),
            }) => Some(WasmCreationRole {
                kind: "extrusion-start",
                boundary: None,
                hole: None,
                segment: None,
                part: None,
            }),
            Some(Origin::Created {
                role: Some(CreationRole::ExtrusionEnd),
            }) => Some(WasmCreationRole {
                kind: "extrusion-end",
                boundary: None,
                hole: None,
                segment: None,
                part: None,
            }),
            Some(Origin::Created {
                role:
                    Some(CreationRole::ExtrusionSide {
                        boundary,
                        segment,
                        part,
                    }),
            }) => {
                let (boundary, hole) = match boundary {
                    ProfileBoundary::Outer => ("outer", None),
                    ProfileBoundary::Hole(index) => ("hole", Some(*index)),
                };
                Some(WasmCreationRole {
                    kind: "extrusion-side",
                    boundary: Some(boundary),
                    hole,
                    segment: Some(*segment),
                    part: Some(*part),
                })
            }
            _ => None,
        }
    }
}

fn tessellation_options(linear: f64, angular: f64) -> TessellationOptions {
    TessellationOptions {
        chord_tolerance: linear,
        angular_tolerance: angular,
        ..TessellationOptions::default()
    }
}
