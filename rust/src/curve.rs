use std::rc::Rc;
use js_sys::Array;
use wasm_bindgen::prelude::*;

use shapex::*;
use solvo::Sketch;
use solvo::AxialRef;
use solvo::CurveRef;

use crate::utils::points_from_js;
use crate::utils::points_to_js;
use crate::utils::point_to_js;
use crate::controllable::as_controllable_mut;
use crate::controllable::as_controllable;
use crate::feature::JsAxialRef;


#[wasm_bindgen]
pub struct JsCurve {
  #[wasm_bindgen(skip)]
  pub real: Ref<CurveType>,
  sketch: Ref<Sketch>,
}

impl JsCurve {
  pub fn from(elem: Ref<CurveType>, sketch: Ref<Sketch>) -> Self {
    Self {
      real: elem,
      sketch: sketch,
    }
  }
}

#[wasm_bindgen]
impl JsCurve {
  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id()).unwrap()
  }

  pub fn typename(&self) -> String {
    match *self.real.borrow() {
      CurveType::Line(_) => "Line",
      CurveType::Arc(_) => "Arc",
      CurveType::Circle(_) => "Circle",
      CurveType::Spline(_) => "Spline",
    }.to_string()
  }

  pub fn center(&self) -> JsValue {
    let mut midpoint = self.real.borrow().as_curve().midpoint();
    midpoint = self.sketch.borrow().work_plane.transform_point(midpoint);
    point_to_js(midpoint)
  }

  pub fn radius(&self) -> f64 {
    match &*self.real.borrow() {
      CurveType::Circle(c) => c.radius,
      CurveType::Arc(a) => a.radius,
      _ => 0.0
    }
  }

  pub fn area(&self) -> f64 {
    let area = match &*self.real.borrow() {
      CurveType::Circle(c) => c.area(),
      _ => 0.0
    };
   area
  }

  pub fn length(&self) -> f64 {
    self.real.borrow().as_curve().length()
  }

  pub fn handles(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).handles().iter().map(|handle| {
      self.sketch.borrow().work_plane.transform_point(*handle)
    }).collect())
  }

  pub fn set_handles(&self, handles: Array, initial: bool) {
    let points = points_from_js(handles);
    let transform = self.sketch.borrow().work_plane.invert().unwrap();
    let points = points.iter().map(|p| transform.transform_point(*p) ).collect();
    if initial {
      as_controllable_mut(&mut self.real.borrow_mut()).set_initial_handles(points);
    } else {
      as_controllable_mut(&mut self.real.borrow_mut()).set_handles(points);
    }
  }

  pub fn snap_points(&self) -> Array {
    let points = as_controllable(&mut self.real.borrow()).snap_points();
    let plane = self.sketch.borrow().work_plane;
    let points = points.iter().map(|p| plane.transform_point(*p) ).collect();
    points_to_js(points)
  }

  pub fn tesselate(&self) -> Array {
    let mut curve = self.real.borrow().clone();
    curve.as_curve_mut().transform(&self.sketch.borrow().work_plane);
    points_to_js(curve.as_curve().tesselate())
  }

  pub fn remove(&self) {
    self.sketch.borrow_mut().elements.retain(|elem| !Rc::ptr_eq(elem, &self.real) );
  }

  pub fn make_axial_reference(&self) -> JsValue {
    let curve = self.real.borrow();
    match *curve {
      CurveType::Line(_) => JsValue::from(JsAxialRef::new(AxialRef::CurveRef(CurveRef {
        curve: self.real.clone(),
        sketch: self.sketch.clone(),
      }))),
      _ => unreachable!(),
    }
  }
}
