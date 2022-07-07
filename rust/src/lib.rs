use wasm_bindgen::prelude::*;

mod document;
mod component;
mod feature;
mod solid;
mod sketch;
mod curve;
mod region;
mod utils;
mod buffer_geometry;
mod controllable;
mod construction_helper;


#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;


#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
  #[cfg(debug_assertions)]
  console_error_panic_hook::set_once();

  log!("Alchemy running");

  Ok(())
}
