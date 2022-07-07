use shapex::Ref;
use shapex::Profile;

use crate::Uuid;
use crate::Sketch;
use crate::ConstructionHelper;


pub type CompRef = Uuid;

#[derive(Debug, Clone)]
pub struct FaceRef {
  pub component_id: CompRef,
  pub face_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct EdgeRef {
  pub component_id: CompRef,
  pub edge_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ProfileRef {
  pub sketch: Ref<Sketch>,
  pub profile: Profile,
}

#[derive(Debug, Clone)]
pub enum PlanarRef {
  FaceRef(FaceRef),
  HelperRef(Ref<ConstructionHelper>),
}

#[derive(Debug, Clone)]
pub enum AxialRef {
  EdgeRef(EdgeRef),
  HelperRef(Ref<ConstructionHelper>),
}
