use crate::base::*;
use cgmath::prelude::*;
use uuid::Uuid;
use std::convert::TryInto;

mod intersection;


pub type PolyLine = Vec<Point3>;


#[derive(Debug, Clone, PartialEq)]
pub enum SketchElement {
  Line(Line),
  Arc(Arc),
  Circle(Circle),
  BezierSpline(BezierSpline),
}

impl SketchElement {
  pub fn as_curve(&self) -> &dyn Curve {
    match self {
      Self::Line(line) => line,
      Self::Arc(arc) => arc,
      Self::Circle(circle) => circle,
      Self::BezierSpline(spline) => spline,
    }
  }

  pub fn as_curve_mut(&mut self) -> &mut dyn Curve {
    match self {
      Self::Line(line) => line,
      Self::Arc(arc) => arc,
      Self::Circle(circle) => circle,
      Self::BezierSpline(spline) => spline,
    }
  }

  pub fn split(&self, cutter: &Self) -> Vec<Self> {
    match self {
      // Line
      Self::Line(line) => line.split_with(cutter).iter().map(|seg| Self::Line(seg.clone())).collect(),

      // Arc
      Self::Arc(arc) => match cutter {
        Self::Line(cutter) => arc.split_with_line(cutter),
        Self::Arc(cutter) => arc.split_with_arc(cutter),
        Self::Circle(cutter) => arc.split_with_circle(cutter),
        Self::BezierSpline(cutter) => arc.split_with_spline(cutter),
      }.iter().map(|seg| Self::Arc(seg.clone())).collect(),

      // Circle
      Self::Circle(circle) => match cutter {
        Self::Line(cutter) => if let Some((arc_l, arc_r)) = circle.split_with_line(cutter) {
          vec![arc_l, arc_r]
        } else { vec![] },
        Self::Arc(cutter) => if let Some((arc_l, arc_r)) = circle.split_with_arc(cutter) {
          vec![arc_l, arc_r]
        } else { vec![] },
        Self::Circle(cutter) => if let Some((arc_l, arc_r)) = circle.split_with_circle(cutter) {
          vec![arc_l, arc_r]
        } else { vec![] },
        Self::BezierSpline(cutter) => circle.split_with_spline(cutter),
      }.iter().map(|seg| Self::Arc(seg.clone())).collect(),

      // Bezier Spline
      Self::BezierSpline(spline) => match cutter {
        Self::Line(cutter) => spline.split_with_line(cutter),
        Self::Arc(cutter) => spline.split_with_arc(cutter),
        Self::Circle(cutter) => spline.split_with_circle(cutter),
        Self::BezierSpline(cutter) => spline.split_with_spline(cutter),
      }.iter().map(|seg| Self::BezierSpline(seg.clone())).collect(),
    }
  }
}


pub trait Curve {
  fn sample(&self, t: f64) -> Point3;
  fn default_tesselation(&self) -> Vec<Point3>;
  fn length(&self) -> f64;
  fn endpoints(&self) -> (Point3, Point3);

  fn tesselate(&self, steps: i32) -> Vec<Point3> {
    (0..steps + 1).map(|i| {
      self.sample(1.0 / steps as f64 * i as f64)
    }).collect()
  }
}

impl core::fmt::Debug for dyn Curve {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


/// A finite line segment between two points
/// # Examples
///
/// ```
/// let x = 5;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
  pub id: Uuid,
  pub points: (Point3, Point3)
}

impl Line {
  pub fn new(start: Point3, end: Point3) -> Self {
    Self {
      id: Uuid::new_v4(),
      points: (start, end),
    }
  }

  pub fn midpoint(&self) -> Point3 {
    (self.points.0 + self.points.1.to_vec()) / 2.0
  }

  pub fn tangent(&self) -> Vec3 {
    (self.points.1 - self.points.0).normalize()
  }

  fn split_with(&self, cutter: &SketchElement) -> Vec<Line> {
    match intersection::intersect(&SketchElement::Line(self.clone()), cutter) {
      Intersection::None | Intersection::Contained | Intersection::Touch(_) | Intersection::Extended(_) => vec![self.clone()],
      Intersection::Hit(mut points) => { //XXX points are not sorted along line
        points.push(self.points.1);
        let mut segments = vec![Self::new(self.points.0, points[0])];
        let mut iter = points.iter().peekable();
        loop {
          match (iter.next(), iter.peek()) {
            (Some(p), Some(next_p)) => segments.push(Self::new(*p, **next_p)),
            _ => break,
          }
        }
        segments
      },
    }
  }
}

impl Curve for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(1)
  }

  fn length(&self) -> f64 {
    self.points.0.distance(self.points.1)
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.points
  }
}


#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
  pub id: Uuid,
  pub center: Point3,
  pub radius: f64,
  pub start: f64,
  pub end: f64,
}

impl Arc {
  pub fn new(center: Point3, radius: f64, start: f64, end: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      center,
      radius,
      start,
      end,
    }
  }

  pub fn split_with_line(&self, _line: &Line) -> Vec<Arc> { vec![] }

  pub fn split_with_arc(&self, _arc: &Arc) -> Vec<Arc> { vec![] }

  pub fn split_with_circle(&self, _circle: &Circle) -> Vec<Arc> { vec![] }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Arc> { vec![] }
}

