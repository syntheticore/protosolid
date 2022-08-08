use js_sys::Array;
use wasm_bindgen::prelude::*;

use solvo::*;
use shapex::*;

use crate::solid::JsSolid;
use crate::sketch::JsSketch;
use crate::construction_helper::JsConstructionHelper;


#[wasm_bindgen]
pub struct JsComponent {
  #[wasm_bindgen(skip)]
  pub component_id: Uuid,

  #[wasm_bindgen(skip)]
  pub document: Ref<Document>,
}

#[wasm_bindgen]
impl JsComponent {
  fn get_comp<'a>(&'a self, doc: &'a Document) -> &Component {
    doc.get_tree().find_child(&self.component_id).unwrap()
  }

  pub fn id(&self) -> JsValue {
    let doc = self.document.borrow();
    JsValue::from_serde(&self.get_comp(&doc).id).unwrap()
  }

  pub fn get_children(&self) -> Array {
    self.get_comp(&self.document.borrow()).children.iter().map(|child|
      JsValue::from(Self {
        component_id: child.id,
        document: self.document.clone(),
      })
    ).collect()
  }

  pub fn get_sketches(&self) -> Array {
    self.get_comp(&self.document.borrow()).sketches.iter().map(|sketch|
      JsValue::from(JsSketch::from(&self.document, self.component_id, sketch))
    ).collect()
  }

  pub fn get_solids(&self) -> Array {
    let doc = self.document.borrow();
    self.get_comp(&doc).compound.solids.iter().map(|body|
      JsValue::from(JsSolid::from(body, self.component_id, self.document.clone()))
    ).collect()
  }

  pub fn get_planes(&self) -> Array {
    self.get_comp(&self.document.borrow()).helpers.iter().filter_map(|helper|
      if let ConstructionHelperType::Plane(_) = &helper.borrow().helper_type {
        // Some(matrix_to_js(plane.as_transform()))
        Some(JsValue::from(JsConstructionHelper::new(helper, self.document.clone())))
      } else {
        None
      }
    ).collect()
  }

  pub fn export_stl(&self, title: &str) -> String {
    let doc = self.document.borrow();
    let comp = self.get_comp(&doc);
    let mesh = comp.compound.solids[0].tesselate();
    shapex::io::stl::export(&mesh, title)
  }

  pub fn export_3mf(&self) -> String {
    let meshes = Self::tesselate_all(&self.get_comp(&self.document.borrow()));
    shapex::io::threemf::export(&meshes, "millimeter")
  }

  fn tesselate_all(comp: &Component) -> Vec<Mesh> {
    let mut meshes: Vec<Mesh> = comp.compound.solids.iter().map(|body| body.tesselate() ).collect();
    for child in &comp.children {
      meshes.append(&mut Self::tesselate_all(&child));
    }
    meshes
  }

  // pub fn make_cube(&self) {
  //   let cube = features::make_cube(10.0, 10.0, 10.0).unwrap();
  //   self.get_comp(&self.document.borrow()).compound.create(cube.into_compound());
  // }

  // pub fn make_cylinder(&self) {
  //   let cube = features::make_cylinder(30.0, 50.0).unwrap();
  //   self.get_comp(&self.document.borrow()).compound.create(cube.into_compound());
  // }
}
