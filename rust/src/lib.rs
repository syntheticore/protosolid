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
    self.real.borrow().get_handles().iter().map(|vertex|
      JsValue::from_serde(&[vertex.x, vertex.y, vertex.z]).unwrap()
    ).collect()
  }

  pub fn tesselate(&self, steps: i32) -> Array {
    self.real.borrow().tesselate(steps).iter().map(|vertex|
      JsValue::from_serde(&[vertex.x, vertex.y, vertex.z]).unwrap()
    ).collect()
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

  pub fn get_children(&self) -> Array {
    self.real.borrow().children.iter().map(|child| JsValue::from(JsComponent::from(child)) ).collect()
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
