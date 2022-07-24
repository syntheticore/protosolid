use uuid::Uuid;
use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;
use shapex::internal::Ref;

use crate::feature::JsPlanarRef;
use crate::feature::JsFaceRef;
use crate::buffer_geometry::JsBufferGeometry;
use crate::utils::point_to_js;
use crate::utils::points_to_js;


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsFace {
  component_id: Uuid,
  real: Ref<Face>,

  #[wasm_bindgen(skip)]
  pub document: Ref<Document>,
}

impl JsFace {
  pub fn from(face: &Ref<Face>, component_id: Uuid, document: Ref<Document>) -> Self {
    Self {
      component_id,
      real: face.clone(),
      document,
    }
  }
}

#[wasm_bindgen]
impl JsFace {
  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_origin(&self) -> JsValue {
    point_to_js(self.make_origin())
  }

  pub fn get_center(&self) -> JsValue {
    self.get_origin()
  }

  pub fn get_normal(&self) -> JsValue {
    point_to_js(Point3::from_vec(self.real.borrow().surface.as_surface().normal_at(0.0, 0.0)))
  }

  // pub fn get_display_normal(&self) -> Array {
  //   let normal = self.real.borrow().surface.as_surface().normal_at(0.0, 0.0);
  //   let origin = self.make_origin();
  //   points_to_js(vec![origin, origin + normal])
  // }

  fn make_origin(&self) -> Point3 {
    match &self.real.borrow().surface {
      SurfaceType::Planar(plane) => plane.plane.origin,
      SurfaceType::Revolution(cyl) => cyl.axis.origin,
      SurfaceType::Spline(spline) => spline.controls[0][0],
    }
  }

  pub fn get_surface_type(&self) -> String {
    match self.real.borrow().surface {
      SurfaceType::Planar(_) => "Planar".into(),
      SurfaceType::Revolution(_) => "Revolution".into(),
      SurfaceType::Spline(_) => "Spline".into(),
    }
  }

  pub fn tesselate(&self) -> JsBufferGeometry {
    let this = self.real.borrow();
    JsBufferGeometry::from(
      this.make_surface().tesselate().to_buffer_geometry()
    )
  }

  pub fn make_face_reference(&self) -> JsValue {
    let face = self.real.borrow();
    JsValue::from(JsFaceRef::new(FaceRef {
      component_id: self.component_id,
      bounds: face.get_edge_ids(),
    }, self.document.clone()))
  }

  pub fn make_planar_reference(&self) -> JsValue {
    let face = self.real.borrow();
    match &face.surface {
      SurfaceType::Planar(_) => JsValue::from(JsPlanarRef::new(PlanarRef::FaceRef(FaceRef {
        component_id: self.component_id,
        bounds: face.get_edge_ids(),
      }), self.document.clone())),
      _ => unreachable!(),
    }
  }

  pub fn duplicate(&self) -> Self {
    self.clone()
  }
}


#[wasm_bindgen]
pub struct JsEdge {
  real: Ref<Edge>,
}

#[wasm_bindgen]
impl JsEdge {
  fn from(edge: &Ref<Edge>) -> Self {
    Self {
      real: edge.clone(),
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn tesselate(&self) -> Array {
    points_to_js(self.real.borrow().left_half.borrow().make_curve().tesselate())
  }
}


#[wasm_bindgen]
pub struct JsSolid {
  solid_id: Uuid,
  faces: Array,
  edges: Array,
  vertices: Array,
  pub area: f64,
  pub volume: f64,
}

impl JsSolid {
  pub fn from(solid: &Solid, component_id: Uuid, document: Ref<Document>) -> Self {
    let shell = &solid.shells[0];
    // Vertices
    let vertices = points_to_js(shell.vertices.iter().map(|v| v.borrow().point ).collect());
    // Edges
    let edges = shell.edges.iter().filter_map(|edge| {
      if edge.borrow().is_inner() {
        None
      } else {
        Some(JsValue::from(JsEdge::from(edge)))
      }
    }).collect();
    // Faces
    let faces = shell.faces.iter().map(|f| {
      JsValue::from(JsFace::from(f, component_id, document.clone()))
    }).collect();
    Self {
      solid_id: solid.id,
      vertices,
      edges,
      faces,
      area: solid.surface_area(),
      volume: solid.volume(),
    }
  }
}

#[wasm_bindgen]
impl JsSolid {
  pub fn typename(&self) -> String {
    "Solid".into()
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn get_faces(&self) -> Array {
    self.faces.clone()
  }

  pub fn get_edges(&self) -> Array {
    self.edges.clone()
  }

  pub fn get_vertices(&self) -> Array {
    self.vertices.clone()
  }

  // pub fn remove(&self) {
  //   self.comp.borrow_mut().compound.solids.retain(|body| body.id != self.solid_id )
  // }

  // pub fn serialize(&self) -> String {
  //   let comp = self.comp.borrow();
  //   let solid = comp.bodies.iter().find(|body| body.id == self.solid_id ).unwrap();
  //   export::export_ron(solid)
  // }
}
