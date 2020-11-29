use std::ptr;
use std::rc::Rc;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashSet;

use uuid::Uuid;
use cgmath::prelude::*;

pub use shapex::*;


#[derive(Debug, Default)]
pub struct Component {
  pub id: Uuid,
  pub title: String,
  // pub visible: bool,
  pub sketch: Sketch,
  pub bodies: Vec<Solid>,
  pub children: Vec<Rc<RefCell<Component>>>,
}

impl Component {
  pub fn new() -> Self {
    let mut this: Self = Default::default();
    this.id = Uuid::new_v4();
    this
  }
}


#[derive(Debug, Clone)]
pub struct DerivedSketchElement {
  pub owned: SketchElement,
  pub original: Rc<RefCell<SketchElement>>,
}


type DerivedRegion = Vec<DerivedSketchElement>;
pub type Region = Vec<TrimmedSketchElement>;


#[derive(Debug, Default)]
pub struct Sketch {
  pub elements: Vec<Rc<RefCell<SketchElement>>>,
}

impl Sketch {
  pub fn new() -> Self {
    Default::default()
  }

  pub fn get_regions(&self) -> Vec<Region> {
    let regions = self.build_regions(false);
    regions.into_iter().map({|region|
      region.into_iter().map({|derived|
        TrimmedSketchElement {
          base: derived.original.borrow().clone(),
          bounds: derived.owned.as_curve().endpoints(),
          cache: derived.owned,
        }
      }).collect()
    }).collect()
  }

  fn build_regions(&self, include_outer: bool) -> Vec<DerivedRegion> {
    let mut cut_elements = Self::all_split(&self.elements);
    Self::remove_dangling_segments(&mut cut_elements);
    let islands = Self::build_islands(&cut_elements);
    islands.iter().flat_map(|island| Self::build_loops_from_island(island, include_outer) ).collect()
  }

  fn build_loops_from_island(island: &Vec<DerivedSketchElement>, include_outer: bool) -> Vec<DerivedRegion> {
    let mut regions = vec![];
    let mut used_forward = HashSet::new();
    let mut used_backward = HashSet::new();
    for i in 0..2 {
      for start_elem in island.iter() {
        let points = start_elem.owned.as_curve().endpoints();
        let start_point = if i == 0 { points.0 } else { points.1 };
        let mut loops = Self::build_loop(&start_point, &start_elem, vec![], &start_point, island, &mut used_forward, &mut used_backward);
        regions.append(&mut loops);
      }
    }
    if !include_outer { Self::remove_outer_loop(&mut regions) }
    regions
  }

  pub fn split_all(&self) -> Vec<SketchElement> {
    Self::all_split(&self.elements).into_iter()
    .map(|elem| elem.owned )
    .collect()
  }

  pub fn all_split(elements: &Vec<Rc<RefCell<SketchElement>>>) -> Vec<DerivedSketchElement> {
    elements.iter().flat_map(|elem| {
      Self::split_element(elem.clone(), &elements)
    }).collect()
  }

  fn split_element(elem: Rc<RefCell<SketchElement>>, others: &Vec<Rc<RefCell<SketchElement>>>) -> Vec<DerivedSketchElement> {
    let mut segments = vec![DerivedSketchElement {
      original: elem.clone(),
      owned: elem.borrow().clone(),
    }];
    for other in others.iter() {
      if ptr::eq(&*elem.borrow(), &*other.borrow()) { continue }
      segments = segments.iter().flat_map(|own| {
        let splits = own.owned.split(&other.borrow());
        splits.into_iter().map(move |split| DerivedSketchElement {
          owned: split,
          original: own.original.clone(),
        })
      }).collect();
    }
    segments
  }

  pub fn build_islands(elements: &Vec<DerivedSketchElement>) -> Vec<Vec<DerivedSketchElement>> {
    let mut unused_elements = elements.clone();
    let mut islands = vec![];
    while let Some(start_elem) = unused_elements.pop() {
      let mut island = vec![];
      Self::build_island(&start_elem, &mut island, &unused_elements);
      for island_elem in island.iter() {
        // unused_elements.retain(|elem| !ptr::eq(elem, island_elem));
        let island_elem_id = as_controllable(&island_elem.owned).id();
        unused_elements.retain(|elem| as_controllable(&elem.owned).id() != island_elem_id );
      }
      if island.len() > 0 { islands.push(island) }
    }
    islands
  }