impl Curve for Arc {
  fn sample(&self, t: f64) -> Point3 {
    let t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y,
      self.center.z + t.cos() * self.radius,
    )
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(120)
  }

  fn length(&self) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }
}


#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
  pub id: Uuid,
  pub center: Point3,
  pub radius: f64,
}

impl Circle {
  pub fn new(center: Point3, radius: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      center,
      radius,
    }
  }

  pub fn split_with_line(&self, _line: &Line) -> Option<(Arc, Arc)> { None }

  pub fn split_with_arc(&self, _arc: &Arc) -> Option<(Arc, Arc)> { None }

  pub fn split_with_circle(&self, _circle: &Circle) -> Option<(Arc, Arc)> { None }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Arc> { vec![] }
}

impl Curve for Circle {
  fn sample(&self, t: f64) -> Point3 {
    let t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y,
      self.center.z + t.cos() * self.radius,
    )
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(120)
  }

  fn length(&self) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }
}


const LUT_STEPS: usize = 10;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BezierSpline {
  pub id: Uuid,
  pub vertices: Vec<Point3>,
  pub lut: Vec<Point3>,
}

impl BezierSpline {
  pub fn new(vertices: Vec<Point3>) -> Self {
    let mut this = Self {
      id: Uuid::new_v4(),
      vertices: vertices,
      lut: vec![],
    };
    this.update();
    this
  }

  pub fn update(&mut self) {
    self.lut = self.tesselate((self.vertices.len() * LUT_STEPS).try_into().unwrap())
  }

  // de Casteljau's algorithm
  fn real_sample(&self, t: f64, vertices: &[Point3]) -> Point3 {
    if vertices.len() == 1 {
      vertices[0]
    } else {
      let len = vertices.len() - 1;
      let mut new_vertices: Vec<Point3> = vec![];
      for i in 0..len {
        new_vertices.push(vertices[i] * (1.0 - t) + (vertices[i + 1] * t).to_vec());
      }
      self.real_sample(t, &new_vertices)
    }
  }

  pub fn split_at(&self, t: f64) -> (Self, Self) {
    let mut left: Vec<Point3> = vec![];
    let mut right: Vec<Point3> = vec![];
    self.real_split(t, &self.vertices, &mut left, &mut right);
    (Self::new(left), Self::new(right))
  }

  fn real_split(&self, t: f64, vertices: &[Point3], left: &mut Vec<Point3>, right: &mut Vec<Point3>) -> Point3 {
    if vertices.len() == 1 {
      let p = vertices[0];
      left.push(p);
      right.push(p);
      p
    } else {
      let len = vertices.len() - 1;
      let mut new_vertices: Vec<Point3> = vec![];
      for i in 0..len {
        if i == 0 { left.push(vertices[i]) }
        if i == len - 1 { right.push(vertices[i + 1]) }
        new_vertices.push(vertices[i] * (1.0 - t) + (vertices[i + 1] * t).to_vec());
      }
      self.real_split(t, &new_vertices, left, right)
    }
  }

  pub fn tangent(&self, t: f64) -> Vec3 {
    self.derive().sample(t).to_vec().normalize()
  }

  // https://stackoverflow.com/questions/25453159/getting-consistent-normals-from-a-3d-cubic-bezier-path
  pub fn normal(&self, t: f64) -> Vec3 {
    let derivative = self.derive();
    let tan = derivative.sample(t).to_vec().normalize();
    let tan2 = (tan + derivative.derive().sample(t).to_vec()).normalize();
    let c = tan2.cross(tan).normalize();
    c.cross(tan).normalize()
  }

  pub fn derive(&self) -> Self {
    let mut derivative: Self = Default::default();
    let len = self.vertices.len() - 1;
    for i in 0..len {
      derivative.vertices[i] = (self.vertices[i + 1] - self.vertices[i].to_vec()) * len as f64;
    }
    derivative
  }

  //XXX Provide exact solution
  pub fn closest_point(&self, point: Point3) -> Point3 {
    let mut mdist = 2_i32.pow(63).into();
    let mut closest = point;
    for &p in &self.lut {
      let d = point.distance(p);
      if point.distance(p) < mdist {
        mdist = d;
        closest = p;
      }
    }
    closest
  }

  pub fn split_with_line(&self, _line: &Line) -> Vec<Self> { vec![] }

  pub fn split_with_arc(&self, _arc: &Arc) -> Vec<BezierSpline> { vec![] }

  pub fn split_with_circle(&self, _circle: &Circle) -> Vec<Self> { vec![] }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Self> { vec![] }
}

impl Curve for BezierSpline {
  fn sample(&self, t: f64) -> Point3 {
    self.real_sample(t, &self.vertices)
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.lut.clone()
  }

  fn length(&self) -> f64 {
    1.0
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.vertices[0], *self.vertices.last().unwrap())
  }
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  #[test]
  fn line_length() {
    let lines = test_data::parallel_lines();
    assert_eq!(lines.0.length(), 1.0);
  }

  // #[test]
  // fn split() {
  //   let lines = test_data::crossing_lines();
  //   let segments = lines.0.split_with(&SketchElement::Line(lines.1));
  //   assert_eq!(segments.len(), 2, "{} segments found instead of 2", segments.len());
  //   assert_eq!(segments[0].length(), 0.5, "Segment had wrong length");
  // }
}
