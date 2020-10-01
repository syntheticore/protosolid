use std::rc::Rc;
use std::cell::RefCell;

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


fn vertices_to_js(points: Vec<Point3>) -> Array {
  points.iter().map(|p|
    JsValue::from_serde(&(p.x, p.y, p.z)).unwrap()
  ).collect()
}

fn vertices_from_js(points: Array) -> Vec<Point3> {
  points.iter().map(|vertex| {
    let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
    Point3::new(vertex.0, vertex.1, vertex.2)
  }).collect()
}


#[wasm_bindgen]
pub struct JsSegment {
  real: Rc<RefCell<SketchElement>>,
}

#[wasm_bindgen]
impl JsSegment {
  fn from(elem: &Rc<RefCell<SketchElement>>) -> Self {
    Self {
      real: elem.clone()
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&as_controllable(&mut self.real.borrow_mut()).id()).unwrap()
  }

  pub fn get_handles(&self) -> Array {
    vertices_to_js(as_controllable(&mut self.real.borrow_mut()).get_handles())
  }

  pub fn set_handles(&self, handles: Array) {
    let points = vertices_from_js(handles);
    as_controllable_mut(&mut self.real.borrow_mut()).set_handles(points);
  }

  pub fn get_snap_points(&self) -> Array {
    vertices_to_js(as_controllable(&mut self.real.borrow_mut()).get_snap_points())
  }

  pub fn tesselate(&self, steps: i32) -> Array {
    vertices_to_js(self.real.borrow_mut().as_curve().tesselate(steps))
  }

  pub fn default_tesselation(&self) -> Array {
    vertices_to_js(self.real.borrow_mut().as_curve().default_tesselation())
  }
}


// #[wasm_bindgen]
// pub struct JsSketch {
//   real: Rc<RefCell<Sketch>>,
// }

// #[wasm_bindgen]
// impl JsSketch {
//   fn from(sketch: &Rc<RefCell<Sketch>>) -> Self {
//     JsSketch {
//       real: sketch.clone(),
//     }
//   }

//   pub fn get_segments(&self) -> Array {
//     self.real.borrow().elements.iter().map(|elem| JsValue::from(JsSegment::from(elem)) ).collect()
//   }

//   pub fn add_segment(&self) {
//     let spline = BezierSpline::new(vec![
//         Point3::new(0.0, 0.0, 1.0),
//         Point3::new(1.0, 0.0, 1.25),
//         Point3::new(1.0, 1.0, 1.5),
//         Point3::new(0.0, 1.0, 1.75),
//         Point3::new(0.0, 0.0, 2.0),
//         Point3::new(1.0, 0.0, 2.25),
//         Point3::new(1.0, 1.0, 2.5),
//         Point3::new(0.0, 1.0, 2.75),
//       ]
//     );
//     self.real.borrow_mut().elements.push(Rc::new(RefCell::new(spline)));
//   }
// }

// fn foo(elem: &mut SketchElement) -> &mut dyn SketchElement {
//   match elem {
//     SketchElement::Line(line) => line as &mut dyn SketchElement,
//     SketchElement::Circle(circle) => circle as &mut dyn SketchElement,
//     SketchElement::BezierSpline(spline) => spline as &mut dyn SketchElement,
//   }
// }


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
      real: comp.clone(),
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_title(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().title).unwrap()
  }

  // pub fn get_sketches(&self) -> Array {
  //   self.real.borrow().sketches.iter().map(|sketch| JsValue::from(JsSketch::from(sketch)) ).collect()
  // }

  pub fn get_sketch_elements(&self) -> Array {
    self.real.borrow().sketch_elements.iter().map(|elem| {
      JsValue::from(JsSegment::from(elem))
    }).collect()
  }

  pub fn get_children(&self) -> Array {
    self.real.borrow().children.iter().map(|child| JsValue::from(JsComponent::from(child)) ).collect()
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
    self.real.borrow_mut().sketch_elements.push(Rc::new(RefCell::new(SketchElement::BezierSpline(spline))));
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsSegment {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = Line::new(Point3::from(p1), Point3::from(p2));
    let mut real = self.real.borrow_mut();
    real.sketch_elements.push(Rc::new(RefCell::new(SketchElement::Line(line))));
    JsSegment::from(&real.sketch_elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsSegment {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = BezierSpline::new(points);
    let mut real = self.real.borrow_mut();
    real.sketch_elements.push(Rc::new(RefCell::new(SketchElement::BezierSpline(spline))));
    JsSegment::from(&real.sketch_elements.last().unwrap())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: JsValue) -> JsSegment {
    let center: (f64, f64, f64) = center.into_serde().unwrap();
    let radius: f64 = radius.into_serde().unwrap();
    let circle = Circle::new(Point3::from(center), radius);
    let mut real = self.real.borrow_mut();
    real.sketch_elements.push(Rc::new(RefCell::new(SketchElement::Circle(circle))));
    JsSegment::from(&real.sketch_elements.last().unwrap())
  }

  pub fn remove_element(&mut self, id: JsValue) {
    let id: uuid::Uuid = id.into_serde().unwrap();
    let mut real = self.real.borrow_mut();
    real.sketch_elements.retain(|elem| as_controllable(&mut elem.borrow_mut()).id() != id);
  }

  pub fn get_regions(&self) -> Array {
    self.real.borrow().closed_regions().iter().map(|region| vertices_to_js(region.clone()) ).collect()
  }

  pub fn get_all_split(&self) -> Array {
    self.real.borrow().all_split().iter().map(|elem| {
      JsValue::from(JsSegment::from(&Rc::new(RefCell::new(elem.clone()))))
    }).collect()
  }

  // pub fn create_sketch(&mut self) -> JsSketch {
  //   let mut sketch = Sketch::new();
  //   sketch.title = "Sketch1".to_string();
  //   sketch.visible = true;
  //   let sketch = Rc::new(RefCell::new(sketch));
  //   self.real.borrow_mut().sketches.push(sketch.clone());
  //   JsSketch::from(&sketch)
  // }

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

  pub fn get_main_assembly(&mut self) -> JsComponent {
    JsComponent::from(&self.scene.tree)
  }
}
