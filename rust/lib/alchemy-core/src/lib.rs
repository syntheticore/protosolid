mod tools;
mod renderer;

use std::rc::Rc;
use std::cell::RefCell;

pub use tools::*;
pub use renderer::*;
pub use shapex::*;

pub trait UIManager {
  fn update_tree(&self);
}

pub struct Alchemy {
  pub scene: Scene,
  pub renderer: Rc<RefCell<Renderer>>,
  tool_stack: Vec<Box<dyn ToolTrait>>,
  pub set_status: Box<dyn Fn(&str) -> ()>,
  pub redraw: Box<dyn Fn() -> ()>,
  pub ui_manager: Option<Rc<dyn UIManager>>
  // pub update_tree: Box<Fn() -> ()>,
}

impl Alchemy {
  pub fn new() -> Self {
    let renderer = Rc::new(RefCell::new(Renderer::new()));
    Self {
      scene: Scene::new(),
      renderer: Rc::clone(&renderer),
      tool_stack: vec![Box::new(SelectTool::new(None, &Rc::clone(&renderer)))],
      set_status: Box::new(|_| {}),
      redraw: Box::new(|| {}),
      ui_manager: None
      // update_tree: Box::new(|| {}),
    }
  }

  pub fn mouse_down(&mut self, x: f64, y: f64, button: u32) {
    println!("{:?}", button);
    self.tool_stack.last_mut().unwrap().mouse_down(&Point2::new(x, y), button);
    self.scene.build_render_tree();
    self.update_status();
  }

  pub fn mouse_up(&mut self, x: f64, y: f64) {
    self.tool_stack.last_mut().unwrap().mouse_up(&Point2::new(x, y));
    self.scene.build_render_tree();
    self.update_status();
  }

  pub fn mouse_move(&mut self, x: f64, y: f64) {
    self.tool_stack.last_mut().unwrap().mouse_move(&Point2::new(x, y));
    self.scene.build_render_tree();
  }

  pub fn mouse_wheel(&self, scroll: f64) {
    self.tool_stack.last().unwrap().mouse_wheel(scroll);
  }

  pub fn key_press(&mut self, key: u32) {
    println!("{:?}", key);
    match key {
      // ESC
      65307 => {
        self.pop_tool();
      },
      // CTRL
      65507 => {
        self.push_tool(Tool::Orbit);
      },
      // L
      108 => {
        self.push_tool(Tool::Line);
      },
      // S
      115 => {
        self.push_tool(Tool::Select);
      },
      _ => {}
    };
    self.scene.build_render_tree();
  }

  pub fn key_release(&mut self, key: u32) {
    // CTRL
    if key == 65507 {
      if self.tool_stack.len() > 1 {
        self.pop_tool();
      }
    }
    self.scene.build_render_tree();
  }

  pub fn render(&mut self, ctx: &dyn DrawingContext, width: u32, height: u32) {
    self.renderer.borrow_mut().render(ctx, &self.scene.render_tree, width, height);
    self.tool_stack.last().unwrap().render(ctx);
  }

  fn update_tree(&self) {
    let _ui_manager = self.ui_manager.as_ref().unwrap();
    // self.scene.fill_tree(&ui_manager.tree_store, &ui_manager.tree_view);
  }

  pub fn push_tool(&mut self, tool: Tool) {
    self.tool_stack.last().unwrap().suspend();
    let tool: Box<dyn ToolTrait> = match tool {
      Tool::Select => {
        let sketch = match &self.scene.current_sketch {
          Some(sketch) => Some(Rc::clone(sketch)),
          None => None
        };
        Box::new(SelectTool::new(sketch, &self.renderer))
      },
      Tool::Line => {
        if !self.scene.current_sketch.is_some() {
          self.scene.create_sketch();
          // (self.update_tree)();
          // let foo = self.ui_manager.as_ref().unwrap();
          // foo.update_tree();
          self.update_tree();
        }
        let sketch = self.scene.current_sketch.as_ref().unwrap();
        Box::new(LineTool::new(Rc::clone(sketch), Rc::clone(&self.renderer)))
      },
      Tool::Orbit => Box::new(OrbitTool::new(&self.renderer))
    };
    self.tool_stack.push(tool);
    self.update_status();
  }

