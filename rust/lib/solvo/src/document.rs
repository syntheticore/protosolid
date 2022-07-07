use std::rc::Rc;

use shapex::*;

use crate::Component;
use crate::Feature;
use crate::FeatureType;
use crate::Sketch;
use crate::CompRef;

// use crate::log;


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
    Self {
      features: vec![],
      cache: vec![Component::default()],
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

  // pub fn find_feature(&mut self, id: Uuid) -> Option<&mut Feature> {
  //   for feature in self.features.iter_mut() {
  //     if feature.id == id {
  //       return Some(feature)
  //     }
  //   }
  //   None
  // }

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
    log!("Invalidate {:#?}", self.last_change_index)
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
    let (from, to) = Self::ordered(self.last_eval_index, self.marker);
    self.last_eval_index = self.marker;
    self.components_modified(from.min(last_change), to)
  }

  fn regenerate(&mut self, from: usize, to: usize) {
    self.cache.resize(self.features.len() + 1, Component::default());
    let mut comp = &self.cache[from];
    for (i, feature) in self.features.iter().enumerate().skip(from).take(to - from) {
      let mut new_comp = comp.deep_clone();
      let mut feature = feature.borrow_mut();
      if let Err(error) = feature.feature_type.as_feature().execute(&mut new_comp) {
        feature.error = Some(error);
      }
      let j = i + 1;
      self.cache[j] = new_comp;
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

  fn ordered(from: usize, to: usize) -> (usize, usize) {
    if from <= to {
      (from, to)
    } else {
      (to, from)
    }
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