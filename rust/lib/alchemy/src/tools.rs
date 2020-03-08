use std::rc::Rc;
use std::cell::RefCell;
use crate::*;

const SNAP_THRESH: f64 = 0.01;

pub enum Tool {
  Select,
  Line,
  Orbit
}

pub trait ToolTrait {
  fn mouse_down(&mut self, p: &Point2, button: u32);
  fn mouse_up(&mut self, p: &Point2);
  fn mouse_move(&mut self, p: &Point2);
  fn mouse_wheel(&self, scroll: f64);
  fn suspend(&self);
  fn resume(&self);
  fn render(&self, ctx: &dyn DrawingContext);
  fn get_status(&self) -> &str;

  // fn snap<'a>(&self, p: &Point2, sketch: &'a Sketch, renderer: &Renderer) -> Option<&'a Point3> {
  //   for line in &sketch.elements {
  //     for vertex in &line.vertices {
  //       let pos = renderer.world2screen(vertex);
  //       let dist = pos.distance(*p);
  //       if dist < SNAP_THRESH {
  //         return Some(vertex);
  //       }
  //     }
  //   }
  //   None
  // }

  fn snap(&self, p: &Point2, sketch: &Sketch, renderer: &Renderer) -> Option<(usize, usize, Point3)> {
    for (i, line) in sketch.elements.iter().enumerate() {
      for (j, vertex) in line.vertices.iter().enumerate() {
        let pos = renderer.world2screen(vertex);
        let dist = pos.distance(*p);
        if dist < SNAP_THRESH {
          return Some((i, j, *vertex));
        }
      }
    }
    None
  }

  fn snapped(&self, p: &Point2, sketch: &Sketch, renderer: &Renderer) -> Option<Point3> {
    match self.snap(p, sketch, renderer) {
      Some((_, _, point)) => Some(point),
      _ => renderer.hit_test(p)
    }
  }
}


pub struct SelectTool {
  sketch: Option<Rc<RefCell<Sketch>>>,
  renderer: Rc<RefCell<Renderer>>,
  // floaters: Vec<Rc<Floater>>,
  preview: Option<Point3>,
  last_vertex: Option<(usize, usize, Point3)>
}

impl SelectTool {
  pub fn new(sketch: Option<Rc<RefCell<Sketch>>>, renderer: &Rc<RefCell<Renderer>>) -> Self {
    Self {
      sketch: sketch,
      renderer: Rc::clone(renderer),
      // floaters: vec![],
      preview: None,
      last_vertex: None
    }
  }
}

impl ToolTrait for SelectTool {
  fn mouse_down(&mut self, p: &Point2, _button: u32) {
    // self.floaters.push(self.renderer.borrow_mut().add_floater(
    //   Point3::new(
    //     rand::random::<f64>() - rand::random::<f64>(),
    //     rand::random::<f64>(),
    //     rand::random::<f64>() - rand::random::<f64>()
    //   )
    // ).unwrap());
    if let Some(sketch) = &self.sketch {
      self.last_vertex = self.snap(p, &sketch.borrow(), &self.renderer.borrow());
    }
  }

  fn mouse_up(&mut self, _p: &Point2) {
    self.last_vertex = None;
  }

  fn mouse_move(&mut self, p: &Point2) {
    if let Some(sketch) = &self.sketch {
      if let Some((i, j, _)) = self.last_vertex {
        let hit = self.snapped(p, &sketch.borrow(), &self.renderer.borrow());
        if let Some(point) = hit {
          sketch.borrow_mut().elements[i].vertices[j] = point;
        }
      } else  {
        for vertex in sketch.borrow().all_vertices() {
          let pos = self.renderer.borrow().world2screen(vertex);
          let dist = pos.distance(*p);
          if dist < SNAP_THRESH {
            self.preview = Some(*vertex);
            return;
          }
        }
      }
    }
    self.preview = None;
  }

