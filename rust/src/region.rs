use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;
use shapex::internal::Ref;

use crate::utils::vec_to_js;
use crate::utils::point_to_js;
use crate::buffer_geometry::JsBufferGeometry;
use crate::feature::JsProfileRef;


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
  pub fn get_id(&self) -> String {
    let mut s = String::new();
    for wire in &self.profile.rings {
      for tcurve in wire {
        s.push_str(&tcurve.base.get_id().to_string());
      }
    }
    s
  }

  pub fn get_mesh(&self) -> JsBufferGeometry {
    let mesh = self.profile.tesselate();
    JsBufferGeometry::from(mesh.to_buffer_geometry())
  }

  pub fn get_center(&self) -> JsValue {
    let center = self.profile.rings[0].iter().fold(
      Point3::origin(),
      |acc, elem| acc + match &elem.base {
        CurveType::Circle(circle) => circle.plane.origin.to_vec() * 2.0,
        _ => elem.bounds.0.to_vec() + elem.bounds.1.to_vec(),
      }
    ) / (self.profile.rings[0].len() as f64 * 2.0);
    point_to_js(self.sketch.borrow().work_plane.transform_point(center))
  }

  pub fn get_normal(&self) -> JsValue {
    let normal = self.sketch.borrow().work_plane.transform_vector(Vec3::new(0.0, 0.0, 1.0));
    vec_to_js(normal)
  }

  pub fn duplicate(&self) -> Self {
    self.clone()
  }

  pub fn make_reference(&self) -> JsValue {
    JsValue::from(JsProfileRef::new(ProfileRef {
      sketch: self.sketch.clone(),
      profile: self.profile.clone(),
    }))
  }
}

