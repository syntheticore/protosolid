use wasm_bindgen::prelude::*;
use js_sys::Array;
use uuid::{uuid, Uuid};
use serde::{Serialize};

use solvo::*;
use shapex::Ref;

use crate::internal::*;
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

  pub fn get_final_tree(&self) -> JsValue {
    let doc = self.real.borrow();
    let mut tree = DummyComponent {
      id: uuid!("00000000-0000-0000-0000-000000000000"),
      children: vec![],
    };
    for feature in &doc.features {
      if let FeatureType::CreateComponent(create_comp) = &feature.borrow().feature_type {
        let new_comp = DummyComponent {
          id: create_comp.new_component_id,
          children: vec![],
        };
        let parent = tree.find_child_mut(&create_comp.component_id).unwrap();
        parent.children.push(new_comp);
      }
    }
    JsValue::from_serde(&tree).unwrap()
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


#[derive(Debug, Clone, Serialize)]
pub struct DummyComponent {
  pub id: Uuid,
  pub children: Vec<Self>,
}

impl DummyComponent {
  pub fn find_child_mut(&mut self, id: &Uuid) -> Option<&mut Self> {
    if *id == self.id { return Some(self) }
    for child in &mut self.children {
      if let Some(target) = child.find_child_mut(id) {
        return Some(target)
      }
    }
    None
  }
}
