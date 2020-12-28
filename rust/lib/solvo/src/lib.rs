use std::convert::TryInto;
use std::cmp::Ordering;
use std::collections::HashSet;

use uuid::Uuid;

pub use shapex::*;


#[derive(Debug, Default)]
pub struct Component {
  pub id: Uuid,
  pub title: String,
  // pub visible: bool,
  pub sketch: Sketch,
  pub bodies: Vec<Solid>,
  pub children: Vec<Ref<Self>>,
}

impl Component {
  pub fn new() -> Self {
    let mut this: Self = Default::default();
    this.id = Uuid::new_v4();
    this
  }

  pub fn create_component(&mut self) -> Ref<Self> {
    let mut comp = Self::new();
    comp.title = "New Component".to_string();
    // comp.visible = true;
    let comp = rc(comp);
    self.children.push(comp.clone());
    comp
  }
}


#[derive(Debug, Default)]
pub struct Sketch {
  pub elements: Vec<Ref<CurveType>>,
}

impl Sketch {
  pub fn get_regions(&self, include_outer: bool) -> Vec<Region> {
    let mut cut_elements = Self::all_split(&self.elements);
    Self::remove_dangling_segments(&mut cut_elements);
    let islands = Self::build_islands(&cut_elements);
    islands.iter()
    .flat_map(|island| Self::build_loops_from_island(island, include_outer) )
    .collect()
  }

  fn build_loops_from_island(island: &Vec<TrimmedCurve>, include_outer: bool) -> Vec<Region> {
    let mut regions = vec![];
    let mut used_forward = HashSet::new();
    let mut used_backward = HashSet::new();
    for i in 0..2 {
      for start_elem in island.iter() {
        let points = start_elem.bounds;
        let start_point = if i == 0 { points.0 } else { points.1 };
        let mut loops = Self::build_loop(&start_point, &start_elem, vec![], &start_point, island, &mut used_forward, &mut used_backward);
        for region in &mut loops { geom2d::straighten_bounds(region) }
        regions.append(&mut loops);
      }
    }
    if !include_outer { Self::remove_outer_loop(&mut regions) }
    regions
  }

  pub fn split_all(&self) -> Vec<CurveType> {
    Self::all_split(&self.elements).into_iter()
    .map(|elem| elem.cache )
    .collect()
  }

  pub fn all_split(elements: &Vec<Ref<CurveType>>) -> Vec<TrimmedCurve> {
    elements.iter().flat_map(|elem| {
      let splits = Self::split_element(&elem.borrow(), &elements);
      splits.into_iter().map(|split| TrimmedCurve {
        base: (*elem.borrow()).clone(),
        bounds: split.as_curve().endpoints(),
        cache: split,
      }).collect::<Region>()
    }).collect()
  }

  pub fn split_element(elem: &CurveType, others: &Vec<Ref<CurveType>>) -> Vec<CurveType> {
    let others = others.iter().map(|other| other.borrow().clone() ).collect();
    elem.split_multi(&others)
  }

  pub fn build_islands(elements: &Vec<TrimmedCurve>) -> Vec<Vec<TrimmedCurve>> {
    let mut unused_elements = elements.clone();
    let mut islands = vec![];
    while let Some(start_elem) = unused_elements.pop() {
      let mut island = vec![];
      Self::build_island(&start_elem, &mut island, &unused_elements);
      for island_elem in island.iter() {
        unused_elements.retain(|elem| elem.bounds != island_elem.bounds);
      }
      if island.len() > 0 { islands.push(island) }
    }
    islands
  }

  fn build_island(start_elem: &TrimmedCurve, mut path: &mut Vec<TrimmedCurve>, all_elements: &Vec<TrimmedCurve>) {
    if path.iter().any(|e| e == start_elem ) { return }
    let (start_point, end_point) = start_elem.bounds;
    path.push(start_elem.clone());
    for elem in all_elements.iter() {
      let (other_start, other_end) = elem.bounds;
      // We are connected to other element
      if end_point.almost(other_start) || end_point.almost(other_end) || start_point.almost(other_start) || start_point.almost(other_end) {
        Self::build_island(&elem, &mut path, all_elements);
      }
    }
  }

