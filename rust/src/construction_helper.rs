use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::utils::matrix_to_js;
use crate::utils::point_to_js;
use crate::feature::JsPlanarRef;
use crate::feature::JsAxialRef;


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsConstructionHelper {

  #[wasm_bindgen(skip)]
  pub real: Ref<ConstructionHelper>,

  document: Ref<Document>,
}

impl JsConstructionHelper {
  pub fn new(helper: &Ref<ConstructionHelper>, document: Ref<Document>) -> Self {
    JsConstructionHelper {
      real: helper.clone(),
      document,
    }
  }
}

#[wasm_bindgen]
impl JsConstructionHelper {
  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn center(&self) -> JsValue {
    point_to_js( match &self.real.borrow().helper_type {
      ConstructionHelperType::Axis(axis) => axis.origin,
      ConstructionHelperType::Plane(plane) => plane.origin,
    })
  }

  pub fn transform(&self) -> JsValue {
    let m = match &self.real.borrow().helper_type {
      ConstructionHelperType::Plane(plane) => plane.as_transform(),
      _ => Matrix4::identity(),
    };
    matrix_to_js(m)
  }

  pub fn make_planar_reference(&self) -> JsValue {
    let helper = self.real.borrow();
    match &helper.helper_type {
      ConstructionHelperType::Plane(_) => JsValue::from(JsPlanarRef::new(PlanarRef::HelperRef(self.real.clone()), self.document.clone())),
      _ => unreachable!(),
    }
  }

    pub fn make_axial_reference(&self) -> JsValue {
    let helper = self.real.borrow();
    match &helper.helper_type {
      ConstructionHelperType::Axis(_) => JsValue::from(JsAxialRef::new(AxialRef::HelperRef(self.real.clone()))),
      _ => unreachable!(),
    }
  }

  pub fn duplicate(&self) -> JsConstructionHelper {
    self.clone()
  }
}
