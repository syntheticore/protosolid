use crate::geom::*;
// use std::rc::Rc;
use std::f64::consts::PI;
use cgmath::Transform as OtherTransform;

pub struct Renderer {
  stack: Vec<Matrix4>,
  transform: Matrix4,
  pub width: f64,
  height: f64,
  camera: Camera,
  last_x: f64,
  last_y: f64,
  mouse_pressed: u32,
  // floaters: Vec<Rc<Floater>>,
  // pub overlay: Option<Rc<gtk::Fixed>>
}

impl Renderer {
  pub fn new() -> Self {
    Self {
      stack: vec![Matrix4::identity()],
      transform: Matrix4::identity(),
      width: 1.0,
      height: 1.0,
      camera: Camera::new(),
      last_x: 0.0,
      last_y: 0.0,
      mouse_pressed: 0,
      // floaters: vec![],
      // overlay: None
    }
  }

  pub fn mouse_down(&mut self, button: u32) {
    self.mouse_pressed = button;
  }

  pub fn mouse_up(&mut self) {
    self.mouse_pressed = 0;
  }

  pub fn mouse_move(&mut self, p: Point2) {
    let diff_x = p.x - self.last_x;
    let diff_y = p.y - self.last_y;
    match self.mouse_pressed {
      1 => {
        self.camera.orbit(diff_x, diff_y);
      },
      2 => {
        self.camera.pan(diff_x, diff_y);
      },
      3 => {
        self.camera.pan(diff_x, diff_y);
      },
      _ => {},
    }
    self.last_x = p.x;
    self.last_y = p.y;
    // self.update_floaters();
  }

  pub fn mouse_wheel(&mut self, scroll: f64) {
    self.camera.zoom(scroll);
    // self.update_floaters();
  }

  // pub fn add_floater(&mut self, p: Point3) -> Option<Rc<Floater>> {
  //   let mut ret = None;
  //   // if let Some(ref overlay) = self.overlay {
  //   //   let floater = Rc::new(Floater::new(p, Rc::clone(overlay)));
  //   //   self.floaters.push(Rc::clone(&floater));
  //   //   ret = Some(floater);
  //   // }
  //   self.update_floaters();
  //   ret
  // }

  // pub fn remove_floater(&mut self, floater: &Rc<Floater>) {
  //   let index = self.floaters.iter().position(|x| x == floater).unwrap();
  //   self.floaters.swap_remove(index);
  // }

  // fn update_floaters(&mut self) {
  //   self.make_transform();
  //   let aspect = self.height / self.width;
  //   for floater in &self.floaters {
  //     let p = self.local2screen(&floater.point);
  //     floater.update(((p.x + 0.5) * self.width) as i32, ((p.y / aspect + 0.5) * self.height) as i32);
  //   }
  // }

  pub fn hit_test(&self, p: &Point2) -> Option<Point3> {
    // Look into image
    let aspect = self.height / self.width;
    let ray_clip = Vec4::new(p.x - 0.5, (-p.y + 0.5) * aspect, -1.0, 1.0);
    // Unproject to eye space
    let inverse_projection = cgmath::perspective(self.camera.fov, 1.0, 0.0001, 10000.0).invert().unwrap();
    let mut ray_eye = inverse_projection * ray_clip;
    // Transform ray to world space
    ray_eye.z = -1.0;
    ray_eye.w = 0.0;
    let inverse_view = self.camera.to_mat4();
    let ray_world = (inverse_view * ray_eye).truncate().normalize();
    // Intersect ray with active sketch plane
    match Plane::new().intersect_line((self.camera.position, self.camera.position + ray_world)) {
      Intersection::One(point) | Intersection::Extended(point) => Some(point),
      _ => None
    }
  }

  pub fn render(&mut self, ctx: &dyn DrawingContext, tree: &TreeNode<Vec<Box<dyn Drawable>>>, width: u32, height: u32) {
    // ctx.scale(width.into(), width.into());
    // ctx.translate(0.5, 0.5 * self.height / self.width);
    ctx.set_line_width(1.0 / width as f64);
    // ctx.set_dash(&[3., 2., 1.], 1.);
    // ctx.set_source_rgb(0.11, 0.122, 0.125);
    ctx.clear();
    // ctx.paint();
    self.width = width as f64;
    self.height = height as f64;
    self.render_treenode(ctx, tree);
  }

