use std::collections::HashSet;

use shapex::Ref;
use shapex::Profile;
use shapex::Face;
use shapex::Plane;
use shapex::SurfaceType;

use crate::Uuid;
use crate::Sketch;
use crate::ConstructionHelper;
use crate::ConstructionHelperType;
use crate::FeatureError;
use crate::Component;


pub type CompRef = Uuid;


#[derive(Debug, Clone)]
pub struct FaceRef {
  pub component_id: CompRef,
  pub bounds: HashSet<Uuid>, // Edge/Curve Ids
}

impl FaceRef {
  pub fn get_face<'a>(&self, top_comp: &'a Component) -> Option<&'a Ref<Face>> {
    let comp = top_comp.find_child(&self.component_id).unwrap();
    comp.compound.find_face_from_bounds(&self.bounds)
  }
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

impl ProfileRef {
  pub fn update(&mut self) -> Result<(), FeatureError> {
    self.sketch.borrow().update_profile(&mut self.profile)
  }
}


#[derive(Debug, Clone)]
pub enum PlanarRef {
  FaceRef(FaceRef),
  HelperRef(Ref<ConstructionHelper>),
}

impl PlanarRef {
  pub fn get_plane(&self, top_comp: &Component) -> Option<Plane> {
    match self {
      PlanarRef::FaceRef(face_ref) => {
        let face = face_ref.get_face(top_comp);
        if let Some(face) = face {
          let face = face.borrow();
          match &face.surface {
            SurfaceType::Planar(plane) => Some(plane.clone()),
            _ => unreachable!("Expected SurfaceType::Planar in {:?}, but got {:?}", self, face.surface),
          }
        } else {
          None
        }
      },
      PlanarRef::HelperRef(helper) => {
        let helper = helper.borrow();
        if let ConstructionHelperType::Plane(plane) = &helper.helper_type {
          Some(plane.clone())
        } else { unreachable!("Expected ConstructionHelperType::Plane, but got {:?}", helper.helper_type) }
      },
    }
  }
}


#[derive(Debug, Clone)]
pub enum AxialRef {
  EdgeRef(EdgeRef),
  FaceRef(FaceRef),
  HelperRef(Ref<ConstructionHelper>),
}
