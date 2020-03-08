use wasm_bindgen::prelude::*;
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

#[wasm_bindgen]
pub struct AlchemyProxy {
  scene: Scene
}

#[wasm_bindgen]
impl AlchemyProxy {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Self {
      scene: Scene::new()
    }
  }
}
