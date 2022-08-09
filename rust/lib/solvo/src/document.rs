use std::rc::Rc;

use uuid::{uuid, Uuid};

use shapex::Ref;
use shapex::Repairable;

use crate::internal::*;
use crate::Component;
use crate::Feature;
use crate::FeatureType;
use crate::Sketch;
use crate::CompRef;
use crate::FeatureError;


#[derive(Debug)]
pub struct Document {
  pub features: Vec<Ref<Feature>>,
  cache: Vec<Component>,
  marker: usize,
  last_change_index: usize,
  last_eval_index: usize,
  removal_modifications: Vec<CompRef>,
}

impl Document {
  pub fn new() -> Self {
    let mut base_comp = Component::default();
    base_comp.id = uuid!("00000000-0000-0000-0000-000000000000");
    Self {
      features: vec![],
      cache: vec![base_comp],
      marker: 0,
      last_change_index: 0,
      last_eval_index: 0,
      removal_modifications: vec![],
    }
  }

  pub fn get_tree(&self) -> &Component {
    &self.cache[self.marker]
  }

  pub fn get_tree_mut(&mut self) -> &mut Component {
    &mut self.cache[self.marker]
  }

  pub fn get_marker(&self) -> usize { self.marker }

  pub fn move_marker(&mut self, to: usize) {
    self.marker = to.min(self.features.len()).max(0);
  }

  pub fn move_marker_to_feature(&mut self, feature: &Ref<Feature>) {
    self.marker = self.find_feature_index(feature) + 1;
  }

  pub fn add_feature(&mut self, feature: Ref<Feature>) {
    self.features.insert(self.marker, feature);
    self.last_change_index = self.last_change_index.min(self.marker);
    self.marker += 1;
  }

  pub fn invalidate_feature(&mut self, feature: &Ref<Feature>) {
    self.last_change_index = self.find_feature_index(feature);
    log!("Invalidate {:#?}", self.last_change_index);
  }

  pub fn repair_feature(&mut self, feature: &Ref<Feature>) {
    let index = self.find_feature_index(feature);
    let comp = &self.cache[index];
    let mut f = feature.borrow_mut();
    f.feature_type.as_feature_mut().repair(comp);
    self.invalidate_feature(feature);
  }

  pub fn remove_feature(&mut self, feature: &Ref<Feature>) {
    self.removal_modifications.append(
      &mut feature.borrow().feature_type.as_feature().modified_components()
    );
    let index = self.find_feature_index(feature);
    self.features.remove(index);
    if self.marker > index {
      self.marker -= 1;
      self.last_eval_index -= 1;
    }
    self.last_change_index = self.last_change_index.min(index);
  }

  pub fn evaluate(&mut self) -> Vec<CompRef> {
    log!("evaluate {:#?}", self.last_change_index);
    let last_change = self.last_change_index;
    self.regenerate(last_change.min(self.marker), self.marker);
    let (from, to) = sort_tuple2(self.last_eval_index, self.marker);
    self.last_eval_index = self.marker;
    self.components_modified(from.min(last_change), to)
  }

  fn regenerate(&mut self, from: usize, to: usize) {
    self.cache.resize(self.features.len() + 1, Component::default());
    let mut comp = &self.cache[from];
    for (i, feature) in self.features.iter_mut().enumerate().skip(from).take(to - from) {
      let mut new_comp = comp.deep_clone();
      let mut feature = feature.borrow_mut();
      feature.error = feature.feature_type.as_feature_mut().execute(&mut new_comp).err();
      let j = i + 1;
      self.cache[j] = if let Some(FeatureError::Error(_)) = feature.error {
        comp.deep_clone()
      } else {
        let repair_error = feature.feature_type.as_feature().modified_components().iter()
          .find_map(|id| new_comp.find_child_mut(id).unwrap().compound.repair().err() )
          .map(|error| FeatureError::Error(error) );
        if repair_error.is_some() {
          feature.error = repair_error;
          comp.deep_clone()
        } else {
          new_comp
        }
      };
      comp = &self.cache[j];
      self.last_change_index = j;
    }
  }

  fn components_modified(&mut self, from: usize, to: usize) -> Vec<CompRef> {
    log!("comps modified range {:#?} {:#?}", from, to);
    // Find unique ids of modified components in given range
    let mut comp_ids = self.features.iter().skip(from).take(to - from).map(|f|
      f.borrow().feature_type.as_feature().modified_components()
    ).collect::<Vec<Vec<CompRef>>>().concat();
    comp_ids.append(&mut self.removal_modifications);
    comp_ids.sort_unstable();
    comp_ids.dedup();
    // Filter children whose parents are already part of the set
    let comps: Vec<&Component> = comp_ids.iter().map(|id|
      self.cache[to].find_child(id).unwrap()
    ).collect();
    comp_ids.retain(|id|
      !comps.iter().any(|comp| Self::has_child(comp, id) )
    );
    comp_ids
  }

  fn find_feature_index(&self, feature: &Ref<Feature>) -> usize {
    self.features.iter().position(|f| Rc::ptr_eq(f, feature) ).unwrap()
  }

  fn has_child(comp: &Component, child_id: &CompRef) -> bool {
    for child in &comp.children {
      if child.id == *child_id {
        return true
      }
    }
    false
  }

  pub fn find_feature(&self, id: Uuid) -> Option<&Ref<Feature>> {
    for feature in self.features.iter() {
      if feature.borrow().id == id {
        return Some(feature)
      }
    }
    None
  }

  pub fn find_feature_from_sketch(&self, sketch: &Ref<Sketch>) -> Option<&Ref<Feature>> {
    self.features.iter().find(|feature| {
      match &feature.borrow().feature_type {
        FeatureType::CreateSketch(sketch_feature) => Rc::ptr_eq(sketch, &sketch_feature.sketch),
        _ => false,
      }
    })
  }
}