  fn build_island(start_elem: &DerivedSketchElement, mut path: &mut Vec<DerivedSketchElement>, all_elements: &Vec<DerivedSketchElement>) {
    if path.iter().any(|e| e.owned == start_elem.owned ) { return }
    let (start_point, end_point) = start_elem.owned.as_curve().endpoints();
    path.push(start_elem.clone());
    for elem in all_elements.iter() {
      let (other_start, other_end) = elem.owned.as_curve().endpoints();
      // We are connected to other element
      if end_point.almost(other_start) || end_point.almost(other_end) || start_point.almost(other_start) || start_point.almost(other_end) {
        Self::build_island(&elem, &mut path, all_elements);
      }
    }
  }

  pub fn build_loop<'a>(
    start_point: &Point3,
    start_elem: &'a DerivedSketchElement,
    mut path: DerivedRegion,
    path_start_point: &Point3,
    all_elements: &'a Vec<DerivedSketchElement>,
    used_forward: &mut HashSet<Uuid>,
    used_backward: &mut HashSet<Uuid>,
  ) -> Vec<DerivedRegion> {
    let mut regions = vec![];
    // Traverse edges only once in every direction
    let start_elem_id = as_controllable(&start_elem.owned).id();
    if start_point.almost(start_elem.owned.as_curve().endpoints().0) {
      if used_forward.contains(&start_elem_id) { return regions }
      used_forward.insert(start_elem_id);
    } else {
      if used_backward.contains(&start_elem_id) { return regions }
      used_backward.insert(start_elem_id);
    }
    // Add start_elem to path
    path.push(start_elem.clone());
    // Find connected segments
    let end_point = start_elem.owned.as_curve().other_endpoint(&start_point);
    let mut connected_elems: Vec<&DerivedSketchElement> = all_elements.iter().filter(|other_elem| {
      let (other_start, other_end) = other_elem.owned.as_curve().endpoints();
      (end_point.almost(other_start) || end_point.almost(other_end)) && as_controllable(&other_elem.owned).id() != start_elem_id
    }).collect();
    if connected_elems.len() > 0 {
      // Sort connected segments in clockwise order
      connected_elems.sort_by(|a, b| {
        let final_point_a = a.owned.as_curve().other_endpoint(&end_point);
        let final_point_b = b.owned.as_curve().other_endpoint(&end_point);
        if geom2d::clockwise(*start_point, end_point, final_point_a) > geom2d::clockwise(*start_point, end_point, final_point_b) {
          Ordering::Less
        } else {
          Ordering:: Greater
        }
        // if geom2d::clockwise(end_point, final_point_a, final_point_b) < 0.0 { Ordering::Less } else { Ordering:: Greater }
      });
      // Follow the leftmost segment to complete loop in anti-clockwise order
      let next_elem = connected_elems[0];
      // if path[0].owned == next_elem.owned {
      // if path_start_point.almost(end_point) {
      if as_controllable(&path[0].owned).id() == as_controllable(&next_elem.owned).id() {
        // We are closing a loop
        regions.push(path);
      } else {
        // Follow loop
        let mut new_regions = Self::build_loop(&end_point, &next_elem, path, path_start_point, all_elements, used_forward, used_backward);
        regions.append(&mut new_regions);
      }
      // // Begin a fresh loop for all other forks
      // for &connected_elem in connected_elems.iter().skip(1) {
      //   let mut new_regions = Self::build_loop(&end_point, &connected_elem, vec![], &end_point, all_elements, used_forward, used_backward);
      //   regions.append(&mut new_regions);
      // }
    }
    regions
  }

  fn poly_from_region(loopy: &DerivedRegion) -> PolyLine {
    geom2d::poly_from_wire(loopy.iter().map(|elem| elem.owned.clone() ).collect())
  }

  fn remove_outer_loop(loops: &mut Vec<DerivedRegion>) {
    if loops.len() <= 1 { return }
    loops.retain(|loopy| {
      geom2d::is_clockwise(&Self::poly_from_region(loopy))
    })
  }

  pub fn remove_dangling_segments(island: &mut Vec<DerivedSketchElement>) {
    let others = island.clone();
    let start_len = island.len();
    island.retain(|elem| {
      if elem.owned.as_curve().length().almost(0.0) { return false }
      let elem_id = as_controllable(&elem.owned).id();
      let (start_point, end_point) = elem.owned.as_curve().endpoints();
      [start_point, end_point].iter().all(|endpoint| {
        others.iter().any(|other_elem| {
          let (other_start, other_end) = other_elem.owned.as_curve().endpoints();
          (endpoint.almost(other_start) || endpoint.almost(other_end))
          && as_controllable(&other_elem.owned).id() != elem_id
        })
      })
    });
    if island.len() < start_len { Self::remove_dangling_segments(island) }
  }
}


pub struct Scene {
  pub tree: Rc<RefCell<Component>>,
  pub current_component: Rc<RefCell<Component>>,
}

