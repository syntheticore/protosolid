use std::cell::RefCell;
use std::rc::Rc;

use solvo::*;
use shapex::*;

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::controllable::as_controllable;
use crate::curve::JsCurve;
use crate::region::JsRegion;
use crate::utils::matrix_to_js;
use crate::utils::matrix_from_js;
// use crate::log;


#[wasm_bindgen]
#[derive(Default)]
pub struct JsSketch {
  real: Ref<Component>,
}

impl JsSketch {
  pub fn from(comp: &Ref<Component>) -> Self {
    JsSketch {
      real: comp.clone(),
    }
  }
}

#[wasm_bindgen]
impl JsSketch {
  pub fn get_sketch_elements(&self) -> Array {
    self.real.borrow().sketch.elements.iter().map(|elem| {
      JsValue::from(JsCurve::from(elem))
    }).collect()
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsCurve {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = Line::new(Point3::from(p1), Point3::from(p2));
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(line.into_enum())));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsCurve {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = BezierSpline::new(points);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(Rc::new(RefCell::new(CurveType::BezierSpline(spline))));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: f64) -> JsCurve {
    let center: (f64, f64, f64) = center.into_serde().unwrap();
    let circle = Circle::new(Point3::from(center), radius);
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(rc(CurveType::Circle(circle)));
    JsCurve::from(&real.sketch.elements.last().unwrap())
  }

  pub fn add_arc(&mut self, p1: JsValue, p2: JsValue, p3: JsValue) -> Result<JsCurve, JsValue> {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let p3: (f64, f64, f64) = p3.into_serde().unwrap();
    let arc = Arc::from_points(Point3::from(p1), Point3::from(p2), Point3::from(p3))?;
    let mut real = self.real.borrow_mut();
    real.sketch.elements.push(rc(arc.into_enum()));
    Ok(JsCurve::from(&real.sketch.elements.last().unwrap()))
  }

  pub fn remove_element(&mut self, id: JsValue) {
    let id: uuid::Uuid = id.into_serde().unwrap();
    let mut real = self.real.borrow_mut();
    real.sketch.elements.retain(|elem| as_controllable(&mut elem.borrow_mut()).id() != id );
  }

  pub fn get_workplane(&self) -> JsValue {
    let plane = self.real.borrow().sketch.work_plane;
    matrix_to_js(plane)
  }

  pub fn set_workplane(&self, plane: JsValue) {
    let plane = matrix_from_js(plane);
    self.real.borrow_mut().sketch.work_plane = plane;
  }

  pub fn get_profiles(&self) -> Array {
    web_sys::console::time_with_label("get_profiles");
    let comp = self.real.borrow();
    let profiles = comp.sketch.get_profiles(false);
    web_sys::console::time_end_with_label("get_profiles");
    profiles.into_iter()
    .map(|profile| JsValue::from(JsRegion {
      profile: profile.1,
      plane: profile.0.as_transform(),
      // plane: comp.sketch.work_plane,
      component: self.real.clone(),
    }))
    .collect()
  }

  pub fn get_all_split(&self) {
    let mut real = self.real.borrow_mut();

    let planar_elements = real.sketch.get_planarized_elements();
    let splits: Vec<TrimmedCurve> = Sketch::all_split(&planar_elements);

    let (mut circles, mut others): (Vec<TrimmedCurve>, Vec<TrimmedCurve>) = splits.into_iter().partition(|elem| match elem.base {
      CurveType::Circle(_) => true,
      _ => false,
    });

    Sketch::remove_dangling_segments(&mut others);
    others.append(&mut circles);

    for tcurve in &mut others {
      tcurve.transform(&real.sketch.work_plane);
    }

    real.sketch.elements.clear();
    for split in others.iter() {
      real.sketch.elements.push(Rc::new(RefCell::new(split.cache.clone())));
    }
  }

  pub fn get_trimmed(&self, elem: JsCurve, _p: JsValue) -> Array {
    let splits = Sketch::split_element(&elem.real.borrow(), &self.real.borrow().sketch.elements);
    splits.into_iter().map(|split| {
      JsValue::from(JsCurve::from(&rc(split)))
    }).collect()
  }
}
