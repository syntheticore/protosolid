use std::rc::Rc;
use std::cell::RefCell;

use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::sketch::JsSketch;
use crate::solid::JsSolid;

use crate::log;

#[wasm_bindgen]
#[derive(Default)]
pub struct JsComponent {
  real: Ref<Component>,
}

#[wasm_bindgen]
impl JsComponent {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Self {
    let tree = Component::new();
    Self { real: rc(tree) }
  }

  fn from(comp: &Rc<RefCell<Component>>) -> Self {
    Self {
      real: comp.clone(),
    }
  }

  pub fn id(&self) -> JsValue {
    JsValue::from_serde(&self.real.borrow().id).unwrap()
  }

  pub fn get_sketch(&self) -> JsSketch {
    JsSketch::from(&self.real)
  }

  // pub fn get_children(&self) -> Array {
  //   self.real.borrow().children.iter().map(|child|
  //     JsValue::from(Self::from(child))
  //   ).collect()
  // }

  pub fn get_solids(&self) -> Array {
    self.real.borrow().compound.solids.iter().map(|body|
      JsValue::from(JsSolid::from(body, self.real.clone()))
    ).collect()
  }

  pub fn create_component(&mut self) -> Self {
    let comp = self.real.borrow_mut().create_component();
    Self::from(&comp)
  }

  pub fn delete_component(&self, comp: Self) {
    self.real.borrow_mut().delete_component(&comp.real)
  }

  pub fn export_stl(&self, title: &str) -> String {
    let comp = self.real.borrow();
    let mesh = comp.compound.solids[0].tesselate();
    log!("{:?}", mesh);
    shapex::io::stl::export(&mesh, title)
  }

  pub fn export_3mf(&self) -> String {
    let meshes = Self::tesselate_all(&self.real);
    shapex::io::threemf::export(&meshes, "millimeter")
  }

  fn tesselate_all(comp: &Ref<Component>) -> Vec<Mesh> {
    let comp = comp.borrow();
    let mut meshes: Vec<Mesh> = comp.compound.solids.iter().map(|body| body.tesselate() ).collect();
    for child in &comp.children {
      meshes.append(&mut Self::tesselate_all(&child));
    }
    meshes
  }

  pub fn serialize(&self) -> String {
    let comp = self.real.borrow();
    solvo::io::export_ron(&comp)
  }

  pub fn unserialize(&mut self, dump: String) {
    self.real = rc(solvo::io::import_ron(dump));
  }

  pub fn make_cube(&self) {
    let cube = features::make_cube(10.0, 10.0, 10.0).unwrap();
    self.real.borrow_mut().compound.add(cube.into_compound());
  }
}
