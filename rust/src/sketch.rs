use std::cell::RefCell;
use std::rc::Rc;

use solvo::*;
use shapex::*;

use js_sys::Array;
use wasm_bindgen::prelude::*;

use crate::internal::*;
use crate::curve::JsCurve;
use crate::region::JsRegion;
use crate::utils::matrix_to_js;
use crate::utils::point_from_js;


#[wasm_bindgen]
pub struct JsSketch {
  document: Ref<Document>,
  component_id: Uuid,

  #[wasm_bindgen(skip)]
  pub real: Ref<Sketch>,
}

impl JsSketch {
  pub fn from(document: &Ref<Document>, component_id: CompRef, sketch: &Ref<Sketch>) -> Self {
    JsSketch {
      document: document.clone(),
      component_id,
      real: sketch.clone(),
    }
  }
}

#[wasm_bindgen]
impl JsSketch {

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn component_id(&self) -> JsValue {
    JsValue::from_serde(&self.component_id).unwrap()
  }

  pub fn get_feature_id(&self) -> JsValue {
    JsValue::from_serde(&self.document.borrow().find_feature_from_sketch(&self.real).unwrap().borrow().id).unwrap()
  }

  pub fn get_sketch_elements(&self) -> Array {
    self.real.borrow_mut().elements.iter().map(|elem| {
      JsValue::from(JsCurve::from(elem.clone(), self.real.clone()))
    }).collect()
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsCurve {
    let p1 = point_from_js(p1);
    let p2 = point_from_js(p2);
    let mut line = Line::new(p1, p2);
    let mut sketch = self.real.borrow_mut();
    line.transform(&sketch.work_plane.invert().unwrap());
    sketch.elements.push(Rc::new(RefCell::new(line.into_enum())));
    JsCurve::from(sketch.elements.last().unwrap().clone(), self.real.clone())
  }

  pub fn add_spline(&self, vertices: Array) -> JsCurve {
    let points = vertices.iter().map(|vertex| {
      point_from_js(vertex)
    }).collect();
    let mut spline = Spline::new(points);
    let mut sketch = self.real.borrow_mut();
    spline.transform(&sketch.work_plane.invert().unwrap());
    sketch.elements.push(Rc::new(RefCell::new(CurveType::Spline(spline))));
    JsCurve::from(sketch.elements.last().unwrap().clone(), self.real.clone())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: f64) -> JsCurve {
    let mut sketch = self.real.borrow_mut();
    let center = point_from_js(center);
    let circle = Circle::new(sketch.work_plane.transform_point(center), radius);
    sketch.elements.push(rc(CurveType::Circle(circle)));
    JsCurve::from(sketch.elements.last().unwrap().clone(), self.real.clone())
  }

  pub fn add_arc(&mut self, p1: JsValue, p2: JsValue, p3: JsValue) -> Result<JsCurve, JsValue> {
    let points = vec![point_from_js(p1), point_from_js(p2), point_from_js(p3)];
    let mut sketch = self.real.borrow_mut();
    let transform = sketch.work_plane.invert().unwrap();
    let points: Vec<Point3> = points.into_iter().map(|p| transform.transform_point(p) ).collect();
    let arc = Arc::from_points(points[0], points[1], points[2])?;
    sketch.elements.push(rc(arc.into_enum()));
    Ok(JsCurve::from(sketch.elements.last().unwrap().clone(), self.real.clone()))
  }

  pub fn get_workplane(&self) -> JsValue {
    let plane = self.real.borrow_mut().work_plane;
    matrix_to_js(plane)
  }

  pub fn get_profiles(&self) -> Array {
    // web_sys::console::time_with_label("get_profiles");
    let profiles = self.real.borrow_mut().get_profiles(false);
    // web_sys::console::time_end_with_label("get_profiles");
    profiles.into_iter().map(|profile| JsValue::from(JsRegion::new(
      profile,
      self.real.clone(),
      self.document.clone(),
    ))).collect()
  }

  pub fn get_all_split(&self) {
    let mut sketch = self.real.borrow_mut();

    let planar_elements = &sketch.elements;
    let splits: Vec<TrimmedCurve> = Sketch::all_split(&planar_elements);

    let (mut circles, mut others): (Vec<TrimmedCurve>, Vec<TrimmedCurve>) = splits.into_iter().partition(|elem| match elem.base {
      CurveType::Circle(_) => true,
      _ => false,
    });

    Sketch::remove_dangling_segments(&mut others);
    others.append(&mut circles);

    for tcurve in &mut others {
      tcurve.transform(&sketch.work_plane);
    }

    sketch.elements.clear();
    for split in others.iter() {
      sketch.elements.push(Rc::new(RefCell::new(split.cache.clone())));
    }
  }
}