  fn render_treenode(&mut self, ctx: &dyn DrawingContext, node: &TreeNode<Vec<Box<dyn Drawable>>>) {
    self.stack.push(node.transform.to_mat4());
    if let Some(ref drawables) = node.item {
      self.make_transform();
      for drawable in drawables {
        drawable.draw(ctx, self);
      }
    }
    for child in &node.children {
      self.render_treenode(ctx, child);
    }
    self.stack.pop();
  }

  pub fn get_projection(&self) -> Matrix4 {
    // let projection = cgmath::perspective(cgmath::Deg(90.0), self.width / self.height, 0.01, 10000.0);
    // let projection = cgmath::ortho(-3.0, 3.0, -3.0, 3.0, 0.01, 1000.0);
    let projection = cgmath::perspective(self.camera.fov, 1.0, 0.0001, 10000.0);
    let view = self.camera.to_mat4().invert().unwrap();
    projection * view
  }

  pub fn world2screen(&self, point: &Point3) -> Point2 {
    let projection = self.get_projection();
    let p = projection.transform_point(*point);
    let aspect = self.height / self.width;
    Point2::new(p.x + 0.5, -p.y / aspect + 0.5)
  }

  fn make_transform(&mut self) {
    let world_transform: Matrix4 = self.stack.iter().product();
    let projection = self.get_projection();
    self.transform = projection * world_transform;
  }

  fn local2screen(&self, point: &Point3) -> Point2 {
    let p = self.transform.transform_point(*point);
    Point2::new(p.x, -p.y)
  }

  pub fn draw_circle(&self, ctx: &dyn DrawingContext, point: &Point3, size: f64) {
    let point = self.local2screen(point);
    ctx.arc(point.x, point.y, size / self.width, 0.0, PI * 2.);
    ctx.fill();
  }

  pub fn draw_line(&self, ctx: &dyn DrawingContext, p1: &Point3, p2: &Point3) {
    let p1 = self.local2screen(p1);
    let p2 = self.local2screen(p2);
    ctx.move_to(p1.x, p1.y);
    ctx.line_to(p2.x, p2.y);
    ctx.stroke();
  }

  pub fn set_line_width(&self, ctx: &dyn DrawingContext, width: f64) {
    ctx.set_line_width(width / self.width);
  }
}


pub trait Drawable {
  fn draw(&self, ctx: &dyn DrawingContext, renderer: &Renderer);
}


pub trait DrawingContext {
  fn move_to(&self, x: f64, y: f64);
  fn line_to(&self, x: f64, y: f64);
  fn begin_path(&self);
  fn stroke(&self);
  fn clear(&self);
  fn set_line_width(&self, width: f64);
  fn arc(&self, x: f64, y: f64, radius: f64, start_angle: f64, end_angle: f64);
  fn fill(&self);
  fn scale(&self, x: f64, y: f64);
  fn translate(&self, x: f64, y: f64);
  fn set_source_rgb(&self, r: f64, g: f64, b: f64);
  fn paint(&self);
}


#[derive(Copy, Clone, Debug, Default)]
pub struct Color3 {
  pub r: f64,
  pub g: f64,
  pub b: f64
}


#[derive(Copy, Clone, Debug, Default)]
pub struct Material {
  pub diffuse: Color3
}


struct Camera {
  // transform: Transform,
  position: Point3,
  target: Point3,
  fov: cgmath::Deg<f64>,
  rotate_x: f64,
  rotate_y: f64,
  zoom: f64
}

impl Camera {
  pub fn new() -> Self {
    let mut this = Self {
      // transform: Transform {
      //   translation: Vec3::new(0.0, 0.0, 0.0)
      // },
      position: Point3::new(0.0, 0.0, -1.0),
      target: Point3::new(0.0, 0.0, 0.0),
      fov: cgmath::Deg(60.0),
      rotate_x: 0.0,
      rotate_y: 0.0,
      zoom: 4.0
    };
    this.orbit(0.0, 0.0);
    this
  }

  pub fn orbit(&mut self, x: f64, y: f64) {
    self.rotate_x -= x * 10.0;
    self.rotate_y += y * 10.0;
    self.update_transform();
  }

  pub fn pan(&mut self, x: f64, y: f64) {
    self.target.x -= x * 4.0;
    self.target.z -= y * 4.0;
    self.update_transform();
  }

  pub fn zoom(&mut self, zoom: f64) {
    self.zoom += zoom / 3.0;
    self.zoom = self.zoom.max(1.0);
    self.update_transform();
  }

  pub fn to_mat4(&self) -> Matrix4 {
    Matrix4::look_at(
      self.position,
      self.target,
      Vec3::new(0.0, 1.0, 0.0)
    ).invert().unwrap()
  }

