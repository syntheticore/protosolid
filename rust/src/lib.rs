use std::rc::Rc;
use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use js_sys::Array;
// use serde::{Serialize};

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


// #[derive(Serialize)]
// pub struct JsSegment {
//   handles: Vec<(f64, f64, f64)>,
//   vertices: Vec<(f64, f64, f64)>,

//   #[serde(skip_serializing)]
//   real: Rc<RefCell<dyn SketchElement>>,
// }

fn vertices_to_js(points: Vec<shapex::Point3>) -> Array {
  points.iter().map(|p|
    JsValue::from_serde(&(p.x, p.y, p.z)).unwrap()
  ).collect()
}

fn vertices_from_js(points: Array) -> Vec<shapex::Point3> {
  points.iter().map(|vertex| {
    let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
    shapex::Point3::new(vertex.0, vertex.1, vertex.2)
  }).collect()
}


#[wasm_bindgen]
pub struct JsSegment {
  real: Rc<RefCell<dyn SketchElement>>,
}

#[wasm_bindgen]
impl JsSegment {
  fn from(elem: &Rc<RefCell<dyn SketchElement>>) -> Self {
    Self {
      real: elem.clone()
    }
  }

  pub fn get_handles(&self) -> Array {
    vertices_to_js(self.real.borrow().get_handles())
  }

  pub fn set_handles(&self, handles: Array) {
    let points = vertices_from_js(handles);
    self.real.borrow_mut().set_handles(points);
  }

  pub fn get_snap_points(&self) -> Array {
    vertices_to_js(self.real.borrow().get_snap_points())
  }

  pub fn tesselate(&self, steps: i32) -> Array {
    vertices_to_js(self.real.borrow().tesselate(steps))
  }

  pub fn default_tesselation(&self) -> Array {
    vertices_to_js(self.real.borrow().default_tesselation())
  }
}


#[wasm_bindgen]
pub struct JsSketch {
  real: Rc<RefCell<Sketch>>,
}

#[wasm_bindgen]
impl JsSketch {
  fn from(sketch: &Rc<RefCell<Sketch>>) -> Self {
    JsSketch {
      real: sketch.clone(),
    }
  }

  pub fn get_segments(&self) -> Array {
    self.real.borrow().elements.iter().map(|elem| JsValue::from(JsSegment::from(elem)) ).collect()
  }

  pub fn add_segment(&self) {
    let spline = shapex::BezierSpline::new(vec![
        shapex::Point3::new(0.0, 0.0, 1.0),
        shapex::Point3::new(1.0, 0.0, 1.25),
        shapex::Point3::new(1.0, 1.0, 1.5),
        shapex::Point3::new(0.0, 1.0, 1.75),
        shapex::Point3::new(0.0, 0.0, 2.0),
        shapex::Point3::new(1.0, 0.0, 2.25),
        shapex::Point3::new(1.0, 1.0, 2.5),
        shapex::Point3::new(0.0, 1.0, 2.75),
      ]
    );
    self.real.borrow_mut().elements.push(Rc::new(RefCell::new(spline)));
  }
}


#[wasm_bindgen]
#[derive(Default)]
pub struct JsComponent {
  // title: String,
  real: Rc<RefCell<Component>>,
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
      real: comp.clone()
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_title(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().title).unwrap()
  }

  pub fn get_sketches(&self) -> Array {
    self.real.borrow().sketches.iter().map(|sketch| JsValue::from(JsSketch::from(sketch)) ).collect()
  }

  pub fn get_sketch_elements(&self) -> Array {
    self.real.borrow().sketch_elements.iter().map(|elem| JsValue::from(JsSegment::from(elem)) ).collect()
  }

  pub fn get_children(&self) -> Array {
    self.real.borrow().children.iter().map(|child| JsValue::from(JsComponent::from(child)) ).collect()
  }

  pub fn add_segment(&self) {
    let spline = shapex::BezierSpline::new(vec![
      shapex::Point3::new(0.0, 0.0, 1.0),
      shapex::Point3::new(1.0, 0.0, 1.25),
      shapex::Point3::new(1.0, 1.0, 1.5),
      shapex::Point3::new(0.0, 1.0, 1.75),
      shapex::Point3::new(0.0, 0.0, 2.0),
      shapex::Point3::new(1.0, 0.0, 2.25),
      shapex::Point3::new(1.0, 1.0, 2.5),
      shapex::Point3::new(0.0, 1.0, 2.75),
    ]);
    self.real.borrow_mut().sketch_elements.push(Rc::new(RefCell::new(spline)));
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsSegment {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = shapex::Line {
      points: (shapex::Point3::from(p1), shapex::Point3::from(p2))
    };
    let mut real = self.real.borrow_mut();
    real.sketch_elements.push(Rc::new(RefCell::new(line)));
    JsSegment::from(&real.sketch_elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsSegment {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      shapex::Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = shapex::BezierSpline::new(points);
    let mut real = self.real.borrow_mut();
    real.sketch_elements.push(Rc::new(RefCell::new(spline)));
    JsSegment::from(&real.sketch_elements.last().unwrap())
  }

  pub fn remove_element(&mut self, index: usize) {
    let mut real = self.real.borrow_mut();
    real.sketch_elements.remove(index);
  }

  pub fn create_sketch(&mut self) -> JsSketch {
    let mut sketch = Sketch::new();
    sketch.title = "Sketch1".to_string();
    sketch.visible = true;
    let sketch = Rc::new(RefCell::new(sketch));
    self.real.borrow_mut().sketches.push(sketch.clone());
    JsSketch::from(&sketch)
  }

  pub fn create_component(&mut self, title: &str) -> JsComponent {
    let mut comp = Component::new();
    comp.title = title.to_string();
    comp.visible = true;
    let comp = Rc::new(RefCell::new(comp));
    self.real.borrow_mut().children.push(comp.clone());
    JsComponent::from(&comp)
  }
}


#[wasm_bindgen]
pub struct AlchemyProxy {
  scene: Scene,
}

#[wasm_bindgen]
impl AlchemyProxy {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let scene = Scene::new();
    Self { scene }
  }

  // pub fn foo(&self) -> Result<f64, JsValue> {
  //   Ok(44.0)
  // }

  // pub fn create_component(&mut self) -> JsComponent {
  //   let comp = self.scene.create_component();
  //   JsComponent::from(&comp)
  // }

  pub fn get_main_assembly(&mut self) -> JsComponent {
    JsComponent::from(&self.scene.tree)
  }
}