impl Scene {
  pub fn new() -> Self {
    let mut comp = Component::new();
    comp.title = "Main Assembly".to_string();
    // comp.visible = true;
    let tree = Rc::new(RefCell::new(comp));
    let current_component = Rc::clone(&tree);
    Self { tree, current_component }
  }

  pub fn create_component(&mut self) -> Rc<RefCell<Component>> {
    let mut comp: Component = Default::default();
    comp.title = "New Component".to_string();
    // comp.visible = true;
    let comp = Rc::new(RefCell::new(comp));
    {
      let mut current_component = self.current_component.borrow_mut();
      current_component.children.push(Rc::clone(&comp));
    }
    self.current_component = Rc::clone(&comp);
    comp
  }

  // pub fn create_sketch(&mut self) -> Rc<RefCell<Sketch>> {
  //   let mut sketch: Sketch = Sketch::new();
  //   sketch.title = "Sketch1".to_string();
  //   sketch.visible = true;
  //   let sketch = Rc::new(RefCell::new(sketch));
  //   self.current_component.borrow_mut().sketches.push(Rc::clone(&sketch));
  //   sketch
  // }

  pub fn activate(&mut self, comp: Rc<RefCell<Component>>) {
    self.current_component = comp;
  }

  // pub fn edit_sketch(&mut self, sketch: &Rc<RefCell<Sketch>>) {
  //   self.current_sketch = Some(Rc::clone(sketch));
  // }
}


trait Constraint {}


pub trait Controllable {
  fn id(&self) -> Uuid;
  fn get_handles(&self) -> Vec<Point3>;
  fn set_handles(&mut self, _: Vec<Point3>);
  fn get_snap_points(&self) -> Vec<Point3>;
}

impl Controllable for Line {
  fn id(&self) -> Uuid {
    self.id
  }

  fn get_handles(&self) -> Vec<Point3> {
    vec![self.points.0, self.points.1]
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.points = (handles[0], handles[1]);
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    let mut points = self.get_handles();
    points.push(self.midpoint());
    points
  }
}

impl Controllable for Arc {
  fn id(&self) -> Uuid {
    self.id
  }

  fn get_handles(&self) -> Vec<Point3> {
    vec![self.center]
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.center = handles[0];
    self.radius = handles[0].distance(handles[1]);
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    vec![self.center]
  }
}


impl Controllable for Circle {
  fn id(&self) -> Uuid {
    self.id
  }

  fn get_handles(&self) -> Vec<Point3> {
    vec![self.center]
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.center = handles[0];
    if handles.len() > 1 {
      let p = handles[1];
      self.radius = handles[0].distance(p);
    }
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    vec![self.center]
  }
}


impl Controllable for BezierSpline {
  fn id(&self) -> Uuid {
    self.id
  }

  fn get_handles(&self) -> Vec<Point3> {
    self.vertices.clone()
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.vertices = handles;
    self.update();
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    self.get_handles()
  }
}


pub fn as_controllable(elem: &SketchElement) -> &dyn Controllable {
  match elem {
    SketchElement::Line(line) => line,
    SketchElement::Arc(arc) => arc,
    SketchElement::Circle(circle) => circle,
    SketchElement::BezierSpline(spline) => spline,
  }
}

pub fn as_controllable_mut(elem: &mut SketchElement) -> &mut dyn Controllable {
  match elem {
    SketchElement::Line(line) => line,
    SketchElement::Arc(arc) => arc,
    SketchElement::Circle(circle) => circle,
    SketchElement::BezierSpline(spline) => spline,
  }
}


#[cfg(test)]
mod tests {
  use super::*;
  use shapex::test_data;

  fn make_sketch(lines: &Vec<Line>) -> Sketch {
    let mut sketch = Sketch::new();
    for line in lines {
      sketch.elements.push(Rc::new(RefCell::new(SketchElement::Line(line.clone()))));
    }
    sketch
  }

  #[test]
  fn split_all_crossing() {
    let sketch = make_sketch(&test_data::crossing_lines());
    let segments = sketch.split_all();
    assert_eq!(segments.len(), 4, "{} segments found instead of 4", segments.len());
    assert_eq!(segments[0].as_curve().length(), 0.5, "Segment had wrong length");
    assert_eq!(segments[1].as_curve().length(), 0.5, "Segment had wrong length");
    assert_eq!(segments[2].as_curve().length(), 0.5, "Segment had wrong length");
    assert_eq!(segments[3].as_curve().length(), 0.5, "Segment had wrong length");
  }

