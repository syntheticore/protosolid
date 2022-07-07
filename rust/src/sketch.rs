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
      JsValue::from(JsCurve::from(elem))
    }).collect()
  }

  pub fn add_line(&mut self, p1: JsValue, p2: JsValue) -> JsCurve {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let line = Line::new(Point3::from(p1), Point3::from(p2));
    let mut sketch = self.real.borrow_mut();
    sketch.elements.push(Rc::new(RefCell::new(line.into_enum())));
    JsCurve::from(&sketch.elements.last().unwrap())
  }

  pub fn add_spline(&self, vertices: Array) -> JsCurve {
    let points = vertices.iter().map(|vertex| {
      let vertex: (f64, f64, f64) = vertex.into_serde().unwrap();
      Point3::new(vertex.0, vertex.1, vertex.2)
    }).collect();
    let spline = BezierSpline::new(points);
    let mut sketch = self.real.borrow_mut();
    sketch.elements.push(Rc::new(RefCell::new(CurveType::BezierSpline(spline))));
    JsCurve::from(&sketch.elements.last().unwrap())
  }

  pub fn add_circle(&mut self, center: JsValue, radius: f64) -> JsCurve {
    let mut sketch = self.real.borrow_mut();
    let center: (f64, f64, f64) = center.into_serde().unwrap();
    let mut circle = Circle::new(Point3::from(center), radius);
    circle.normal = sketch.work_plane.transform_vector(Vec3::new(0.0, 0.0, 1.0));
    sketch.elements.push(rc(CurveType::Circle(circle)));
    JsCurve::from(&sketch.elements.last().unwrap())
  }

  pub fn add_arc(&mut self, p1: JsValue, p2: JsValue, p3: JsValue) -> Result<JsCurve, JsValue> {
    let p1: (f64, f64, f64) = p1.into_serde().unwrap();
    let p2: (f64, f64, f64) = p2.into_serde().unwrap();
    let p3: (f64, f64, f64) = p3.into_serde().unwrap();
    let arc = Arc::from_points(Point3::from(p1), Point3::from(p2), Point3::from(p3))?;
    let mut sketch = self.real.borrow_mut();
    sketch.elements.push(rc(arc.into_enum()));
    Ok(JsCurve::from(&sketch.elements.last().unwrap()))
  }

  pub fn remove_element(&mut self, id: JsValue) {
    let id: uuid::Uuid = id.into_serde().unwrap();
    let mut sketch = self.real.borrow_mut();
    sketch.elements.retain(|elem| as_controllable(&mut elem.borrow_mut()).id() != id );
  }

  pub fn get_workplane(&self) -> JsValue {
    let plane = self.real.borrow_mut().work_plane;
    matrix_to_js(plane)
  }

  pub fn set_workplane(&self, plane: JsValue) {
    let plane = matrix_from_js(plane);
    self.real.borrow_mut().work_plane = plane;
  }

  pub fn get_profiles(&self) -> Array {
    web_sys::console::time_with_label("get_profiles");
    let profiles = self.real.borrow_mut().get_profiles(false);
    web_sys::console::time_end_with_label("get_profiles");
    profiles.into_iter().map(|profile| JsValue::from(JsRegion::new(
      profile,
      self.real.borrow().work_plane,
      self.real.clone(),
    ))).collect()
  }

  pub fn get_all_split(&self) {
    let mut sketch = self.real.borrow_mut();

    let planar_elements = sketch.get_planarized_elements();
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
