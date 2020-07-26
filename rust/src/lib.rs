use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use serde::{Serialize};
use uuid::Uuid;
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

#[derive(Serialize)]
pub struct JsTreeNode {
  id: Uuid,
  title: String,
  sketches: Vec<JsSketch>,
  children: Vec<JsTreeNode>,
}

#[derive(Serialize)]
pub struct JsSketch {
  title: String,
  segments: Vec<JsSegment>,
}

#[derive(Serialize)]
pub struct JsSegment {
  handles: Vec<(f64, f64, f64)>,
  vertices: Vec<(f64, f64, f64)>,
}

#[wasm_bindgen]
pub struct AlchemyProxy {
  scene: Scene,
  element_cache: HashMap<Uuid, Rc<RefCell<Component>>>,
}

#[wasm_bindgen]
impl AlchemyProxy {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let scene = Scene::new();
    let mut this = Self { scene, element_cache: Default::default() };
    this.create_component();
    this.create_component();
    this.create_sketch();
    this
  }

  pub fn foo(&self) -> Result<f64, JsValue> {
    Ok(44.0)
  }

  pub fn create_component(&mut self) {
    let comp = self.scene.create_component();
    let id = comp.borrow().id;
    self.element_cache.insert(id, comp);
  }

  pub fn create_sketch(&mut self) {
    let sketch = self.scene.create_sketch();
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
    sketch.borrow_mut().elements.push(Box::new(spline));
  }

  pub fn get_tree(&mut self) -> JsValue {
    let tree = self.build_tree(&self.scene.tree);
    JsValue::from_serde(&tree).unwrap()
  }

  pub fn get_element(&self, id: &str) -> JsValue {
    let id = Uuid::parse_str(id).unwrap();
    let comp = self.element_cache.get(&id).unwrap();
    JsValue::from_serde(&self.build_tree_node(comp)).unwrap()
  }

  fn build_tree(&self, comp: &Rc<RefCell<Component>>) -> JsTreeNode {
    let mut node = self.build_tree_node(&comp);
    for child in &comp.borrow().children {
      node.children.push(self.build_tree(child));
    }
    node
  }

  fn build_tree_node(&self, comp: &Rc<RefCell<Component>>) -> JsTreeNode {
    let comp = comp.borrow();
    let sketches = comp.sketches.iter().map(|sketch| {
      let sketch = sketch.borrow();
      JsSketch {
        title: String::from(&sketch.title),
        segments: sketch.elements.iter()
          .map(|elem| JsSegment { handles: elem.get_handles().iter().map(|p| (p.x, p.y, p.z) ).collect(), vertices: elem.tesselate(40).iter().map(|p| (p.x, p.y, p.z) ).collect() } )
          .collect(),
      }
    }).collect();
    JsTreeNode {
      id: comp.id,
      title: String::from(&comp.title),
      sketches: sketches,
      children: vec![],
    }
  }
}
