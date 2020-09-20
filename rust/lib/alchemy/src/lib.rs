use std::ptr;
use std::rc::Rc;
use std::cell::RefCell;
use uuid::Uuid;

pub use shapex::*;


trait Constraint {}


// #[derive(Debug)]
// pub struct TreeNode<T> {
//   pub item: Option<T>,
//   pub transform: Matrix4,
//   pub children: Vec<TreeNode<T>>
// }

// impl<T> TreeNode<T> {
//   pub fn new(item: Option<T>) -> Self {
//     Self {
//       item: item,
//       transform: Matrix4::from_scale(1.0),
//       children: Default::default()
//     }
//   }

//   pub fn add_child(&mut self, child: T) {
//     self.children.push(TreeNode::new(Some(child)));
//   }
// }


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

impl Controllable for Circle {
  fn id(&self) -> Uuid {
    self.id
  }

  fn get_handles(&self) -> Vec<Point3> {
    vec![self.center]
  }

  fn set_handles(&mut self, _handles: Vec<Point3>) {}

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

// pub trait SketchElement: Curve + Controllable {}

// impl core::fmt::Debug for dyn SketchElement {
//   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//     write!(f, "Sketch elem")
//   }
// }

// impl SketchElement for Line {}
// impl SketchElement for Circle {}
// impl SketchElement for BezierSpline {}

pub fn as_controllable(elem: &mut SketchElement) -> &mut dyn Controllable {
  match elem {
    SketchElement::Line(line) => line,
    SketchElement::Circle(circle) => circle,
    SketchElement::BezierSpline(spline) => spline,
  }
}


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


// #[derive(Debug)]
// pub struct TreeNode<T> {
//   pub item: T,
//   pub transform: Matrix4,
//   pub children: Vec<TreeNode<T>>
// }

// impl<T> TreeNode<T> {
//   pub fn new(item: T) -> Self {
//     Self {
//       item: item,
//       transform: Matrix4::from_scale(1.0),
//       children: Default::default()
//     }
//   }

//   pub fn add_child(&mut self, child: T) -> &Self {
//     self.children.push(Self::new(child));
//     &self.children.last().unwrap()
//   }
// }


#[derive(Default)]
pub struct Component {
  pub id: Uuid,
  pub title: String,

  pub bodies: Vec<Solid>,

  // pub sketches: Vec<Rc<RefCell<Sketch>>>,
  pub visible: bool,
  pub children: Vec<Rc<RefCell<Component>>>,
  // pub sketch_elements: Vec<Rc<RefCell<dyn SketchElement>>>,
  pub sketch_elements: Vec<Rc<RefCell<SketchElement>>>,
}

impl Component {
  pub fn new() -> Self {
    let mut this: Self = Default::default();
    this.id = Uuid::new_v4();
    this
  }

  pub fn closed_regions(&self) -> Vec<PolyLine> {
    let cut_elements = Self::split_all(&self.sketch_elements);
    let islands = Self::build_islands(&cut_elements);
    let mut regions = vec![];
    for island in islands.iter() {
      let first = &island[0];
      regions.append(&mut Self::build_regions(first.as_curve().endpoints().0, first, vec![], &island));
    }
    regions
  }

  pub fn all_split(&self) -> Vec<SketchElement> {
    Self::split_all(&self.sketch_elements)
  }

  fn split_all(elems: &Vec<Rc<RefCell<SketchElement>>>) -> Vec<SketchElement> {
    elems.iter().flat_map(|elem| {
      Self::split_element(&elem.borrow(), elems)
    }).collect()
  }

  fn split_element(elem: &SketchElement, others: &Vec<Rc<RefCell<SketchElement>>>) -> Vec<SketchElement> {
    let mut segments = vec![elem.clone()];
    for other in others.iter() {
      segments = segments.iter().flat_map(|own| {
        if let Some(new_segments) = own.as_curve().split(&other.borrow()) {
          new_segments
        } else {
          vec![own.clone()]
        }
      }).collect();
    }
    segments
  }

