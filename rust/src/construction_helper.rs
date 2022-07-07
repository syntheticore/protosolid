use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::utils::matrix_to_js;
use crate::feature::JsPlanarRef;
use crate::feature::JsAxialRef;

// use crate::log;


#[wasm_bindgen]
pub struct JsConstructionHelper {

  component_id: Uuid,

  #[wasm_bindgen(skip)]
  pub real: Ref<ConstructionHelper>,
}

impl JsConstructionHelper {
  pub fn new(component_id: Uuid, helper: &Ref<ConstructionHelper>) -> Self {
    JsConstructionHelper {
      component_id,
      real: helper.clone(),
    }
  }
}

#[wasm_bindgen]
impl JsConstructionHelper {
  pub fn get_transform(&self) -> JsValue {
    let m = match &self.real.borrow().helper_type {
      ConstructionHelperType::Plane(plane) => plane.as_transform(),
      _ => Matrix4::one(),
    };
    matrix_to_js(m)
  }

  pub fn make_reference(&self) -> JsValue {
    let helper = self.real.borrow();
    match &helper.helper_type {
      ConstructionHelperType::Plane(_) => JsValue::from(JsPlanarRef::new(PlanarRef::HelperRef(self.real.clone()))),
      ConstructionHelperType::Axis(_) => JsValue::from(JsAxialRef::new(AxialRef::HelperRef(self.real.clone()))),
    }
  }
}
