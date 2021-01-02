use std::rc::Rc;
use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use js_sys::Array;

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

  pub fn typename(&self) -> JsValue {
    JsValue::from_serde(
      match *self.real.borrow() {
        CurveType::Line(_) => "Line",
        CurveType::Arc(_) => "Arc",
        CurveType::Circle(_) => "Circle",
        CurveType::BezierSpline(_) => "BezierSpline",
      }
    ).unwrap()
  }

  pub fn get_radius(&self) -> JsValue {
    JsValue::from_serde(
      match &*self.real.borrow() {
        CurveType::Circle(c) => &c.radius,
        _ => &0.0
      }
    ).unwrap()
  }

  pub fn get_area(&self) -> JsValue {
    let area = match &*self.real.borrow() {
      CurveType::Circle(c) => c.area(),
      _ => 0.0
    };
   JsValue::from_serde(&area).unwrap()
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

  pub fn get_length(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().as_curve().length()).unwrap()
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
  pub fn from(buffer_geometry: Vec<f64>) -> Self {
    Self {
      position: buffer_geometry,
      normal: vec![],
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
  pub fn get_mesh(&mut self) -> JsValue {
    let poly = geom2d::poly_from_wire(&self.region);
    JsValue::from(JsBufferGeometry::from(
      geom2d::tesselate_polygon(poly, Vec3::unit_z()).to_buffer_geometry()
    ))
  }

  pub fn get_center(&self) -> JsValue {
    let center = self.region.iter().fold(
      Point3::new(0.0, 0.0, 0.0),
      |acc, elem| acc + elem.bounds.0.to_vec() + elem.bounds.1.to_vec()
    ) / (self.region.len() as f64 * 2.0);
    point_to_js(center)
  }

  pub fn extrude(&self, distance: f64) {
    web_sys::console::time_with_label("BREP extrude");
    let tool = features::extrude(self.region.clone(), distance);
    Solid::boolean_all(tool, &mut self.component.borrow_mut().bodies, BooleanType::Add);
    web_sys::console::time_end_with_label("BREP extrude");
  }

  pub fn extrude_preview(&self, distance: f64) -> JsValue {
    JsValue::from(JsBufferGeometry::from_solid(&features::extrude(self.region.clone(), distance)))
  }
}


#[wasm_bindgen]
pub struct JsFace {
  real: Ref<Face>,
}

#[wasm_bindgen]
impl JsFace {
  fn from(face: &Ref<Face>) -> Self {
    Self {
      real: face.clone(),
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_origin(&self) -> JsValue {
    match &self.real.borrow().surface {
      SurfaceType::Planar(plane) => point_to_js(plane.origin),
      SurfaceType::Cylindrical(cyl) => point_to_js(cyl.origin),
    }
  }

  pub fn get_surface_type(&self) -> String {
    match self.real.borrow().surface {
      SurfaceType::Planar(_) => "Planar".to_string(),
      SurfaceType::Cylindrical(_) => "Cylindrical".to_string(),
    }
  }

  pub fn tesselate(&self) -> JsValue {
    let this = self.real.borrow();
    JsValue::from(JsBufferGeometry::from(
      this.tesselate().to_buffer_geometry()
    ))
  }
}


#[wasm_bindgen]
pub struct JsSolid {
  faces: Array,
  edges: Array,
  vertices: Array,
}

#[wasm_bindgen]
impl JsSolid {
  fn from(solid: &Solid) -> Self {
    let shell = &solid.shells[0];
    let vertices = points_to_js(shell.vertices.iter().map(|v| v.borrow().point ).collect());
    let edges = shell.edges.iter().map(|e| {
      // let left = e.borrow().left_half.borrow().origin.borrow().point;
      // let right = e.borrow().right_half.borrow().origin.borrow().point;
      // points_to_js(vec![
      //   left,
      //   right
      // ])
      points_to_js(e.borrow().curve.as_curve().tesselate())
    }).collect();
    let faces = shell.faces.iter().map(|f| {
      JsValue::from(JsFace::from(f))
    }).collect();
    Self {
      faces,
      edges,
      vertices,
    }
  }

  pub fn get_faces(&self) -> Array {
    self.faces.clone()
  }

  pub fn get_edges(&self) -> Array {
    self.edges.clone()
  }
}


#[wasm_bindgen]
#[derive(Default)]
pub struct JsComponent {
  // title: String,
  real: Ref<Component>,
}

#[wasm_bindgen]
impl JsComponent {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Default::default()
  }

  fn from(comp: &Rc<RefCell<Component>>) -> Self {
    JsComponent {
      // title: String::from(&comp.borrow().title),
      real: comp.clone(),
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_title(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().title).unwrap()
  }

  pub fn get_sketch(&self) -> JsSketch {
    JsSketch::from(&self.real)
  }

  pub fn get_children(&self) -> Array {
    self.real.borrow().children.iter().map(|child|
      JsValue::from(JsComponent::from(child))
    ).collect()
  }

  pub fn get_solids(&self) -> Array {
    self.real.borrow().bodies.iter().map(|body|
      JsValue::from(JsSolid::from(body))
    ).collect()
  }

  pub fn export_stl(&self) -> String {
    let comp = self.real.borrow();
    let mesh = comp.bodies[0].tesselate();
    log!("{:?}", mesh);
    export::stl(&mesh, &comp.title)
  }

  pub fn create_component(&mut self, title: &str) -> JsComponent {
    let comp = self.real.borrow_mut().create_component();
    comp.borrow_mut().title = title.to_string();
    JsComponent::from(&comp)
  }
}


#[wasm_bindgen]
pub struct AlchemyProxy {
  tree: Ref<Component>,
}

#[wasm_bindgen]
impl AlchemyProxy {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let mut tree = Component::new();
    tree.title = "Main Assembly".to_string();
    Self { tree: rc(tree) }
  }

  pub fn get_main_assembly(&mut self) -> JsComponent {
    JsComponent::from(&self.tree)
  }
}
