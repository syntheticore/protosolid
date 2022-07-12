use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::utils::vec_to_js;
use crate::utils::point_to_js;
use crate::buffer_geometry::JsBufferGeometry;


#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JsRegion {
  sketch: Ref<Sketch>,

  #[wasm_bindgen(skip)]
  pub profile: Profile,
}

impl JsRegion {
  pub fn new(profile: Profile, sketch: Ref<Sketch>) -> Self {
    Self {
      profile,
      sketch,
    }
  }
}

#[wasm_bindgen]
impl JsRegion {
  pub fn sketch_id(&self) -> JsValue {
    JsValue::from_serde(&self.sketch.borrow().id).unwrap()
  }

  pub fn get_ids(&self) -> Array {
    let ids = Array::new();
    for wire in &self.profile {
      for tcurve in wire {
        ids.push(&JsValue::from_serde(&tcurve.base.get_id()).unwrap());
      }
    }
    ids
  }

  pub fn get_mesh(&self) -> JsBufferGeometry {
    // web_sys::console::time_with_label("tesselate_profile");
    let mut mesh = geom2d::tesselate_profile(&self.profile, Vec3::unit_z());
    mesh.transform(&self.sketch.borrow().work_plane);
    let geom = JsBufferGeometry::from(mesh.to_buffer_geometry());
    // web_sys::console::time_end_with_label("tesselate_profile");
    geom
  }

  pub fn get_center(&self) -> JsValue {
    let center = self.profile[0].iter().fold(
      Point3::origin(),
      |acc, elem| acc + match &elem.base {
        CurveType::Circle(circle) => circle.plane.origin.to_vec() * 2.0,
        _ => elem.bounds.0.to_vec() + elem.bounds.1.to_vec(),
      }
    ) / (self.profile[0].len() as f64 * 2.0);
    point_to_js(self.sketch.borrow().work_plane.transform_point(center))
  }

  pub fn get_normal(&self) -> JsValue {
    let normal = self.sketch.borrow().work_plane.transform_vector(Vec3::new(0.0, 0.0, 1.0));
    vec_to_js(normal)
  }

  pub fn duplicate(&self) -> JsRegion {
    self.clone()
  }
}
