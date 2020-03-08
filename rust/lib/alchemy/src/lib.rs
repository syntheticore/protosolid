// mod tools;
// mod renderer;

// pub use tools::*;
// pub use renderer::*;
pub use shapex;

use std::rc::Rc;
use std::cell::RefCell;
use shapex::*;


#[derive(Default)]
pub struct Component {
  pub name: String,
  pub parts: Vec<Part>,
  pub sketches: Vec<Rc<RefCell<Sketch>>>,
  pub visible: bool
}


pub struct Part {
  pub name: String,
  pub solid: Solid,
  // pub material: Material,
  pub visible: bool
}


trait Constraint {}


pub struct VertexIterator<'a> {
  elem_iter: std::slice::Iter<'a, PolyLine>,
  vertex_iter: Option<std::slice::Iter<'a, Point3>>,
}

impl<'a> VertexIterator<'a> {
  pub fn new(sketch: &'a Sketch) -> Self {
    Self {
      elem_iter: sketch.elements.iter(),
      vertex_iter: None
    }
  }
}

impl<'a> Iterator for VertexIterator<'a> {
  type Item = &'a Point3;

  fn next(&mut self) -> Option<&'a Point3> {
    if let Some(ref mut vertex_iter) = self.vertex_iter {
      let vertex = vertex_iter.next();
      if vertex.is_some() {
        vertex
      } else {
        self.vertex_iter = None;
        self.next()
      }
    } else {
      if let Some(line) = self.elem_iter.next() {
        self.vertex_iter = Some(line.vertices.iter());
        self.next()
      } else {
        None
      }
    }
  }
}


#[derive(Debug)]
pub struct Sketch {
  pub name: String,
  pub plane: Plane,
  pub elements: Vec<PolyLine>,
  // pub constraints: Vec<Box<Constraint>>
  pub visible: bool
}

impl Sketch {
  pub fn new() -> Self {
    Self {
      name: "Sketch1".to_string(),
      plane: Plane::new(),
      elements: vec![],
      // constraints: vec![]
      visible: true
    }
  }

  pub fn all_vertices(&self) -> VertexIterator {
    VertexIterator::new(self)
  }
}


pub struct Scene {
  pub tree: TreeNode<Rc<RefCell<Component>>>,
  // pub render_tree: TreeNode<Vec<Box<dyn Drawable>>>,
  // pub current_component: Rc<RefCell<Component>>,
  // pub current_sketch: Option<Rc<RefCell<Sketch>>>
}

impl Scene {
  pub fn new() -> Self {
    let mut comp: Component = Default::default();
    comp.name = "Main Bracket".to_string();
    comp.visible = true;
    let comp = Rc::new(RefCell::new(comp));
    let tree = TreeNode::new(Some(Rc::clone(&comp)));
    Self {
      tree: tree,
    }
  }

  pub fn create_component(&mut self) {
    let mut component: Component = Default::default();
    component.name = "Assembly1".to_string();
    component.visible = true;
    self.tree.add_child(Rc::new(RefCell::new(component)));
  }

  pub fn create_sketch(&mut self) {
    let mut sketch: Sketch = Sketch::new();
    sketch.name = "Sketch1".to_string();
    sketch.visible = true;
    let _sketch = Rc::new(RefCell::new(sketch));
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
