// mod tools;
// mod renderer;

// pub use tools::*;
// pub use renderer::*;

pub use shapex;

use core::fmt::Debug;
use std::rc::Rc;
use std::cell::RefCell;
use uuid::Uuid;
// use std::marker::PhantomPinned;
// use std::pin::Pin;
// use std::ptr::NonNull;
use shapex::*;


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
  fn get_handles(&self) -> &Vec<Point3>;
  fn set_handles(&mut self, _: Vec<Point3>);
}


pub trait SketchElement: Differentiable + Controllable {}

impl Debug for dyn SketchElement {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Sketch elem")
  }
}


impl Controllable for BezierSpline {
  fn get_handles(&self) -> &Vec<Point3> {
    &self.vertices
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.vertices = handles;
    self.update();
  }
}

impl SketchElement for BezierSpline {}


#[derive(Debug)]
pub struct Sketch {
  pub title: String,
  pub plane: Plane,
  // pub elements: Vec<PolyLine>,
  pub elements: Vec<Box<dyn SketchElement>>,
  // pub constraints: Vec<Box<Constraint>>
  pub visible: bool
}

impl Sketch {
  pub fn new() -> Self {
    Self {
      title: "Sketch1".to_string(),
      plane: Plane::new(),
      elements: vec![],
      // constraints: vec![]
      visible: true
    }
  }

  // pub fn all_vertices(&self) -> VertexIterator {
  //   VertexIterator::new(self)
  // }
}


#[derive(Debug)]
pub struct TreeNode<T> {
  pub item: T,
  pub transform: Matrix4,
  pub children: Vec<TreeNode<T>>
}

impl<T> TreeNode<T> {
  pub fn new(item: T) -> Self {
    Self {
      item: item,
      transform: Matrix4::from_scale(1.0),
      children: Default::default()
    }
  }

  pub fn add_child(&mut self, child: T) -> &Self {
    self.children.push(Self::new(child));
    &self.children.last().unwrap()
  }
}


#[derive(Default)]
pub struct Component {
  pub id: Uuid,
  pub title: String,
  pub bodies: Vec<Solid>,
  pub sketches: Vec<Rc<RefCell<Sketch>>>,
  pub visible: bool,
  pub children: Vec<Rc<RefCell<Component>>>,
}

impl Component {
  pub fn new() -> Self {
    let mut this: Self = Default::default();
    this.id = Uuid::new_v4();
    this
  }
}


pub struct Scene {
  pub tree: Rc<RefCell<Component>>,
  pub current_node: Rc<RefCell<Component>>,
  // pub render_tree: TreeNode<Vec<Box<dyn Drawable>>>,
  // pub current_sketch: Option<Rc<RefCell<Sketch>>>
}

impl Scene {
  pub fn new() -> Self {
    let mut comp = Component::new();
    comp.title = "Main Assembly".to_string();
    comp.visible = true;
    let tree = Rc::new(RefCell::new(comp));
    let current_node = Rc::clone(&tree);
    Self { tree, current_node }
  }

  pub fn create_component(&mut self) -> Rc<RefCell<Component>> {
    let mut comp = Component::new();
    comp.title = "New Component".to_string();
    comp.visible = true;
    let comp = Rc::new(RefCell::new(comp));
    {
      let mut current_node = self.current_node.borrow_mut();
      current_node.children.push(Rc::clone(&comp));
    }
    self.current_node = Rc::clone(&comp);
    comp
  }

  pub fn create_sketch(&mut self) -> Rc<RefCell<Sketch>> {
    let mut sketch: Sketch = Sketch::new();
    sketch.title = "Sketch1".to_string();
    sketch.visible = true;
    let sketch = Rc::new(RefCell::new(sketch));
    self.current_node.borrow_mut().sketches.push(Rc::clone(&sketch));
    sketch
  }

  pub fn activate(&mut self, comp: Rc<RefCell<Component>>) {
    self.current_node = comp;
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
