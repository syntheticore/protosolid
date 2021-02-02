use uuid::Uuid;
use serde::{Serialize, Deserialize};

use shapex::rc;
use shapex::CurveType;

use crate::Sketch;


pub fn export_ron(comp: &crate::Component) -> String {
  ron::to_string(&dump_component(comp, false)).unwrap()
}

pub fn import_ron(dump: String) -> crate::Component {
  let comp: Component = ron::from_str(&dump).unwrap();
  undump_component(comp)
}


fn undump_component(comp: Component) -> crate::Component {
  crate::Component {
    id: Uuid::new_v4(),
    sketch: Sketch {
      elements: comp.sketch_elements.into_iter().map(|elem|
        rc(elem)
      ).collect(),
    },
    bodies: comp.bodies.iter().map(|body|
      shapex::io::serde::import(body.to_string())
    ).collect(),
    children: comp.children.into_iter().map(|child|
      rc(undump_component(child))
    ).collect(),
  }
}

fn dump_component(comp: &crate::Component, recursive: bool) -> Component {
  Component {
    sketch_elements: comp.sketch.elements.iter().map(|elem|
      elem.borrow().clone()
    ).collect(),
    bodies: comp.bodies.iter().map(|body|
      shapex::io::serde::export(body)
    ).collect(),
    children: if recursive {
      comp.children.iter().map(|child|
        dump_component(&child.borrow(), true)
      ).collect()
    } else {
      vec![]
    }
  }
}


#[derive(Debug, Serialize, Deserialize)]
struct Component {
  pub sketch_elements: Vec<CurveType>,
  pub bodies: Vec<String>,
  pub children: Vec<Self>,
}
