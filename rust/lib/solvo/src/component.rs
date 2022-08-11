use shapex::*;

use crate::internal::*;
use crate::Uuid;
use crate::Sketch;
use crate::ConstructionHelper;
use crate::ConstructionHelperType;
use shapex::DeepClone;


#[derive(Debug, Clone)]
pub struct Component {
  pub id: Uuid,
  pub transform: Matrix4,
  pub sketches: Vec<Ref<Sketch>>,
  pub helpers: Vec<Ref<ConstructionHelper>>,
  pub compound: Compound,
  pub children: Vec<Self>,
}

impl Default for Component {
  fn default() -> Self {
    Self {
      id: Uuid::new_v4(),
      transform: Matrix4::identity(),
      sketches: Default::default(),
      helpers: vec![
        Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
        Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
        Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)),
      ].into_iter().map(|plane|
        rc(ConstructionHelper::new(ConstructionHelperType::Plane(plane)))
      ).collect(),
      compound: Default::default(),
      children: Default::default(),
    }
  }
}

impl Component {
  pub fn find_child(&self, id: &Uuid) -> Option<&Self> {
    if *id == self.id { return Some(self) }
    for child in &self.children {
      if let Some(target) = child.find_child(id) {
        return Some(target)
      }
    }
    None
  }

  pub fn find_child_mut(&mut self, id: &Uuid) -> Option<&mut Self> {
    if *id == self.id { return Some(self) }
    for child in &mut self.children {
      if let Some(target) = child.find_child_mut(id) {
        return Some(target)
      }
    }
    None
  }

  pub fn find_sketch(&self, id: Uuid, recursive: bool) -> Option<&Ref<Sketch>> {
    for sketch in &self.sketches {
      if sketch.borrow().id == id {
        return Some(sketch)
      }
    }
    if recursive {
      for child in &self.children {
        if let Some(sketch) = child.find_sketch(id, true) {
          return Some(sketch)
        }
      }
    }
    None
  }

  pub fn create_component(&mut self) -> &mut Self {
    let comp = Self::default();
    self.children.push(comp);
    self.children.last_mut().unwrap()
  }

  pub fn add_sketch(&mut self, sketch: Ref<Sketch>) {
    self.sketches.push(sketch);
  }

  pub fn deep_clone(&self) -> Self {
    let mut clone = self.clone();
    clone.compound = clone.compound.deep_clone();
    clone
  }
}
