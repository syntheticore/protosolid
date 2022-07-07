use wasm_bindgen::prelude::*;
use js_sys::Array;

use shapex::*;
use solvo::*;

use crate::document::JsDocument;
use crate::sketch::JsSketch;
use crate::region::JsRegion;
use crate::buffer_geometry::JsBufferGeometry;
// use crate::log;


#[wasm_bindgen]
pub struct JsPlanarRef(PlanarRef);

impl JsPlanarRef {
  pub fn new(real: PlanarRef) -> Self {
    Self(real)
  }
}


#[wasm_bindgen]
pub struct JsAxialRef(AxialRef);

impl JsAxialRef {
  pub fn new(real: AxialRef) -> Self {
    Self(real)
  }
}


// #[wasm_bindgen]
// pub struct JsSketchRef(SketchRef);


#[wasm_bindgen]
pub struct JsProfileList {
  profiles: Vec<JsRegion>,
}

#[wasm_bindgen]
impl JsProfileList {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Self {
      profiles: vec![],
    }
  }

  pub fn push(&mut self, profile: &JsRegion) {
    self.profiles.push((*profile).clone());
  }
}


#[wasm_bindgen]
pub struct JsFeature {
  document: Ref<Document>,

  #[wasm_bindgen(skip)]
  pub real: Option<Ref<Feature>>,
}

impl JsFeature {
  pub fn from_real(document: &Ref<Document>, real: &Ref<Feature>) -> Self {
    Self {
      document: document.clone(),
      real: Some(real.clone()),
    }
  }
}

#[wasm_bindgen]
impl JsFeature {
  #[wasm_bindgen(constructor)]
  pub fn new(doc: &JsDocument) -> Self {
    Self {
      document: doc.real.clone(),
      real: None,
    }
  }

  pub fn id(&self) -> JsValue {
    let feature = self.real.as_ref().unwrap().borrow();
    JsValue::from_serde(&feature.id).unwrap()
  }

  pub fn preview(&self) -> JsValue {
    match self.real.as_ref() {
      Some(real) => {
        if let Some(compound) = real.borrow().feature_type.as_feature().preview() {
          JsValue::from(JsBufferGeometry::from_compound(&compound))
        } else {
          JsValue::undefined()
        }
      },
      None => JsValue::undefined(),
    }
  }

  pub fn error(&self) -> JsValue {
    if let Some(real) = self.real.as_ref() {
      real.borrow().error.as_ref().map_or(JsValue::undefined(), |e| JsValue::from(e.to_string()) )
    } else {
      JsValue::undefined()
    }
  }

  pub fn modified_components(&self) -> Array {
    self.real.as_ref().unwrap().borrow().feature_type.as_feature().modified_components()
      .iter().map(|comp_ref| JsValue::from_serde(comp_ref).unwrap() ).collect()
  }

  pub fn create_component(&mut self, comp_ref: JsValue) {
    self.process_feature(Feature::new(
      CreateComponentFeature {
          component_id: comp_ref.into_serde().unwrap(),
          new_component_id: Uuid::new_v4(),
        }.into_enum(),
    ));
  }

  pub fn create_sketch(&mut self, comp_ref: JsValue, plane: &JsPlanarRef) {
    self.process_feature(Feature::new(
      CreateSketchFeature {
        component_id: comp_ref.into_serde().unwrap(),
        plane: plane.0.clone(),
        sketch: rc(Sketch::default()),
        title: "New Sketch".into(),
      }.into_enum(),
    ));
  }

  pub fn extrusion(&mut self, comp_ref: JsValue, sketch: &JsSketch, profiles: &JsProfileList, distance: f64, op: &str) {
    let profiles = &profiles.profiles;
    self.process_feature(Feature::new(
      ExtrusionFeature {
        component_id: comp_ref.into_serde().unwrap(),
        profiles: profiles.iter().map(|p| ProfileRef {
          sketch: sketch.real.clone(),
          profile: p.profile.clone(),
        }).collect(),
        distance,
        op: get_op(op),
      }.into_enum(),
    ));
  }

  fn process_feature(&mut self, feature: Feature) {
    let mut doc = self.document.borrow_mut();
    if let Some(this) = &mut self.real {
      *this.borrow_mut() = feature;
      doc.invalidate_feature(this);
    } else {
      let feature = rc(feature);
      self.real = Some(feature.clone());
      doc.add_feature(feature);
    }
  }

  pub fn invalidate(&mut self) {
    let mut doc = self.document.borrow_mut();
    if let Some(this) = &mut self.real {
      doc.invalidate_feature(this);
    }
  }

  pub fn remove(&mut self) {
    if let Some(this) = &mut self.real {
      self.document.borrow_mut().remove_feature(this);
      self.real = None;
    }
  }
}

fn get_op(str: &str) -> BooleanType {
  match str {
    "join" => BooleanType::Join,
    "cut" => BooleanType::Cut,
    "intersect" => BooleanType::Intersection,
    "create" => BooleanType::Create,
    _ => BooleanType::Create,
  }
}
