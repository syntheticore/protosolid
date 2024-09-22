use serde::{Serialize, Deserialize};

use shapex::*;

use crate::references::*;
use crate::Uuid;
use crate::Component;
use crate::Sketch;



#[derive(Debug, Clone, Serialize, Deserialize)]
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
  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError>;
  fn modified_components(&self) -> Vec<CompRef>;
  fn repair(&mut self, _tree: &Component) {}
  fn preview(&self, _tree: &Component) -> Option<Compound> { None }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeatureType {
  CreateComponent(CreateComponentFeature),
  CreateSketch(CreateSketchFeature),
  Extrusion(ExtrusionFeature),
  Revolution(RevolutionFeature),
  Draft(DraftFeature),
}

impl FeatureType {
  pub fn as_feature(&self) -> &dyn FeatureTrait {
    match self {
      Self::CreateComponent(f) => f,
      Self::CreateSketch(f) => f,
      Self::Extrusion(f) => f,
      Self::Revolution(f) => f,
      Self::Draft(f) => f,
    }
  }

  pub fn as_feature_mut(&mut self) -> &mut dyn FeatureTrait {
    match self {
      Self::CreateComponent(f) => f,
      Self::CreateSketch(f) => f,
      Self::Extrusion(f) => f,
      Self::Revolution(f) => f,
      Self::Draft(f) => f,
    }
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
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


#[derive(Debug, Clone, Serialize, Deserialize)]
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


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstructionHelperType {
  Axis(Axis),
  Plane(Plane),
}


fn update_profiles(profiles: &mut Vec<ProfileRef>, tree: &Component) -> Result<(), FeatureError> {
  let mut res = Ok(());
  for profile_ref in profiles {
    let result = profile_ref.update(tree);
    match result {
      Err(FeatureError::Error(_)) => return result,
      Err(FeatureError::Warning(_)) => res = result,
      Ok(_) => {},
    }
  }
  res
}


#[derive(Debug, Clone, Serialize, Deserialize)]
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
  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError> {
    let comp = tree.find_child_mut(&self.component_id).unwrap();
    let new_comp = comp.create_component();
    new_comp.id = self.new_component_id;
    Ok(())
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
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
  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError> {
    // Refetch sketch plane from face or plane helper
    let result = if let Some(plane) = self.plane.get_plane(tree) {
      self.sketch.borrow_mut().work_plane = plane.as_transform();
      Ok(())
    } else {
      Err(FeatureError::Warning("Sketch plane was lost".into()))
    };
    // Fetch component and add sketch
    let comp = tree.find_child_mut(&self.component_id).unwrap();
    comp.add_sketch(self.sketch.clone());
    result
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtrusionFeature {
  pub component_id: Uuid,
  pub profiles: Vec<ProfileRef>,
  pub distance: f64,
  pub op: BooleanType,
}

impl ExtrusionFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::Extrusion(self)
  }

  fn make_tool(&self, profiles: &Vec<ProfileRef>, tree: &Component) -> Result<Compound, FeatureError> {
    let mut tool = Compound::default();
    for profile_ref in profiles {
      let mut profile = profile_ref.profile.clone();
      profile.transform(&profile_ref.get_sketch(tree).unwrap().borrow().work_plane);
      match features::extrude(&profile, self.distance) {
        Ok(solid) => tool.join(solid.into_compound()),
        Err(error) => return Err(FeatureError::Error(error)),
      }
    }
    Ok(tool)
  }
}

impl FeatureTrait for ExtrusionFeature {
  fn preview(&self, tree: &Component) -> Option<Compound> {
    let mut profiles = self.profiles.clone();
    match update_profiles(&mut profiles, tree) {
      Err(FeatureError::Error(_)) => None,
      Err(FeatureError::Warning(_)) | Ok(_) => self.make_tool(&profiles, tree).ok(),
    }
  }

  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError> {
    let mut profiles = self.profiles.clone();
    let result = update_profiles(&mut profiles, tree);
    if let Err(FeatureError::Error(_)) = result {
      return result;
    }
    let tool = self.make_tool(&profiles, tree)?;
    let comp = tree.find_child_mut(&self.component_id).unwrap();
    comp.compound.boolean(tool.clone(), self.op);
    result
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }

  fn repair(&mut self, tree: &Component) {
    update_profiles(&mut self.profiles, tree).ok();
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevolutionFeature {
  pub component_id: Uuid,
  pub profiles: Vec<ProfileRef>,
  pub axis: AxialRef,
  pub angle: Deg<f64>,
  pub op: BooleanType,
  pub preview_compound: Option<Compound>,
}

impl RevolutionFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::Revolution(self)
  }

  fn make_tool(&self, profiles: &Vec<ProfileRef>, tree: &Component) -> Result<Compound, FeatureError> {
    let mut tool = Compound::default();
    if let Some(axis) = self.axis.get_axis(tree) {
      for profile_ref in profiles {
        let mut profile = profile_ref.profile.clone();
        profile.transform(&profile_ref.get_sketch(tree).unwrap().borrow().work_plane);
        match features::revolve(&profile, axis.clone(), self.angle) {
          Ok(solid) => tool.join(solid.into_compound()),
          Err(error) => return Err(FeatureError::Error(error)),
        }
      }
      Ok(tool)
    } else {
      Err(FeatureError::Error("Axis was lost".into()))
    }
  }
}

impl FeatureTrait for RevolutionFeature {
  fn preview(&self, _tree: &Component) -> Option<Compound> {
    self.preview_compound.clone()
  }

  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError> {
    self.preview_compound = None;
    let mut profiles = self.profiles.clone();
    let result = update_profiles(&mut profiles, tree);
    if let Err(FeatureError::Error(_)) = result {
      return result;
    }
    let tool = self.make_tool(&profiles, tree)?;
    let comp = tree.find_child_mut(&self.component_id).unwrap();
    comp.compound.boolean(tool.clone(), self.op);
    self.preview_compound = Some(tool);
    result
  }

  fn modified_components(&self) -> Vec<CompRef> {
    vec![self.component_id]
  }

  fn repair(&mut self, tree: &Component) {
    update_profiles(&mut self.profiles, tree).ok();
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftFeature {
  pub fixed_plane: PlanarRef,
  pub faces: Vec<FaceRef>,
  pub angle: Deg<f64>,
}

impl DraftFeature {
  pub fn into_enum(self) -> FeatureType {
    FeatureType::Draft(self)
  }
}

impl FeatureTrait for DraftFeature {
  fn execute(&mut self, tree: &mut Component) -> Result<(), FeatureError> {
    if let Some(plane) = self.fixed_plane.get_plane(tree) {
      let found_faces = self.faces.iter().filter_map(|face| face.get_face(tree) ).cloned().collect();
      let result = features::draft(&found_faces, &plane, self.angle)
      .map_err(|error| FeatureError::Error(error) );
      if found_faces.len() == self.faces.len() {
        result
      } else {
        Err(result.err().unwrap_or(FeatureError::Warning("Some faces could not be found".into())))
      }
    } else {
      Err(FeatureError::Error("Reference plane was lost".into()))
    }
  }

  fn modified_components(&self) -> Vec<CompRef> {
    let mut ids: Vec<Uuid> = self.faces.iter().map(|face| face.component_id ).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
  }

  fn repair(&mut self, tree: &Component) {
    self.faces.retain(|face| face.get_face(tree).is_some() );
  }
}
