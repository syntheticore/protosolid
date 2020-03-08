use std::rc::Rc;
use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use serde::{Serialize};
use uuid::Uuid;
use alchemy::*;

#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// macro_rules! log {
//   ( $( $t:tt )* ) => {
//     web_sys::console::log_1(&format!( $( $t )* ).into());
//   }
// }

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
  // log!("Hello from macro");

  // setup_canvas(&document);

  Ok(())
}

#[derive(Serialize)]
pub struct Example {
  pub id: Uuid,
  pub name: String,
  pub children: Vec<Example>,
}

#[wasm_bindgen]
pub struct AlchemyProxy {
  scene: Scene
}

#[wasm_bindgen]
impl AlchemyProxy {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let mut scene = Scene::new();
    scene.create_component();
    scene.create_component();
    scene.create_component();
    Self { scene }
  }

  pub fn foo(&self) -> Result<f64, JsValue> {
    Ok(44.0)
  }

  pub fn bar(&self) -> f64 {
    90.0
  }

  pub fn get_tree(&mut self) -> JsValue {
    let tree = self.build_tree_node(&self.scene.tree);
    JsValue::from_serde(&tree).unwrap()
  }

  fn build_tree_node(&self, node: &TreeNode<Rc<RefCell<Component>>>) -> Example {
    let item = node.item.borrow();
    let mut out = Example {
      id: item.id,
      name: String::from(&item.name),
      children: vec![],
    };
    for child in &node.children {
      out.children.push(self.build_tree_node(child));
    }
    out
  }
}
