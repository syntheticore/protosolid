use std::rc::Rc;
use std::cell::RefCell;

use uuid::Uuid;
use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;


#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}

#[wasm_bindgen]
extern {
  fn alert(s: &str);
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
  #[cfg(debug_assertions)]
  console_error_panic_hook::set_once();

  log!("Alchemy running");

  Ok(())
}


fn point_to_js(p: Point3) -> JsValue {
  JsValue::from_serde(&(p.x, p.y, p.z)).unwrap()
}

fn points_to_js(points: Vec<Point3>) -> Array {
  points.into_iter().map(point_to_js).collect()
}

fn point_from_js(p: JsValue) -> Point3 {
  let p: (f64, f64, f64) = p.into_serde().unwrap();
  Point3::new(p.0, p.1, p.2)
}

fn vertices_from_js(points: Array) -> Vec<Point3> {
  points.iter().map(point_from_js).collect()
}


#[wasm_bindgen]
pub struct JsCurve {
  real: Rc<RefCell<CurveType>>,
}

#[wasm_bindgen]
impl JsCurve {
  fn from(elem: &Rc<RefCell<CurveType>>) -> Self {
    Self {
      real: elem.clone()
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&as_controllable(&mut self.real.borrow()).id()).unwrap()
  }

  pub fn typename(&self) -> String {
    match *self.real.borrow() {
      CurveType::Line(_) => "Line",
      CurveType::Arc(_) => "Arc",
      CurveType::Circle(_) => "Circle",
      CurveType::BezierSpline(_) => "BezierSpline",
    }.to_string()
  }

  pub fn get_radius(&self) -> f64 {
    match &*self.real.borrow() {
      CurveType::Circle(c) => c.radius,
      _ => 0.0
    }
  }

  pub fn get_area(&self) -> f64 {
    let area = match &*self.real.borrow() {
      CurveType::Circle(c) => c.area(),
      _ => 0.0
    };
   area
  }

  pub fn get_handles(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).get_handles())
  }

  pub fn set_handles(&self, handles: Array) {
    let points = vertices_from_js(handles);
    as_controllable_mut(&mut self.real.borrow_mut()).set_handles(points);
  }

  pub fn set_initial_handles(&self, handles: Array) {
    let mut real = self.real.borrow_mut();
    let points = vertices_from_js(handles);
    as_controllable_mut(&mut real).set_initial_handles(points);
  }

  pub fn get_snap_points(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).get_snap_points())
  }

  pub fn tesselate_adaptive(&self, steps: i32) -> Array {
    points_to_js(self.real.borrow().as_curve().tesselate_adaptive(steps.into()))
  }

  pub fn tesselate(&self) -> Array {
    points_to_js(self.real.borrow().as_curve().tesselate())
  }

  pub fn get_length(&self) -> f64 {
    self.real.borrow().as_curve().length()
  }
}


#[wasm_bindgen]
#[derive(Default)]
pub struct JsSketch {
  real: Ref<Component>,
}

#[wasm_bindgen]
impl JsSketch {
  fn from(comp: &Ref<Component>) -> Self {
    JsSketch {
      real: comp.clone(),
    }
  }

