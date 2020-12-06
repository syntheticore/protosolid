use std::rc::Rc;
use std::cell::RefCell;
// use std::collections::HashSet;

use wasm_bindgen::prelude::*;
use js_sys::Array;

use alchemy::*;

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

// #[wasm_bindgen]
// #[allow(non_snake_case)]
// pub fn getAlchemy() -> AlchemyProxy {
//   AlchemyProxy::new()
// }

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
  #[cfg(debug_assertions)]
  console_error_panic_hook::set_once();

  let document = web_sys::window().unwrap().document().unwrap();
  let _body = document.body().expect("Document should have a body");

  // let val = document.create_element("p")?;
  // val.set_inner_html("Hello from Rust!");
  // body.append_child(&val)?;

  // setup_canvas(&document);

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
pub struct JsSketchElement {
  real: Rc<RefCell<SketchElement>>,
}

#[wasm_bindgen]
impl JsSketchElement {
  fn from(elem: &Rc<RefCell<SketchElement>>) -> Self {
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
        SketchElement::Line(_) => "Line",
        SketchElement::Arc(_) => "Arc",
        SketchElement::Circle(_) => "Circle",
        SketchElement::BezierSpline(_) => "BezierSpline",
      }
    ).unwrap()
  }

  pub fn get_radius(&self) -> JsValue {
    JsValue::from_serde(
      match &*self.real.borrow() {
        SketchElement::Circle(c) => &c.radius,
        _ => &0.0
      }
    ).unwrap()
  }

  pub fn get_area(&self) -> JsValue {
    let area = match &*self.real.borrow() {
      SketchElement::Circle(c) => c.area(),
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

  pub fn get_snap_points(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).get_snap_points())
  }

  pub fn tesselate(&self, steps: i32) -> Array {
    points_to_js(self.real.borrow().as_curve().tesselate_relative(steps.into()))
  }

  pub fn default_tesselation(&self) -> Array {
    points_to_js(self.real.borrow().as_curve().default_tesselation())
  }

  pub fn get_length(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().as_curve().length()).unwrap()
  }
}


#[wasm_bindgen]
pub struct JsBufferGeometry {
  position: Vec<f64>,
  normal: Vec<f64>,
}

#[wasm_bindgen]
impl JsBufferGeometry {
  pub fn position(&self) -> JsValue {
    JsValue::from_serde(&self.position).unwrap()
  }

  pub fn normal(&self) -> JsValue {
    JsValue::from_serde(&self.normal).unwrap()
  }
}


// #[wasm_bindgen]
// pub struct JsSolid {
//   // faces: Array,
//   edges: Array,
//   vertices: Array,
//   half_edges: Array,
// }

// #[wasm_bindgen]
// impl JsSolid {
//   fn from(solid: &Solid) -> Self {
//     let shell = solid.shells[0];
//     let vertices = points_to_js(shell.vertices.iter().map(|v| v.borrow().point ).collect());
//     let edges = shell.edges.iter().map(|e| {
//       let left = e.borrow().left_half.borrow().origin.borrow().point;
//       let right = e.borrow().right_half.borrow().origin.borrow().point;
//       points_to_js(vec![
//         left,
//         right
//       ])
//     });
//     // let faces = shell.faces.map(|f| {

//     // });
//     Self {
//       edges,
//       vertices,
//     }
//   }

//   pub fn position(&self) -> JsValue {
//     JsValue::from_serde(&self.position).unwrap()
//   }

//   pub fn normal(&self) -> JsValue {
//     JsValue::from_serde(&self.normal).unwrap()
//   }
// }


#[wasm_bindgen]
pub struct JsRegion {
  region: Vec<TrimmedSketchElement>,
  component: Rc<RefCell<Component>>,
}

#[wasm_bindgen]
impl JsRegion {
  pub fn get_mesh(&self) -> JsValue {
    let poly = geom2d::poly_from_wire(&self.region.iter().map(|elem| elem.cache.clone() ).collect());
    JsValue::from(JsBufferGeometry {
      position: geom2d::tesselate_polygon(poly).to_buffer_geometry(),
      normal: vec![],
    })
  }