  fn pop_tool(&mut self) {
    if self.tool_stack.len() > 1 {
      {
        let tool = self.tool_stack.last_mut().unwrap();
        tool.mouse_up(&Point2::new(0.0, 0.0));
        tool.suspend();
      }
      self.tool_stack.pop();
      self.tool_stack.last().unwrap().resume();
    }
    self.update_status();
  }

  pub fn make_scene(&mut self) {
    // let mut curve = PolyLine::new();

    // curve.add_vertex(Point3::new(0.1, 0.1, 0.0));
    // curve.add_vertex(Point3::new(0.4, 0.2, 0.2));
    // curve.add_vertex(Point3::new(0.6, 0.6, 0.0));
    // curve.add_vertex(Point3::new(0.7, 0.7, 0.0));

    // let mut component: Component = Default::default();
    // component.name = "Sub Assembly".to_string();
    // component.visible = true;
    // // component.material.diffuse = Color3 {
    // //   r: 0.1,
    // //   g: 0.1,
    // //   b: 0.1
    // // };
    // let mut sketch = Sketch::new();
    // sketch.elements.push(curve);
    // let sketch = Rc::new(RefCell::new(sketch));
    // component.sketches.push(Rc::clone(&sketch));
    // self.scene.tree.add_child(Rc::new(RefCell::new(component)));
    // self.scene.edit_sketch(&Rc::clone(&sketch));
    // self.scene.build_render_tree();
  }

  fn update_status(&self) {
    (self.set_status)(self.tool_stack.last().unwrap().get_status());
  }
}


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
  pub material: Material,
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
  pub render_tree: TreeNode<Vec<Box<dyn Drawable>>>,
  pub current_component: Rc<RefCell<Component>>,
  pub current_sketch: Option<Rc<RefCell<Sketch>>>
}

impl Scene {
  pub fn new() -> Self {
    let mut comp: Component = Default::default();
    comp.name = "Main Bracket".to_string();
    comp.visible = true;
    let comp = Rc::new(RefCell::new(comp));
    let tree = TreeNode::new(Some(Rc::clone(&comp)));
    let mut this = Self {
      tree: tree,
      render_tree: TreeNode::new(None),
      current_component: Rc::clone(&comp),
      current_sketch: None
    };
    this.build_render_tree();
    this
  }

  pub fn create_component(&mut self) {
    let mut component: Component = Default::default();
    component.name = "Assembly1".to_string();
    component.visible = true;
    self.tree.add_child(Rc::new(RefCell::new(component)));
    self.build_render_tree();
  }

  pub fn create_sketch(&mut self) {
    let mut sketch: Sketch = Sketch::new();
    sketch.name = "Sketch1".to_string();
    sketch.visible = true;
    let sketch = Rc::new(RefCell::new(sketch));
    self.current_component.borrow_mut().sketches.push(Rc::clone(&sketch));
    self.edit_sketch(&sketch);
    self.build_render_tree();
  }

  pub fn edit_sketch(&mut self, sketch: &Rc<RefCell<Sketch>>) {
    self.current_sketch = Some(Rc::clone(sketch));
  }

  pub fn build_render_tree(&mut self) {
    let origin = Locator::new(Point3::new(0.0, 0.0, 0.0));
    let grid = Grid::new(Plane::new(), 10, 10, 0.1);
    // self.render_tree = TreeNode::new(Some(vec![Box::new(grid), Box::new(origin)]));
    self.render_tree = TreeNode::new(Some(vec![Box::new(grid)]));
    let objects = self.build_render_node(&self.tree);
    self.render_tree.children.push(objects)
  }

  fn build_render_node(&self, node: &TreeNode<Rc<RefCell<Component>>>) -> TreeNode<Vec<Box<dyn Drawable>>> {
    let mut render_node = TreeNode::new(None);
    if let Some(ref comp) = node.item {
      let comp = comp.borrow();
      if comp.sketches.len() >= 1 {
        let drawables = comp.sketches.iter()
                                     .flat_map(|sketch| sketch.borrow().elements.clone() )
                                     .map(|drawable| Box::new(drawable.clone()) as Box<dyn Drawable> )
                                     .collect();
        render_node.item = Some(drawables);
      }
    }
    for child in &node.children {
      render_node.children.push(self.build_render_node(child));
    }
    render_node
  }
}
