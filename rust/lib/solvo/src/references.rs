use std::collections::HashSet;

use shapex::Profile;
use shapex::Axis;
use shapex::Face;
use shapex::Curve;
use shapex::Plane;
use shapex::CurveType;
use shapex::SurfaceType;
use shapex::internal::Ref;

use crate::Uuid;
use crate::Sketch;
use crate::ConstructionHelper;
use crate::ConstructionHelperType;
use crate::FeatureError;
use crate::Component;


pub type CompRef = Uuid;


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
pub struct FaceRef {
  pub component_id: CompRef,
  pub bounds: HashSet<Uuid>,
}

impl FaceRef {
  // pub fn new(face: &Face, component_id: Uuid) -> Self {
  //   Self {
  //     component_id,
  //     bounds: face.get_edge_ids(),
  //   }
  // }

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
pub struct CurveRef {
  pub curve: Ref<CurveType>,
}


#[derive(Debug, Clone)]
pub enum PlanarRef {
  FaceRef(FaceRef),
  HelperRef(Ref<ConstructionHelper>),
}

impl PlanarRef {
  pub fn get_plane(&self, top_comp: &Component) -> Option<Plane> {
    match self {
      Self::FaceRef(face_ref) => {
        let face = face_ref.get_face(top_comp);
        if let Some(face) = face {
          let face = face.borrow();
          match &face.surface {
            SurfaceType::Planar(plane) => Some(plane.plane.clone()),
            _ => unreachable!("Expected SurfaceType::Planar, but got {:?}", face.surface),
          }
        } else {
          None
        }
      },
      Self::HelperRef(helper) => {
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
  CurveRef(CurveRef),
  HelperRef(Ref<ConstructionHelper>),
}

impl AxialRef {
  pub fn get_axis(&self, _top_comp: &Component) -> Option<Axis> {
    match self {
      Self::EdgeRef(_) => todo!(),
      Self::FaceRef(_) => todo!(),
      Self::CurveRef(curve_ref) => {
        let curve = curve_ref.curve.borrow();
        if let CurveType::Line(line) = &*curve {
          Some(Axis::from_points(line.endpoints()))
        } else {
          unreachable!("Expected CurveType::Line, but got {:?}", curve)
        }
      },
      Self::HelperRef(_) => todo!(),
    }
  }
}
