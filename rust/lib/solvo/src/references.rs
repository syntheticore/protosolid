use serde::{Serialize, Deserialize};

use std::collections::HashSet;

use shapex::Profile;
use shapex::Axis;
use shapex::Face;
use shapex::Curve;
use shapex::Plane;
use shapex::CurveType;
use shapex::SurfaceType;
use shapex::Transformable;
use shapex::internal::Ref;

use crate::Uuid;
use crate::Sketch;
use crate::ConstructionHelper;
use crate::ConstructionHelperType;
use crate::FeatureError;
use crate::Component;


pub type CompRef = Uuid;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRef {
  pub sketch_id: Uuid,
  pub profile: Profile,
}

impl ProfileRef {
  pub fn update(&mut self, tree: &Component) -> Result<(), FeatureError> {
    let sketch = self.get_sketch(tree).unwrap().borrow();
    self.profile.plane = (&sketch.work_plane).into();
    sketch.update_profile(&mut self.profile)
  }

  pub fn get_sketch<'a>(&self, tree: &'a Component) -> Option<&'a Ref<Sketch>> {
    tree.find_sketch(self.sketch_id, true)
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
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

  pub fn get_face<'a>(&self, tree: &'a Component) -> Option<&'a Ref<Face>> {
    let comp = tree.find_child(&self.component_id).unwrap();
    comp.compound.find_face_from_bounds(&self.bounds)
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRef {
  pub component_id: CompRef,
  pub edge_id: Uuid,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveRef {
  pub sketch: Ref<Sketch>,
  pub curve: Ref<CurveType>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlanarRef {
  FaceRef(FaceRef),
  HelperRef(Ref<ConstructionHelper>),
}

impl PlanarRef {
  pub fn get_plane(&self, tree: &Component) -> Option<Plane> {
    match self {
      Self::FaceRef(face_ref) => {
        let face = face_ref.get_face(tree);
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


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AxialRef {
  EdgeRef(EdgeRef),
  FaceRef(FaceRef),
  CurveRef(CurveRef),
  HelperRef(Ref<ConstructionHelper>),
}

impl AxialRef {
  pub fn get_axis(&self, _tree: &Component) -> Option<Axis> {
    match self {
      Self::EdgeRef(_) => todo!(),
      Self::FaceRef(_) => todo!(),
      Self::CurveRef(curve_ref) => {
        let curve = curve_ref.curve.borrow();
        if let CurveType::Line(line) = &*curve {
          let mut axis = Axis::from_points(line.endpoints());
          axis.transform(&curve_ref.sketch.borrow().work_plane);
          Some(axis)
        } else {
          unreachable!("Expected CurveType::Line, but got {:?}", curve)
        }
      },
      Self::HelperRef(_) => todo!(),
    }
  }
}
