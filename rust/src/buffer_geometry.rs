use wasm_bindgen::prelude::*;

use shapex::*;


#[wasm_bindgen]
pub struct JsBufferGeometry {
  position: Vec<f64>,
  normal: Vec<f64>,
}

impl JsBufferGeometry {
  pub fn from(buffer_geometry: (Vec<f64>, Vec<f64>)) -> Self {
    Self {
      position: buffer_geometry.0,
      normal: buffer_geometry.1,
    }
  }

  pub fn from_solid(solid: &Solid) -> Self {
    web_sys::console::time_with_label("Tesselation");
    let mesh = solid.tesselate();
    web_sys::console::time_end_with_label("Tesselation");
    Self::from(mesh.to_buffer_geometry())
  }

  pub fn from_compound(compound: &Compound) -> Self {
    let mut mesh = Mesh::default();
    for solid in &compound.solids {
      mesh.append(solid.tesselate());
    }
    Self::from(mesh.to_buffer_geometry())
  }
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
