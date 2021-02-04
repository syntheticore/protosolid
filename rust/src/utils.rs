use js_sys::Array;
use wasm_bindgen::prelude::*;

use shapex::*;


#[macro_export] macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}

#[wasm_bindgen]
extern {
  fn alert(s: &str);
}

pub fn point_to_js(p: Point3) -> JsValue {
  JsValue::from_serde(&(p.x, p.y, p.z)).unwrap()
  // JsValue::from_serde(&p).unwrap()
}

pub fn points_to_js(points: Vec<Point3>) -> Array {
  points.into_iter().map(point_to_js).collect()
}

pub fn point_from_js(p: JsValue) -> Point3 {
  let p: (f64, f64, f64) = p.into_serde().unwrap();
  Point3::new(p.0, p.1, p.2)
}

pub fn vertices_from_js(points: Array) -> Vec<Point3> {
  points.iter().map(point_from_js).collect()
}
