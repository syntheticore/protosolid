use shapex::*;

use crate::base::*;
use crate::Uuid;
use crate::Component;
use crate::Sketch;



#[derive(Debug, Clone)]
pub struct Feature {
  pub id: Uuid,
  pub error: Option<FeatureError>,
  pub feature_type: FeatureType,
}

impl Feature {
  pub fn new(feature_type: FeatureType) -> Self {
    Self {
      id: Uuid::new_v4(),
      error: None,
      feature_type,
    }
  }
}


pub trait FeatureTrait {
  fn preview(&self) -> Option<Compound>;
  fn execute(&mut self, top_comp: &mut Component) -> Result<(), FeatureError>;
  fn modified_components(&self) -> Vec<CompRef>;
}


#[derive(Debug, Clone)]
pub enum FeatureType {
  CreateComponent(CreateComponentFeature),
  CreateSketch(CreateSketchFeature),
  Extrusion(ExtrusionFeature),
}

impl FeatureType {
  pub fn as_feature(&self) -> &dyn FeatureTrait {
    match self {
      Self::CreateComponent(f) => f,
      Self::CreateSketch(f) => f,
      Self::Extrusion(f) => f,
    }
  }

  pub fn as_feature_mut(&mut self) -> &mut dyn FeatureTrait {
    match self {
      Self::CreateComponent(f) => f,
      Self::CreateSketch(f) => f,
      Self::Extrusion(f) => f,
    }
  }
}


#[derive(Debug, Clone)]
pub enum FeatureError {
  Warning(String),
  Error(String),
}

impl std::fmt::Display for FeatureError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      Self::Warning(str) => write!(f, "{}", str),
      Self::Error(str) => write!(f, "{}", str),
    }
  }
}

// impl From<FeatureError> for String {
//   fn from(error: FeatureError) -> Self {
//     error.into()
//   }
// }

impl std::error::Error for FeatureError {}


#[derive(Debug, Clone)]
pub struct ConstructionHelper {
  pub id: Uuid,
  pub helper_type: ConstructionHelperType,
}

impl ConstructionHelper {
  pub fn new(helper_type: ConstructionHelperType) -> Self {
    Self {
      id: Uuid::new_v4(),
      helper_type,
    }
  }
}


#[derive(Debug, Clone)]
pub enum ConstructionHelperType {
  Axis(geom3d::Axis),
  Plane(Plane),
}


#[derive(Debug, Clone)]
pub struct CreateComponentFeature {
  pub component_id: CompRef,
  pub new_component_id: Uuid,
}

impl CreateComponentFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::CreateComponent(self)
  }
}

impl FeatureTrait for CreateComponentFeature {
  fn preview(&self) -> Option<Compound> { None }

  fn execute(&mut self, top_comp: &mut Component) -> Result<(), FeatureError> {
    let comp = top_comp.find_child_mut(&self.component_id).unwrap();
    let new_comp = comp.create_component();
    new_comp.id = self.new_component_id;
    Ok(())
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }
}


#[derive(Debug, Clone)]
pub struct CreateSketchFeature {
  pub component_id: CompRef,
  pub plane: PlanarRef,
  pub sketch: Ref<Sketch>,
}

impl CreateSketchFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::CreateSketch(self)
  }
}

impl FeatureTrait for CreateSketchFeature {
  fn preview(&self) -> Option<Compound> { None }

  fn execute(&mut self, top_comp: &mut Component) -> Result<(), FeatureError> {
    // Refetch sketch plane from face or plane helper
    self.sketch.borrow_mut().work_plane = match &self.plane {
      PlanarRef::FaceRef(face_ref) => {
        let comp = top_comp.find_child(&face_ref.component_id).unwrap();
        let face = comp.compound.find_face_from_bounds(&face_ref.bounds).unwrap().borrow();
        match &face.surface {
          SurfaceType::Planar(plane) => plane.as_transform(),
          _ => panic!("Expected SurfaceType::Planar in {:?}, but got {:?}", self.plane, face.surface),
        }
      },
      PlanarRef::HelperRef(helper) => {
        let helper = helper.borrow();
        if let ConstructionHelperType::Plane(plane) = &helper.helper_type {
          plane.as_transform()
        } else { panic!("Expected ConstructionHelperType::Plane, but got {:?}", helper.helper_type) }
      },
    };
    // Fetch component and add sketch
    let comp = top_comp.find_child_mut(&self.component_id).unwrap();
    comp.add_sketch(self.sketch.clone());
    Ok(())
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }
}


#[derive(Debug, Clone)]
pub struct ExtrusionFeature {
  pub component_id: Uuid,
  pub profiles: Vec<ProfileRef>,
  pub distance: f64,
  pub op: BooleanType,
  pub face_id_seed: Uuid,
}

impl ExtrusionFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::Extrusion(self)
  }

  fn make_tool(&self) -> Result<Compound, FeatureError> {
    let mut tool = Compound::default();
    for profile_ref in &self.profiles {
      let mut profile = profile_ref.profile.clone();
      profile_ref.sketch.borrow().transform_profile(&mut profile);
      match features::extrude(&profile, self.distance) {
        Ok(compound) => tool.join(compound),
        Err(error) => return Err(FeatureError::Error(error)),
      }
    }
    Ok(tool)
  }

  fn update_profiles(&mut self) {
    for profile_ref in &mut self.profiles {
      let sketch = profile_ref.sketch.borrow();
      sketch.update_profile(&mut profile_ref.profile);
    }
  }
}

impl FeatureTrait for ExtrusionFeature {
  fn preview(&self) -> Option<Compound> {
    self.make_tool().ok()
  }

  fn execute(&mut self, top_comp: &mut Component) -> Result<(), FeatureError> {
    self.update_profiles();
    let tool = self.make_tool()?;
    let comp = top_comp.find_child_mut(&self.component_id).unwrap();
    comp.compound.boolean(tool.clone(), self.op);
    Ok(())
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }
}