  pub fn get_sketch_elements(&self) -> Array {
    self.real.borrow().sketch.elements.iter().map(|elem| {
      JsValue::from(JsCurve::from(elem))
    }).collect()
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsCurve {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = Line::new(Point3::from(p1), Point3::from(p2));
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(line.into_enum())));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsCurve {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = BezierSpline::new(points);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(CurveType::BezierSpline(spline))));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: f64) -> JsCurve {
    let center: (f64, f64, f64) = center.into_serde().unwrap();
    let circle = Circle::new(Point3::from(center), radius);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(rc(CurveType::Circle(circle)));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_arc(&mut self, p1: JsValue, p2: JsValue, p3: JsValue) -> JsCurve {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let p3: (f64, f64, f64) = p3.into_serde().unwrap();
    let arc = Arc::from_points(Point3::from(p1), Point3::from(p2), Point3::from(p3)).unwrap();
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(rc(arc.into_enum()));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn remove_element(&mut self, id: JsValue) {
    let id: uuid::Uuid = id.into_serde().unwrap();
    let mut real = self.real.borrow_mut();
    real.sketch.elements.retain(|elem| as_controllable(&mut elem.borrow_mut()).id() != id );
  }

  pub fn get_regions(&self) -> Array {
    self.real.borrow().sketch.get_regions(false).into_iter()
    .map(|region| JsValue::from(JsRegion {
      region,
      component: self.real.clone(),
    }))
    .collect()
  }

  pub fn get_all_split(&self) {
    let mut real = self.real.borrow_mut();
    // let splits = real.sketch.split_all();
    let mut splits = Sketch::all_split(&real.sketch.elements);
    Sketch::remove_dangling_segments(&mut splits);
    let islands = Sketch::build_islands(&splits);
    let islands: Vec<TrimmedCurve> = islands.into_iter().flatten().collect();

    real.sketch.elements.clear();
    for split in islands.iter() {
      real.sketch.elements.push(Rc::new(RefCell::new(split.cache.clone())));
    }
  }

  pub fn get_trimmed(&self, elem: JsCurve, _p: JsValue) -> Array {
    let splits = Sketch::split_element(&elem.real.borrow(), &self.real.borrow().sketch.elements);
    splits.into_iter().map(|split| {
      JsValue::from(JsCurve::from(&rc(split)))
    }).collect()
  }
}


#[wasm_bindgen]
pub struct JsBufferGeometry {
  position: Vec<f64>,
  normal: Vec<f64>,
}

#[wasm_bindgen]
impl JsBufferGeometry {
  fn from(buffer_geometry: (Vec<f64>, Vec<f64>)) -> Self {
    Self {
      position: buffer_geometry.0,
      normal: buffer_geometry.1,
    }
  }

  fn from_solid(solid: &Solid) -> Self {
    web_sys::console::time_with_label("Tesselation");
    let mesh = solid.tesselate();
    web_sys::console::time_end_with_label("Tesselation");
    Self::from(mesh.to_buffer_geometry())
  }

  pub fn position(&self) -> JsValue {
    JsValue::from_serde(&self.position).unwrap()
  }

  pub fn normal(&self) -> JsValue {
    JsValue::from_serde(&self.normal).unwrap()
  }
}


#[wasm_bindgen]
pub struct JsRegion {
  region: Vec<TrimmedCurve>,
  component: Rc<RefCell<Component>>,
}

#[wasm_bindgen]
impl JsRegion {
  pub fn get_mesh(&mut self) -> JsBufferGeometry {
    let poly = geom2d::poly_from_wire(&self.region);
    JsBufferGeometry::from(
      geom2d::tesselate_polygon(poly, Vec3::unit_z()).to_buffer_geometry()
    )
  }

  pub fn get_center(&self) -> JsValue {
    let center = self.region.iter().fold(
      Point3::new(0.0, 0.0, 0.0),
      |acc, elem| acc + match &elem.base {
        CurveType::Circle(circle) => circle.center.to_vec() * 2.0,
        _ => elem.bounds.0.to_vec() + elem.bounds.1.to_vec(),
      }
    ) / (self.region.len() as f64 * 2.0);
    point_to_js(center)
  }

  pub fn extrude(&self, distance: f64) {
    web_sys::console::time_with_label("BREP extrude");
    let tool = features::extrude(self.region.clone(), distance).unwrap();
    Solid::boolean_all(tool, &mut self.component.borrow_mut().bodies, BooleanType::Add);
    web_sys::console::time_end_with_label("BREP extrude");
  }

  pub fn extrude_preview(&self, distance: f64) -> JsValue {
    let extrusion = features::extrude(self.region.clone(), distance);
    match extrusion {
      Ok(res) => JsValue::from(JsBufferGeometry::from_solid(&res)),
      Err(error) => JsValue::from(error),
    }
  }
}


#[wasm_bindgen]
pub struct JsFace {
  real: Ref<Face>,
  solid_id: Uuid,
}

#[wasm_bindgen]
impl JsFace {
  fn from(face: &Ref<Face>, solid_id: Uuid) -> Self {
    Self {
      real: face.clone(),
      solid_id,
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_solid_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn get_origin(&self) -> JsValue {
    point_to_js(self.make_origin())
  }

  pub fn get_normal(&self) -> JsValue {
    point_to_js(Point3::from_vec(self.real.borrow().surface.as_surface().normal_at(0.0, 0.0)))
  }

  pub fn get_display_normal(&self) -> Array {
    let normal = self.real.borrow().surface.as_surface().normal_at(0.0, 0.0);
    let origin = self.make_origin();
    points_to_js(vec![origin, origin + normal])
  }

  fn make_origin(&self) -> Point3 {
    match &self.real.borrow().surface {
      SurfaceType::Planar(plane) => plane.origin,
      SurfaceType::Cylindrical(cyl) => cyl.origin,
    }
  }

  pub fn get_surface_type(&self) -> String {
    match self.real.borrow().surface {
      SurfaceType::Planar(_) => "Planar".to_string(),
      SurfaceType::Cylindrical(_) => "Cylindrical".to_string(),
    }
  }

  pub fn tesselate(&self) -> JsBufferGeometry {
    let this = self.real.borrow();
    JsBufferGeometry::from(
      this.get_surface().tesselate().to_buffer_geometry()
    )
  }
}


#[wasm_bindgen]
pub struct JsEdge {
  real: Ref<Edge>,
  solid_id: Uuid,
}

#[wasm_bindgen]
impl JsEdge {
  fn from(edge: &Ref<Edge>, solid_id: Uuid) -> Self {
    Self {
      real: edge.clone(),
      solid_id,
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_solid_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn tesselate(&self) -> Array {
    points_to_js(self.real.borrow().curve.as_curve().tesselate())
  }
}


#[wasm_bindgen]
pub struct JsSolid {
  comp: Ref<Component>,
  solid_id: Uuid,
  faces: Array,
  edges: Array,
  vertices: Array,
  pub area: f64,
  pub volume: f64,
}

#[wasm_bindgen]
impl JsSolid {
  fn from(solid: &Solid, comp: Ref<Component>) -> Self {
    let shell = &solid.shells[0];
    // Vertices
    let vertices = points_to_js(shell.vertices.iter().map(|v| v.borrow().point ).collect());
    // Edges
    let edges = shell.edges.iter().filter_map(|edge| {
      if edge.borrow().is_inner() {
        None
      } else {
        Some(JsValue::from(JsEdge::from(edge, solid.id)))
      }
    }).collect();
    // Faces
    let faces = shell.faces.iter().map(|f| {
      JsValue::from(JsFace::from(f, solid.id))
    }).collect();
    Self {
      comp,
      solid_id: solid.id,
      vertices,
      edges,
      faces,
      area: solid.area(),
      volume: solid.volume(),
    }
  }

  pub fn typename(&self) -> String {
    "Solid".to_string()
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn get_faces(&self) -> Array {
    self.faces.clone()
  }

  pub fn get_edges(&self) -> Array {
    self.edges.clone()
  }

  pub fn get_vertices(&self) -> Array {
    self.vertices.clone()
  }

  pub fn remove(&self) {
    self.comp.borrow_mut().bodies.retain(|body| body.id != self.solid_id )
  }
}


#[wasm_bindgen]
#[derive(Default)]
pub struct JsComponent {
  real: Ref<Component>,
}

#[wasm_bindgen]
impl JsComponent {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let tree = Component::new();
    Self { real: rc(tree) }
  }

  fn from(comp: &Rc<RefCell<Component>>) -> Self {
    Self {
      real: comp.clone(),
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_sketch(&self) -> JsSketch {
    JsSketch::from(&self.real)
  }

  // pub fn get_children(&self) -> Array {
  //   self.real.borrow().children.iter().map(|child|
  //     JsValue::from(Self::from(child))
  //   ).collect()
  // }

  pub fn get_solids(&self) -> Array {
    self.real.borrow().bodies.iter().map(|body|
      JsValue::from(JsSolid::from(body, self.real.clone()))
    ).collect()
  }

  pub fn create_component(&mut self) -> Self {
    let comp = self.real.borrow_mut().create_component();
    Self::from(&comp)
  }

  pub fn delete_component(&self, comp: Self) {
    self.real.borrow_mut().delete_component(&comp.real)
  }

  pub fn export_stl(&self, title: &str) -> String {
    let comp = self.real.borrow();
    let mesh = comp.bodies[0].tesselate();
    log!("{:?}", mesh);
    export::stl(&mesh, title)
  }

  pub fn export_3mf(&self) -> String {
    let meshes = Self::tesselate_all(&self.real);
    export::threemf(&meshes, "millimeter")
  }

  fn tesselate_all(comp: &Ref<Component>) -> Vec<Mesh> {
    let comp = comp.borrow();
    let mut meshes: Vec<Mesh> = comp.bodies.iter().map(|body| body.tesselate() ).collect();
    for child in &comp.children {
      meshes.append(&mut Self::tesselate_all(&child));
    }
    meshes
  }
}
