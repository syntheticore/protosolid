use std::rc::Rc;
use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::utils::point_to_js;
use crate::buffer_geometry::JsBufferGeometry;


#[wasm_bindgen]
pub struct JsRegion {
  #[wasm_bindgen(skip)]
  pub region: Vec<TrimmedCurve>,
  #[wasm_bindgen(skip)]
  pub component: Rc<RefCell<Component>>,
}

#[wasm_bindgen]
impl JsRegion {
  pub fn get_mesh(&mut self) -> JsBufferGeometry {
    let poly = geom2d::tesselate_wire(&self.region);
    JsBufferGeometry::from(
      geom2d::tesselate_polygon(poly, Vec3::unit_z()).to_buffer_geometry()
    )
  }

  pub fn get_center(&self) -> JsValue {
    let center = self.region.iter().fold(
      Point3::new(0.0, 0.0, 0.0),
      |acc, elem| acc + match &elem.base {
        CurveType::Circle(circle) => circle.center.to_vec() * 2.0,
        _ => elem.bounds.0.to_vec() + elem.bounds.1.to_vec(),
      }
    ) / (self.region.len() as f64 * 2.0);
    point_to_js(center)
  }

  pub fn extrude(&self, distance: f64) {
    web_sys::console::time_with_label("BREP extrude");
    let tool = features::extrude(self.region.clone(), distance).unwrap();
    // Solid::boolean_all(&mut self.component.borrow_mut().bodies, tool, BooleanType::Add);
    self.component.borrow_mut().compound.add(tool.into_compound());
    web_sys::console::time_end_with_label("BREP extrude");
  }

  pub fn extrude_preview(&self, distance: f64) -> JsValue {
    let extrusion = features::extrude(self.region.clone(), distance);
    match extrusion {
      Ok(res) => JsValue::from(JsBufferGeometry::from_solid(&res)),
      Err(error) => JsValue::from(error),
    }
  }
}
