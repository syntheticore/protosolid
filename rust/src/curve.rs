use crate::controllable::as_controllable_mut;
use crate::controllable::as_controllable;
use std::cell::RefCell;
use std::rc::Rc;


use shapex::*;

use js_sys::Array;
use wasm_bindgen::prelude::*;


use crate::utils::points_from_js;
use crate::utils::points_to_js;


#[wasm_bindgen]
pub struct JsCurve {
  #[wasm_bindgen(skip)]
  pub real: Rc<RefCell<CurveType>>,
}

impl JsCurve {
  pub fn from(elem: &Rc<RefCell<CurveType>>) -> Self {
    Self {
      real: elem.clone()
    }
  }
}

#[wasm_bindgen]
impl JsCurve {
  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&as_controllable(&mut self.real.borrow()).id()).unwrap()
  }

  pub fn typename(&self) -> String {
    match *self.real.borrow() {
      CurveType::Line(_) => "Line",
      CurveType::Arc(_) => "Arc",
      CurveType::Circle(_) => "Circle",
      CurveType::BezierSpline(_) => "BezierSpline",
    }.to_string()
  }

  pub fn get_radius(&self) -> f64 {
    match &*self.real.borrow() {
      CurveType::Circle(c) => c.radius,
      _ => 0.0
    }
  }

  pub fn get_area(&self) -> f64 {
    let area = match &*self.real.borrow() {
      CurveType::Circle(c) => c.area(),
      _ => 0.0
    };
   area
  }

  pub fn get_handles(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).get_handles())
  }

  pub fn set_handles(&self, handles: Array) {
    let points = points_from_js(handles);
    as_controllable_mut(&mut self.real.borrow_mut()).set_handles(points);
  }

  pub fn set_initial_handles(&self, handles: Array) -> Result<(), JsValue>{
    let mut real = self.real.borrow_mut();
    let points = points_from_js(handles);
    as_controllable_mut(&mut real).set_initial_handles(points)?;
    Ok(())
  }

  pub fn get_snap_points(&self) -> Array {
    points_to_js(as_controllable(&mut self.real.borrow()).get_snap_points())
  }

  pub fn tesselate_adaptive(&self, steps: i32) -> Array {
    points_to_js(self.real.borrow().as_curve().tesselate_adaptive(steps.into()))
  }

  pub fn tesselate(&self) -> Array {
    points_to_js(self.real.borrow().as_curve().tesselate())
  }

  pub fn get_length(&self) -> f64 {
    self.real.borrow().as_curve().length()
  }
}