  fn build_islands(elements: &Vec<SketchElement>) -> Vec<Vec<SketchElement>> {
    let mut unused_elements = elements.clone();
    let mut islands = vec![];
    while unused_elements.len() != 0 {
      let start_elem = unused_elements.pop().unwrap();
      let mut island = vec![];
      Self::build_island(&start_elem, &mut island, &unused_elements);
      for island_elem in island.iter() {
        unused_elements.retain(|elem| !ptr::eq(elem, island_elem));
      }
      islands.push(island);
    }
    islands
  }

  fn build_island(start_elem: &SketchElement, mut path: &mut Vec<SketchElement>, all_elements: &Vec<SketchElement>) {
    let (start_point, end_point) = start_elem.as_curve().endpoints();
    if path.iter().any(|e| e.as_curve().endpoints().0 == start_point ) { return }
    path.push(start_elem.clone());
    for elem in all_elements.iter() {
      let (other_start, other_end) = elem.as_curve().endpoints();
      // We are connected to other element
      if end_point == other_start || end_point == other_end || start_point == other_start || start_point == other_end {
        Self::build_island(elem, &mut path, all_elements);
      }
    }
  }

  fn build_regions(start_point: Point3, start_elem: &SketchElement, mut path: Vec<Point3>, all_elements: &Vec<SketchElement>) -> Vec<Vec<Point3>> {
    path.push(start_point);
    let end_point = Self::other_endpoint(start_elem, &start_point);
    let mut regions: Vec<Vec<Point3>> = vec![];
    for elem in all_elements.iter() {
      let (other_start, other_end) = elem.as_curve().endpoints();
      // We are connected to this other element
      if end_point == other_start || end_point == other_end {
        // We are closing a loop
        if path.contains(&end_point) {
          regions.push(path.clone());
        } else {
          let mut new_regions = Self::build_regions(end_point, elem, path.clone(), all_elements);
          regions.append(&mut new_regions);
        }
      }
    }
    regions
  }

  fn other_endpoint(elem: &SketchElement, point: &Point3) -> Point3 {
    let (start, end) = elem.as_curve().endpoints();
    if *point == start { end } else { start }
  }
}


pub struct Scene {
  pub tree: Rc<RefCell<Component>>,
  pub current_component: Rc<RefCell<Component>>,
  // pub render_tree: TreeNode<Vec<Box<dyn Drawable>>>,
  // pub current_sketch: Option<Rc<RefCell<Sketch>>>
}

impl Scene {
  pub fn new() -> Self {
    let mut comp = Component::new();
    comp.title = "Main Assembly".to_string();
    comp.visible = true;
    let tree = Rc::new(RefCell::new(comp));
    let current_component = Rc::clone(&tree);
    Self { tree, current_component }
  }

  pub fn create_component(&mut self) -> Rc<RefCell<Component>> {
    let mut comp = Component::new();
    comp.title = "New Component".to_string();
    comp.visible = true;
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

  // pub fn build_render_tree(&mut self) {
  //   let origin = Locator::new(Point3::new(0.0, 0.0, 0.0));
  //   let grid = Grid::new(Plane::new(), 10, 10, 0.1);
  //   // self.render_tree = TreeNode::new(Some(vec![Box::new(grid), Box::new(origin)]));
  //   self.render_tree = TreeNode::new(Some(vec![Box::new(grid)]));
  //   let objects = self.build_render_node(&self.tree);
  //   self.render_tree.children.push(objects)
  // }

  // fn build_render_node(&self, node: &TreeNode<Rc<RefCell<Component>>>) -> TreeNode<Vec<Box<dyn Drawable>>> {
  //   let mut render_node = TreeNode::new(None);
  //   if let Some(ref comp) = node.item {
  //     let comp = comp.borrow();
  //     if comp.sketches.len() >= 1 {
  //       let drawables = comp.sketches.iter()
  //                                    .flat_map(|sketch| sketch.borrow().elements.clone() )
  //                                    .map(|drawable| Box::new(drawable.clone()) as Box<dyn Drawable> )
  //                                    .collect();
  //       render_node.item = Some(drawables);
  //     }
  //   }
  //   for child in &node.children {
  //     render_node.children.push(self.build_render_node(child));
  //   }
  //   render_node
  // }
}