  fn mouse_wheel(&self, _scroll: f64) {}

  fn suspend(&self) {}

  fn resume(&self) {}

  fn render(&self, ctx: &dyn DrawingContext) {
    let renderer = self.renderer.borrow();
    ctx.set_source_rgb(1.0, 0.0, 0.0);
    if let Some(point) = self.preview {
      renderer.draw_circle(ctx, &point, 4.0);
    }
  }

  fn get_status(&self) -> &str {"Select Geometry"}
}

impl Drop for SelectTool {
  fn drop(&mut self) {
    // for floater in &self.floaters {
    //   self.renderer.borrow_mut().remove_floater(&floater);
    // }
  }
}


pub struct LineTool {
  sketch: Rc<RefCell<Sketch>>,
  renderer: Rc<RefCell<Renderer>>,
  last_point: Option<Point3>,
  current_point: Option<Point3>
}

impl LineTool {
  pub fn new(sketch: Rc<RefCell<Sketch>>, renderer: Rc<RefCell<Renderer>>) -> Self {
    sketch.borrow_mut().elements.push(PolyLine::new());
    Self {
      sketch: sketch,
      renderer: renderer,
      last_point: None,
      current_point: None
    }
  }
}

impl ToolTrait for LineTool {
  fn suspend(&self) {}

  fn resume(&self) {}

  fn mouse_down(&mut self, p: &Point2, _button: u32) {
    let hit = self.snapped(p, &self.sketch.borrow(), &self.renderer.borrow());
    if let Some(point) = hit {
      if let Some(last) = self.last_point {
        let mut line = PolyLine::new();
        line.add_vertex(last);
        line.add_vertex(point);
        let mut sketch = self.sketch.borrow_mut();
        sketch.elements.push(line);
      }
      self.last_point = Some(point);
    }
  }

  fn mouse_up(&mut self, _p: &Point2) {}

  fn mouse_move(&mut self, p: &Point2) {
    self.current_point = self.snapped(p, &self.sketch.borrow(), &self.renderer.borrow());
  }

  fn mouse_wheel(&self, _scroll: f64) {}

  fn render(&self, ctx: &dyn DrawingContext) {
    let renderer = self.renderer.borrow();
    if let Some(current) = self.current_point {
      if let Some(last) = self.last_point {
        renderer.set_line_width(ctx, 2.0);
        renderer.draw_line(ctx, &last, &current);
        renderer.draw_circle(ctx, &last, 4.0);
      }
      renderer.draw_circle(ctx, &current, 4.0);
    }
  }

  fn get_status(&self) -> &str {"Draw Line"}
}

impl Drop for LineTool {
  fn drop(&mut self) {
    let mut sketch = self.sketch.borrow_mut();
    if sketch.elements.last_mut().unwrap().vertices.len() == 1 {
      sketch.elements.pop();
    }
  }
}


pub struct OrbitTool {
  renderer: Rc<RefCell<Renderer>>
}

impl OrbitTool {
  pub fn new(renderer: &Rc<RefCell<Renderer>>) -> Self {
    Self {
      renderer: Rc::clone(renderer)
    }
  }
}

impl ToolTrait for OrbitTool {
  fn suspend(&self) {}

  fn resume(&self) {}

  fn mouse_down(&mut self, _p: &Point2, button: u32) {
    self.renderer.borrow_mut().mouse_down(button);
  }

  fn mouse_up(&mut self, _p: &Point2) {
    self.renderer.borrow_mut().mouse_up();
  }

  fn mouse_move(&mut self, p: &Point2) {
    self.renderer.borrow_mut().mouse_move(*p);
  }

  fn mouse_wheel(&self, scroll: f64) {
    self.renderer.borrow_mut().mouse_wheel(scroll);
  }

  fn render(&self, _ctx: &dyn DrawingContext) {}

  fn get_status(&self) -> &str {"Orbit Camera"}
}
