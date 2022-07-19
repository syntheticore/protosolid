use wasm_bindgen::prelude::*;
use js_sys::Array;

use shapex::*;
use shapex::internal::rc;
use shapex::internal::Ref;
use solvo::*;

use crate::document::JsDocument;
use crate::region::JsRegion;
use crate::buffer_geometry::JsBufferGeometry;
// use crate::log;


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsPlanarRef(PlanarRef);

impl JsPlanarRef {
  pub fn new(real: PlanarRef) -> Self {
    Self(real)
  }
}


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsAxialRef(AxialRef);

impl JsAxialRef {
  pub fn new(real: AxialRef) -> Self {
    Self(real)
  }
}


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsFaceRef(FaceRef);

impl JsFaceRef {
  pub fn new(real: FaceRef) -> Self {
    Self(real)
  }
}

#[wasm_bindgen]
impl JsFaceRef {
  pub fn get_ids(&self) -> Array {
    let ids = Array::new();
    for bound in &self.0.bounds {
      ids.push(&JsValue::from_serde(bound).unwrap());
    }
    ids
  }
}


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsProfileRef(ProfileRef);

impl JsProfileRef {
  pub fn new(real: ProfileRef) -> Self {
    Self(real)
  }
}


#[wasm_bindgen]
pub struct JsProfileRefList {
  profiles: Vec<JsProfileRef>,
}

#[wasm_bindgen]
impl JsProfileRefList {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Self {
      profiles: vec![],
    }
  }

  pub fn push(&mut self, profile: &JsProfileRef) {
    self.profiles.push(profile.clone());
  }
}


#[wasm_bindgen]
pub struct JsFaceRefList {
  faces: Vec<JsFaceRef>,
}

#[wasm_bindgen]
impl JsFaceRefList {

  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    Self {
      faces: vec![],
    }
  }

  pub fn push(&mut self, face: &JsFaceRef) {
    self.faces.push(face.clone());
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
      real.borrow().error.as_ref().map_or(JsValue::undefined(), |e| JsValue::from(vec![e.to_string(), match e {
        FeatureError::Warning(_) => "warning".into(),
        FeatureError::Error(_) => "error".into(),
      }].iter().map(|item| JsValue::from(item) ).collect::<Array>()))
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
      }.into_enum(),
    ));
  }

  pub fn extrusion(&mut self, comp_ref: JsValue, profiles: JsProfileRefList, distance: f64, op: &str) {
    let profiles = &profiles.profiles;
    let feature_id = self.real.as_ref().map_or(Uuid::new_v4(), |real| real.borrow().id );
    let mut feature = Feature::new(
      ExtrusionFeature {
        component_id: comp_ref.into_serde().unwrap(),
        profiles: profiles.iter().map(|profile| profile.0.clone() ).collect(),
        distance,
        op: get_op(op),
      }.into_enum(),
    );
    feature.id = feature_id;
    self.process_feature(feature);
  }

  pub fn draft(&mut self, faces: JsFaceRefList, ref_plane: &JsPlanarRef, angle: f64) {
    let faces = &faces.faces;
    let feature = Feature::new(
      DraftFeature {
        fixed_plane: ref_plane.0.clone(),
        faces: faces.iter().map(|face| face.0.clone() ).collect(),
        angle: Deg(angle),
      }.into_enum(),
    );
    self.process_feature(feature);
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

  pub fn repair(&mut self) {
    let mut doc = self.document.borrow_mut();
    if let Some(this) = &mut self.real {
      doc.repair_feature(this);
    }
  }

  pub fn remove(&mut self) {
    if let Some(this) = &mut self.real {
      self.document.borrow_mut().remove_feature(this);
      self.real = None;
    }
  }

  pub fn get_profiles(&self) -> Array {
    if let Some(real) = &self.real {
      if let FeatureType::Extrusion(feature) = &real.borrow().feature_type {
        feature.profiles.iter().map(|profile| JsValue::from(JsRegion::new(profile.profile.clone(), profile.sketch.clone())) ).collect()
      } else {
        Array::new()
      }
    } else {
      Array::new()
    }
  }

  pub fn get_face_refs(&self) -> Array {
    if let Some(real) = &self.real {
      if let FeatureType::Draft(feature) = &real.borrow().feature_type {
        feature.faces.iter().map(|face_ref| JsValue::from(JsFaceRef::new(face_ref.clone())) ).collect()
      } else {
        Array::new()
      }
    } else {
      Array::new()
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