  fn update_transform(&mut self) {
    self.position = Point3 {
      x: self.rotate_x.sin() * self.zoom,
      y: self.rotate_y,
      z: self.rotate_x.cos() * self.zoom
    } + self.target.to_vec();
  }
}


pub struct Grid {
  plane: Plane,
  num_u: i32,
  num_v: i32,
  step: f64
}

impl Grid {
  pub fn new(plane: Plane, num_u: i32, num_v: i32, step: f64) -> Self {
    Self {
      plane: plane,
      num_u: num_u,
      num_v: num_v,
      step: step
    }
  }
}

impl Drawable for Grid {
  fn draw(&self, ctx: &dyn DrawingContext, renderer: &Renderer) {
    // ctx.set_source_rgb(0.3, 0.3, 0.3);
    ctx.set_source_rgb(0.191, 0.214, 0.221);
    for u in -self.num_u..=self.num_u {
      let start = self.plane.eval(u as f64 * self.step, -self.num_v as f64 * self.step);
      let end = self.plane.eval(u as f64 * self.step, self.num_v as f64 * self.step);
      renderer.draw_line(ctx, &start, &end);
    }
    for v in -self.num_v..=self.num_v {
      let start = self.plane.eval(-self.num_u as f64 * self.step, v as f64 * self.step);
      let end = self.plane.eval(self.num_u as f64 * self.step, v as f64 * self.step);
      renderer.draw_line(ctx, &start, &end);
    }
  }
}


pub struct Locator {
  location: Point3
}

impl Locator {
  pub fn new(p: Point3) -> Self {
    Self {
      location: p
    }
  }
}

impl Drawable for Locator {
  fn draw(&self, ctx: &dyn DrawingContext, renderer: &Renderer) {
    ctx.set_line_width(1.0 / renderer.width);
    // Draw X axis
    ctx.set_source_rgb(1.0, 0.0, 0.0);
    renderer.draw_line(ctx, &self.location, &Point3::from_vec(Vec3::unit_x() * 0.5));
    // Draw Y axis
    ctx.set_source_rgb(0.0, 1.0, 0.0);
    renderer.draw_line(ctx, &self.location, &Point3::from_vec(Vec3::unit_y() * 0.5));
    // Draw Z axis
    ctx.set_source_rgb(0.0, 0.0, 1.0);
    renderer.draw_line(ctx, &self.location, &Point3::from_vec(Vec3::unit_z() * 0.5));
    // Draw self.location
    ctx.set_source_rgb(0.9, 0.9, 0.9);
    renderer.draw_circle(ctx, &self.location, 5.0);
  }
}


impl Drawable for PolyLine {
  fn draw(&self, ctx: &dyn DrawingContext, renderer: &Renderer) {
    let len = self.vertices.len();
    if self.vertices.len() > 1 {
      ctx.set_line_width(2.0 / renderer.width);
      for (i, vertex) in self.vertices.iter().take(len - 1).enumerate() {
        let next = &self.vertices[i + 1];
        ctx.set_source_rgb(0.302, 0.824, 1.0);
        renderer.draw_line(ctx, &vertex, &next);
        let draw_vertices = true;
        if draw_vertices {
          // ctx.set_source_rgb(0.6, 0.6, 0.6);
          ctx.set_source_rgb(1.0, 1.0, 1.0);
          renderer.draw_circle(ctx, &vertex, 4.0);
        }
        if draw_vertices && i == len - 2 {
          renderer.draw_circle(ctx, &next, 4.0);
        }
      }
    }
  }
}


// #[derive(PartialEq)]
// pub struct Floater {
//   // pub id: uuid::Uuid,
//   point: Point3,
//   overlay: Rc<gtk::Fixed>,
//   floater: gtk::Widget
// }

// impl Floater {
//   pub fn new(p: Point3, overlay: Rc<gtk::Fixed>) -> Self {
//     let floater = gtk::Button::new_from_icon_name("document-edit-symbolic", gtk::IconSize::Button.into());
//     overlay.put(&floater, p.x as i32, p.y as i32);
//     floater.show();
//     Self {
//       // id: uuid::Uuid::new_v4(),
//       point: p,
//       overlay: overlay,
//       floater: floater.upcast::<gtk::Widget>()
//     }
//   }

//   pub fn update(&self, x: i32, y: i32) {
//     self.overlay.move_(&self.floater, x, y);
//   }
// }

// impl Drop for Floater {
//   fn drop(&mut self) {
//     self.overlay.remove(&self.floater);
//     self.floater.destroy();
//     self.overlay.show_all();
//   }
// }
