use serde::{Serialize, Serializer, Deserialize, Deserializer};

use shapex::Ref;

use crate::Feature;
use crate::document;


impl Serialize for document::Document {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    Document {
      features: self.features.clone(),
    }.serialize(serializer)
  }
}


impl<'de> Deserialize<'de> for document::Document {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let dummy = Document::deserialize(deserializer)?;
    let mut doc = Self::new();
    doc.features = dummy.features;
    Ok(doc)
  }
}


#[derive(Debug, Serialize, Deserialize)]
struct Document {
  pub features: Vec<Ref<Feature>>,
}


// pub fn export_ron(comp: &crate::Component) -> String {
//   ron::to_string(&dump_component(comp, false)).unwrap()
// }

// pub fn import_ron(dump: String) -> crate::Component {
//   let comp: Component = ron::from_str(&dump).unwrap();
//   undump_component(comp)
// }


// fn undump_component(comp: Component) -> crate::Component {
//   crate::Component {
//     id: Uuid::new_v4(),
//     sketches: comp.sketches.into_iter().map(|sketch|
//       rc(Sketch {
//         elements: sketch.into_iter().map(|elem| rc(elem) ).collect(),
//         ..Default::default()
//       })
//     ).collect(),
//     compound: shapex::Compound {
//       solids: comp.bodies.iter().map(|body|
//         shapex::io::serde::import(body.to_string())
//       ).collect(),
//     },
//     children: comp.children.into_iter().map(|child|
//       undump_component(child)
//     ).collect(),
//     ..Default::default()
//   }
// }

// fn dump_component(comp: &crate::Component, recursive: bool) -> Component {
//   Component {
//     sketches: comp.sketches.iter().map(|sketch|
//       sketch.borrow().elements.iter().map(|elem|
//         elem.borrow().clone()
//       ).collect(),
//     ).collect(),
//     bodies: comp.compound.solids.iter().map(|body|
//       shapex::io::serde::export(body)
//     ).collect(),
//     children: if recursive {
//       comp.children.iter().map(|child|
//         dump_component(&child, true)
//       ).collect()
//     } else {
//       vec![]
//     }
//   }
// }


// #[derive(Debug, Serialize, Deserialize)]
// struct Component {
//   pub sketches: Vec<Vec<CurveType>>,
//   pub bodies: Vec<String>,
//   pub children: Vec<Self>,
// }