  // https://stackoverflow.com/questions/838076/small-cycle-finding-in-a-planar-graph
  pub fn build_loop<'a>(
    start_point: &Point3,
    start_elem: &'a TrimmedCurve,
    mut path: Region,
    path_start_point: &Point3,
    all_elements: &'a Vec<TrimmedCurve>,
    used_forward: &mut HashSet<Uuid>,
    used_backward: &mut HashSet<Uuid>,
  ) -> Vec<Region> {
    let mut regions = vec![];
    // Traverse edges only once in every direction
    let start_elem_id = as_controllable(&start_elem.cache).id();
    if start_point.almost(start_elem.bounds.0) {
      if used_forward.contains(&start_elem_id) { return regions }
      used_forward.insert(start_elem_id);
    } else {
      if used_backward.contains(&start_elem_id) { return regions }
      used_backward.insert(start_elem_id);
    }
    // Add start_elem to path
    path.push(start_elem.clone());
    // Find connected segments
    let end_point = start_elem.other_bound(&start_point);
    let mut connected_elems: Vec<&TrimmedCurve> = all_elements.iter().filter(|other_elem| {
      let (other_start, other_end) = other_elem.bounds;
      (end_point.almost(other_start) || end_point.almost(other_end)) && as_controllable(&other_elem.cache).id() != start_elem_id
    }).collect();
    if connected_elems.len() > 0 {
      // Sort connected segments in clockwise order
      connected_elems.sort_by(|a, b| {
        let final_point_a = a.other_bound(&end_point);
        let final_point_b = b.other_bound(&end_point);
        if geom2d::clockwise(*start_point, end_point, final_point_a) > geom2d::clockwise(*start_point, end_point, final_point_b) {
          Ordering::Less
        } else {
          Ordering:: Greater
        }
        // if geom2d::clockwise(end_point, final_point_a, final_point_b) < 0.0 { Ordering::Less } else { Ordering:: Greater }
      });
      // Follow the leftmost segment to complete loop in anti-clockwise order
      let next_elem = connected_elems[0];
      // if path[0].cache == next_elem.cache {
      // if path_start_point.almost(end_point) {
      if as_controllable(&path[0].cache).id() == as_controllable(&next_elem.cache).id() {
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

  fn remove_outer_loop(loops: &mut Vec<Region>) {
    if loops.len() <= 1 { return }
    loops.retain(|region| {
      !geom2d::is_clockwise(&geom2d::poly_from_wire(region))
    })
  }

  pub fn remove_dangling_segments(island: &mut Vec<TrimmedCurve>) {
    let others = island.clone();
    let start_len = island.len();
    island.retain(|elem| {
      if elem.cache.as_curve().length().almost(0.0) { return false }
      let (start_point, end_point) = elem.bounds;
      // Keep closed circles, arcs and splines
      if start_point == end_point { return true }
      [start_point, end_point].iter().all(|endpoint| {
        others.iter().any(|other_elem| {
          let (other_start, other_end) = other_elem.bounds;
          (endpoint.almost(other_start) || endpoint.almost(other_end))
          && other_elem.bounds != elem.bounds
        })
      })
    });
    if island.len() < start_len { Self::remove_dangling_segments(island) }
  }
}


trait Constraint {}


pub trait Controllable {
  fn id(&self) -> Uuid;
  fn get_handles(&self) -> Vec<Point3>;
  fn set_handles(&mut self, handles: Vec<Point3>);
  fn get_snap_points(&self) -> Vec<Point3>;

  fn set_initial_handles(&mut self, handles: Vec<Point3>) {
    self.set_handles(handles);
  }
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
    let endpoints = self.endpoints();
    vec![self.center, endpoints.0, endpoints.1]
  }

  // Three points on arc
  fn set_initial_handles(&mut self, handles: Vec<Point3>) {
    let [p1, p2, p3]: [Point3; 3] = handles.try_into().unwrap();
    let circle = Circle::from_points(p1, p2, p3).unwrap();
    self.center = circle.center;
    self.radius = circle.radius;
    self.start = circle.unsample(&p1);
    self.end = circle.unsample(&p3);
  }

  // Endpoints + center
  fn set_handles(&mut self, handles: Vec<Point3>) {
    let [center, start, end]: [Point3; 3] = handles.try_into().unwrap();
    self.center = center;
    self.radius = (start - center).magnitude();
    let circle = Circle::new(self.center, self.radius);
    self.start = circle.unsample(&start);
    self.end = circle.unsample(&end);
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    let endpoints = self.endpoints();
    vec![self.center, endpoints.0, endpoints.1]
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


pub fn as_controllable(elem: &CurveType) -> &dyn Controllable {
  match elem {
    CurveType::Line(line) => line,
    CurveType::Arc(arc) => arc,
    CurveType::Circle(circle) => circle,
    CurveType::BezierSpline(spline) => spline,
  }
}

pub fn as_controllable_mut(elem: &mut CurveType) -> &mut dyn Controllable {
  match elem {
    CurveType::Line(line) => line,
    CurveType::Arc(arc) => arc,
    CurveType::Circle(circle) => circle,
    CurveType::BezierSpline(spline) => spline,
  }
}


#[cfg(test)]
mod tests {
  use super::*;
  use shapex::test_data;

  fn make_sketch(lines: &Vec<Line>) -> Sketch {
    let mut sketch = Sketch::default();
    for line in lines {
      sketch.elements.push(rc(line.clone().into_enum()));
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
    let regions = sketch.get_regions(false);
    assert_eq!(cut_elements.len(), 4, "{} cut_elements found instead of 4", cut_elements.len());
    assert_eq!(islands.len(), 1, "{} islands found instead of 1", islands.len());
    assert_eq!(regions.len(), 1, "{} regions found instead of 1", regions.len());
  }

  #[test]
  fn region_crossing_rect() {
    let sketch = make_sketch(&test_data::crossing_rectangle());
    let cut_elements = Sketch::all_split(&sketch.elements);
    let islands = Sketch::build_islands(&cut_elements);
    let regions = sketch.get_regions(false);
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
    let regions = sketch.get_regions(false);
    assert_eq!(cut_elements.len(), 6, "{} cut_elements found instead of 6", cut_elements.len());
    assert_eq!(islands.len(), 1, "{} islands found instead of 1", islands.len());
    assert_eq!(regions.len(), 1, "{} regions found instead of 1", regions.len());
  }

  #[test]
  fn dangling_segment() {
    let mut sketch = Sketch::default();
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0));
    sketch.elements.push(rc(line.into_enum()));
    let _regions = sketch.get_regions(false);
  }
}