  #[test]
  fn split_all_parallel() {
    let sketch = make_sketch(&test_data::parallel_lines());
    let segments = sketch.split_all();
    assert_eq!(segments.len(), 2, "{} segments found instead of 2", segments.len());
    assert_eq!(segments[0].as_curve().length(), 1.0, "Segment had wrong length");
    assert_eq!(segments[1].as_curve().length(), 1.0, "Segment had wrong length");
  }

  #[test]
  fn t_split() {
    let sketch = make_sketch(&test_data::t_section());
    let segments = sketch.split_all();
    assert_eq!(segments.len(), 3, "{} segments found instead of 3", segments.len());
    assert_eq!(segments[0].as_curve().length(), 1.0, "Segment had wrong length");
    assert_eq!(segments[1].as_curve().length(), 1.0, "Segment had wrong length");
    assert_eq!(segments[2].as_curve().length(), 1.0, "Segment had wrong length");
  }

  #[test]
  fn region_rect() {
    let sketch = make_sketch(&test_data::rectangle());
    let cut_elements = Sketch::all_split(&sketch.elements);
    let islands = Sketch::build_islands(&cut_elements);
    let regions = sketch.get_regions();
    assert_eq!(cut_elements.len(), 4, "{} cut_elements found instead of 4", cut_elements.len());
    assert_eq!(islands.len(), 1, "{} islands found instead of 1", islands.len());
    assert_eq!(regions.len(), 1, "{} regions found instead of 1", regions.len());
  }

  #[test]
  fn region_crossing_rect() {
    let mut sketch = make_sketch(&test_data::crossing_rectangle());
    let cut_elements = Sketch::all_split(&sketch.elements);
    let islands = Sketch::build_islands(&cut_elements);
    let regions = sketch.get_regions();
    sketch.elements.clear();
    for split in cut_elements.iter() {
      sketch.elements.push(Rc::new(RefCell::new(split.owned.clone())));
    }
    assert_eq!(cut_elements.len(), 8, "{} cut_elements found instead of 8", cut_elements.len());
    assert_eq!(islands.len(), 1, "{} islands found instead of 1", islands.len());
    assert_eq!(regions.len(), 1, "{} regions found instead of 1", regions.len());
  }

  #[test]
  fn region_crossing_corner() {
    let mut lines = test_data::rectangle();
    lines[2].points.1.x = -2.0;
    lines[3].points.0.y = -2.0;
    let sketch = make_sketch(&lines);
    let cut_elements = Sketch::all_split(&sketch.elements);
    let islands = Sketch::build_islands(&cut_elements);
    let regions = sketch.get_regions();
    assert_eq!(cut_elements.len(), 6, "{} cut_elements found instead of 6", cut_elements.len());
    assert_eq!(islands.len(), 1, "{} islands found instead of 1", islands.len());
    assert_eq!(regions.len(), 1, "{} regions found instead of 1", regions.len());
  }

  #[test]
  fn dangling_segment() {
    let mut sketch = Sketch::new();
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0));
    sketch.elements.push(Rc::new(RefCell::new(SketchElement::Line(line))));
    let _regions = sketch.get_regions();
  }
}


// pub struct VertexIterator<'a> {
//   elem_iter: std::slice::Iter<'a, PolyLine>,
//   vertex_iter: Option<std::slice::Iter<'a, Point3>>,
// }

// impl<'a> VertexIterator<'a> {
//   pub fn new(sketch: &'a Sketch) -> Self {
//     Self {
//       elem_iter: sketch.elements.iter(),
//       vertex_iter: None
//     }
//   }
// }

// impl<'a> Iterator for VertexIterator<'a> {
//   type Item = &'a Point3;

//   fn next(&mut self) -> Option<&'a Point3> {
//     if let Some(ref mut vertex_iter) = self.vertex_iter {
//       let vertex = vertex_iter.next();
//       if vertex.is_some() {
//         vertex
//       } else {
//         self.vertex_iter = None;
//         self.next()
//       }
//     } else {
//       if let Some(line) = self.elem_iter.next() {
//         self.vertex_iter = Some(line.vertices.iter());
//         self.next()
//       } else {
//         None
//       }
//     }
//   }
// }

// #[derive(Debug)]
// pub struct Sketch {
//   pub title: String,
//   pub plane: Plane,
//   // pub elements: Vec<PolyLine>,
//   pub elements: Vec<Rc<RefCell<dyn SketchElement>>>,
//   // pub constraints: Vec<Box<Constraint>>
//   pub visible: bool
// }

// impl Sketch {
//   pub fn new() -> Self {
//     Self {
//       title: "Sketch1".to_string(),
//       plane: Plane::new(),
//       elements: vec![],
//       // constraints: vec![]
//       visible: true
//     }
//   }

//   // pub fn all_vertices(&self) -> VertexIterator {
//   //   VertexIterator::new(self)
//   // }
// }