  pub fn extrude(&self, distance: f64) {
    let tool = features::extrude(self.region.clone(), distance);
    Solid::boolean_all(tool, &mut self.component.borrow_mut().bodies, BooleanType::Add);
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
      JsValue::from(JsSketchElement::from(elem))
    }).collect()
  }

  pub fn add_segment(&self) {
    let spline = BezierSpline::new(vec![
      Point3::new(0.0, 0.0, 1.0),
      Point3::new(1.0, 0.0, 1.25),
      Point3::new(1.0, 1.0, 1.5),
      Point3::new(0.0, 1.0, 1.75),
      Point3::new(0.0, 0.0, 2.0),
      Point3::new(1.0, 0.0, 2.25),
      Point3::new(1.0, 1.0, 2.5),
      Point3::new(0.0, 1.0, 2.75),
    ]);
    self.real.borrow_mut().sketch.elements.push(Rc::new(RefCell::new(SketchElement::BezierSpline(spline))));
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsSketchElement {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = Line::new(Point3::from(p1), Point3::from(p2));
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(line.into_enum())));
    JsSketchElement::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsSketchElement {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = BezierSpline::new(points);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(SketchElement::BezierSpline(spline))));
    JsSketchElement::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: f64) -> JsSketchElement {
    let center: (f64, f64, f64) = center.into_serde().unwrap();
    let circle = Circle::new(Point3::from(center), radius);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(SketchElement::Circle(circle))));
    JsSketchElement::from(&real.sketch.elements.last().unwrap())
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
    let islands: Vec<TrimmedSketchElement> = islands.into_iter().flatten().collect();

    real.sketch.elements.clear();
    for split in islands.iter() {
      real.sketch.elements.push(Rc::new(RefCell::new(split.cache.clone())));
    }
  }

  pub fn get_trimmed(&self, elem: JsSketchElement, _p: JsValue) -> Array {
    let splits = Sketch::split_element(&elem.real.borrow(), &self.real.borrow().sketch.elements);
    splits.into_iter().map(|split| {
      JsValue::from(JsSketchElement::from(&rc(split)))
    }).collect()
  }
}


fn js_mesh_from_body(body: &Solid) -> JsValue {
  JsValue::from(JsBufferGeometry {
    position: body.tesselate().to_buffer_geometry(),
    normal: vec![],
  })
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
    self.real.borrow().children.iter().map(|child| JsValue::from(JsComponent::from(child)) ).collect()
  }

  pub fn get_mesh(&self) -> JsValue {
    let bodies = &self.real.borrow().bodies;
    if bodies.len() > 0 {
      js_mesh_from_body(&bodies[0])
    } else { JsValue::UNDEFINED }
  }

  pub fn get_wireframe(&self) -> Array {
    let bodies = &self.real.borrow().bodies;
    if bodies.len() == 0 {
      return Array::new_with_length(0);
    }
    bodies[0].shells[0].edges.iter().map(|edge| {
      let edge = edge.borrow();
      let left = edge.left_half.borrow().origin.borrow().point;
      let right = edge.right_half.borrow().origin.borrow().point;
      points_to_js(vec![
        left,
        right
      ])
    }).collect()
  }

  pub fn add_segment(&self) {
    let spline = BezierSpline::new(vec![
      Point3::new(0.0, 0.0, 1.0),
      Point3::new(1.0, 0.0, 1.25),
      Point3::new(1.0, 1.0, 1.5),
      Point3::new(0.0, 1.0, 1.75),
      Point3::new(0.0, 0.0, 2.0),
      Point3::new(1.0, 0.0, 2.25),
      Point3::new(1.0, 1.0, 2.5),
      Point3::new(0.0, 1.0, 2.75),
    ]);
    self.real.borrow_mut().sketch.elements.push(Rc::new(RefCell::new(SketchElement::BezierSpline(spline))));
  }

  pub fn create_component(&mut self, title: &str) -> JsComponent {
    let comp = self.real.borrow_mut().create_component();
    comp.borrow_mut().title = title.to_string();
    JsComponent::from(&comp)
  }
}


#[wasm_bindgen]
#[derive(Default)]
pub struct JsFoo {
  pub cut: usize,
  pub islands: usize,
  pub regions: usize,
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

  // pub fn foo(&self) -> Result<f64, JsValue> {
  //   Ok(44.0)
  // }

  pub fn get_main_assembly(&mut self) -> JsComponent {
    JsComponent::from(&self.tree)
  }
}
