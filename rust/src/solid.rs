use uuid::Uuid;
use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::utils::point_to_js;
use crate::utils::points_to_js;
use crate::buffer_geometry::JsBufferGeometry;


#[wasm_bindgen]
pub struct JsFace {
  real: Ref<Face>,
  solid_id: Uuid,
}

#[wasm_bindgen]
impl JsFace {
  fn from(face: &Ref<Face>, solid_id: Uuid) -> Self {
    Self {
      real: face.clone(),
      solid_id,
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_solid_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn get_origin(&self) -> JsValue {
    point_to_js(self.make_origin())
  }

  pub fn get_normal(&self) -> JsValue {
    point_to_js(Point3::from_vec(self.real.borrow().surface.as_surface().normal_at(0.0, 0.0)))
  }

  pub fn get_display_normal(&self) -> Array {
    let normal = self.real.borrow().surface.as_surface().normal_at(0.0, 0.0);
    let origin = self.make_origin();
    points_to_js(vec![origin, origin + normal])
  }

  fn make_origin(&self) -> Point3 {
    match &self.real.borrow().surface {
      SurfaceType::Planar(plane) => plane.origin,
      SurfaceType::Cylindrical(cyl) => cyl.origin,
    }
  }

  pub fn get_surface_type(&self) -> String {
    match self.real.borrow().surface {
      SurfaceType::Planar(_) => "Planar".to_string(),
      SurfaceType::Cylindrical(_) => "Cylindrical".to_string(),
    }
  }

  pub fn tesselate(&self) -> JsBufferGeometry {
    let this = self.real.borrow();
    JsBufferGeometry::from(
      this.get_surface().tesselate().to_buffer_geometry()
    )
  }
}


#[wasm_bindgen]
pub struct JsEdge {
  real: Ref<Edge>,
  solid_id: Uuid,
}

#[wasm_bindgen]
impl JsEdge {
  fn from(edge: &Ref<Edge>, solid_id: Uuid) -> Self {
    Self {
      real: edge.clone(),
      solid_id,
    }
  }

  pub fn get_id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_solid_id(&self) -> JsValue {
    JsValue::from_serde(&self.solid_id).unwrap()
  }

  pub fn tesselate(&self) -> Array {
    points_to_js(self.real.borrow().curve.as_curve().tesselate())
  }
}


#[wasm_bindgen]
pub struct JsSolid {
  comp: Ref<Component>,
  solid_id: Uuid,
  faces: Array,
  edges: Array,
  vertices: Array,
  pub area: f64,
  pub volume: f64,
}

impl JsSolid {
  pub fn from(solid: &Solid, comp: Ref<Component>) -> Self {
    let shell = &solid.shells[0];
    // Vertices
    let vertices = points_to_js(shell.vertices.iter().map(|v| v.borrow().point ).collect());
    // Edges
    let edges = shell.edges.iter().filter_map(|edge| {
      if edge.borrow().is_inner() {
        None
      } else {
        Some(JsValue::from(JsEdge::from(edge, solid.id)))
      }
    }).collect();
    // Faces
    let faces = shell.faces.iter().map(|f| {
      JsValue::from(JsFace::from(f, solid.id))
    }).collect();
    Self {
      comp,
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
    "Solid".to_string()
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

  pub fn remove(&self) {
    self.comp.borrow_mut().compound.solids.retain(|body| body.id != self.solid_id )
  }

  // pub fn serialize(&self) -> String {
  //   let comp = self.comp.borrow();
  //   let solid = comp.bodies.iter().find(|body| body.id == self.solid_id ).unwrap();
  //   export::export_ron(solid)
  // }
}
