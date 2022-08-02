use wasm_bindgen::prelude::*;
use js_sys::Array;

use solvo::*;
use shapex::internal::rc;
use shapex::internal::Ref;

use crate::component::JsComponent;
use crate::feature::JsFeature;


#[wasm_bindgen]
pub struct JsDocument {

  #[wasm_bindgen(skip)]
  pub real: Ref<Document>,
}

#[wasm_bindgen]
impl JsDocument {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Self { real: rc(Document::new()) }
  }

  #[wasm_bindgen(getter)]
  pub fn marker(&self) -> usize {
    self.real.borrow().get_marker()
  }

  #[wasm_bindgen(setter)]
  pub fn set_marker(&mut self, index: usize) {
    self.real.borrow_mut().move_marker(index);
  }

  pub fn move_marker_to_feature(&self, feature: &JsFeature) {
    self.real.borrow_mut().move_marker_to_feature(&feature.real.as_ref().unwrap());
  }

  pub fn get_tree(&self) -> JsComponent {
    let real = self.real.borrow();
    let comp = real.get_tree();
    JsComponent { component_id: comp.id, document: self.real.clone() }
  }

  pub fn evaluate(&self) -> Array {
    self.real.borrow_mut().evaluate().iter().map(|comp_ref| JsValue::from_serde(comp_ref).unwrap() ).collect()
  }

  pub fn get_features(&self) -> Array {
    self.real.borrow().features.iter().map(|feature|
      JsValue::from(JsFeature::from_real(&self.real, &feature))
    ).collect()
  }

  pub fn serialize(&self) -> String {
    let doc = self.real.borrow();
    ron::to_string(&*doc).unwrap()
  }

  pub fn deserialize(&mut self, dump: String) {
    self.real = rc(ron::from_str(&dump).unwrap());
  }
}
