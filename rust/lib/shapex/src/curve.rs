use std::ptr;
use std::convert::TryInto;

use uuid::Uuid;

use crate::base::*;
use crate::geom3d::*;
use intersection::CurveIntersection;

pub mod intersection;


pub trait Curve: Transformable {
  fn sample(&self, t: f64) -> Point3;
  // fn unsample(&self, p: Point3) -> f64;
  fn tesselate(&self) -> PolyLine;
  fn length(&self) -> f64;
  fn endpoints(&self) -> (Point3, Point3);

  fn other_endpoint(&self, point: &Point3) -> Point3 {
    let (start, end) = self.endpoints();
    if point.almost(start) { end } else { start }
  }

  fn tesselate_fixed(&self, steps: i32) -> Vec<Point3> {
    (0..steps + 1).map(|i| {
      self.sample(i as f64 / steps as f64)
    }).collect()
  }

  fn tesselate_adaptive(&self, steps_per_mm: f64) -> Vec<Point3> {
    self.tesselate_fixed((steps_per_mm * self.length()).round() as i32)
  }

  // fn closest_point(&self, p: Point3) -> Point3 {
  //   self.sample(self.unsample(p))
  // }
}

impl std::fmt::Debug for dyn Curve {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


#[derive(Debug, Clone, PartialEq)]
pub enum CurveType {
  Line(Line),
  Arc(Arc),
  Circle(Circle),
  BezierSpline(BezierSpline),
}

impl CurveType {
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
      }.iter().map(|seg| Self::Arc(seg.clone()) ).collect(),

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

  pub fn split_multi(&self, others: &Vec<Self>) -> Vec<Self> {
    let mut segments = vec![self.clone()];
    for other in others.iter() {
      if ptr::eq(self, &*other) { continue }
      segments = segments.iter().flat_map(|own| {
        own.split(&other)
      }).collect();
    }
    segments
  }

  // pub fn trim(&self, at: f64, side: bool) {

  // }
}


#[derive(Debug, Clone, PartialEq)]
pub struct TrimmedCurve {
  pub base: CurveType,
  // pub bounds: (f64, f64),
  pub bounds: (Point3, Point3),
  pub cache: CurveType,
}

impl TrimmedCurve {
  pub fn new(elem: CurveType) -> Self {
    Self {
      bounds: elem.as_curve().endpoints(),
      base: elem.clone(),
      cache: elem,
    }
  }

  pub fn other_bound(&self, p: &Point3) -> Point3 {
    let (start, end) = self.bounds;
    if p.almost(start) { end } else { start }
  }
}

impl Transformable for TrimmedCurve {
  fn transform(&mut self, transform: &Transform) {
    self.base.as_curve_mut().transform(transform);
    self.cache.as_curve_mut().transform(transform);
    self.bounds = (transform.apply(self.bounds.0), transform.apply(self.bounds.1));
  }
}


pub type PolyLine = Vec<Point3>;


/// Elements in a region are connected by their endpoints
/// and already sorted in a closed loop
pub type Region = Vec<TrimmedCurve>;


/// Wires fulfill all properties of regions, but their elements
/// run clockwise and their bounds are ordered in the direction of the loop
pub type Wire = Vec<TrimmedCurve>;


/// A finite line segment between two points
/// # Examples
///
/// ```
/// let line = Line.new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0));
/// assert_eq!(line.midpoint(), Point3::new(0.5, 0.0, 0.0))
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

  pub fn angle_to(&self, other: &Self) -> f64 {
    self.tangent().dot(other.tangent()).acos()
  }

  pub fn split_with(&self, cutter: &CurveType) -> Vec<Line> {
    match intersection::intersect(&self.clone().into_enum(), cutter) {
      CurveIntersection::None
      | CurveIntersection::Contained
      | CurveIntersection::Touch(_)
      | CurveIntersection::Extended(_)
      => vec![self.clone()],

      CurveIntersection::Cross(mut points)
      | CurveIntersection::Pierce(mut points)
      => { //XXX points are not sorted along line
        // Check if intersection goes exactly through endpoint
        if points[0].almost(self.points.0) || points[0].almost(self.points.1) {
          return vec![self.clone()];
        }
        points.push(self.points.1);
        let mut segments = vec![Self::new(self.points.0, points[0])];
        let mut iter = points.iter().peekable();
        loop {
          match (iter.next(), iter.peek()) {
            (Some(p), Some(&next_p)) => segments.push(Self::new(*p, *next_p)),
            _ => break,
          }
        }
        segments
      },
    }
  }

  pub fn into_enum(self) -> CurveType {
    CurveType::Line(self)
  }
}

impl Curve for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(1)
  }

  fn length(&self) -> f64 {
    self.points.0.distance(self.points.1)
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.points
  }
}

impl Transformable for Line {
  fn transform(&mut self, transform: &Transform) {
    self.points = (
      transform.apply(self.points.0),
      transform.apply(self.points.1)
    );
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
      self.center.y + t.cos() * self.radius,
      self.center.z,
    )
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(120)
  }

  fn length(&self) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.sample(0.0), self.sample(1.0))
  }
}

impl Transformable for Arc {
  fn transform(&mut self, transform: &Transform) {
    self.center = transform.apply(self.center);
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

  pub fn diameter(&self) -> f64 {
    self.radius * 2.0
  }

  pub fn circumfence(&self) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius
  }

  pub fn area(&self) -> f64 {
    std::f64::consts::PI * self.radius.powf(2.0)
  }

  pub fn split_with_line(&self, _line: &Line) -> Option<(Arc, Arc)> {
    Some((
      Arc::new(self.center, self.radius, 0.0, 1.0),
      Arc::new(self.center, self.radius, 0.0, 1.0)
    ))
  }

  pub fn split_with_arc(&self, _arc: &Arc) -> Option<(Arc, Arc)> { None }

  pub fn split_with_circle(&self, _circle: &Circle) -> Option<(Arc, Arc)> { None }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Arc> { vec![] }
}

impl Curve for Circle {
  fn sample(&self, t: f64) -> Point3 {
    let t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y + t.cos() * self.radius,
      self.center.z,
    )
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(120)
  }

  fn length(&self) -> f64 {
    self.circumfence()
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }
}

impl Transformable for Circle {
  fn transform(&mut self, transform: &Transform) {
    self.center = transform.apply(self.center);
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
    self.lut = self.tesselate_fixed((self.vertices.len() * LUT_STEPS).try_into().unwrap())
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
    let mut left = vec![];
    let mut right = vec![];
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
      if point.distance2(p) < mdist {
        mdist = d;
        closest = p;
      }
    }
    closest
  }

  pub fn split_with_line(&self, _line: &Line) -> Vec<Self> { vec![self.clone()] }

  pub fn split_with_arc(&self, _arc: &Arc) -> Vec<Self> { vec![] }

  pub fn split_with_circle(&self, _circle: &Circle) -> Vec<Self> { vec![] }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Self> { vec![] }
}

impl Curve for BezierSpline {
  fn sample(&self, t: f64) -> Point3 {
    self.real_sample(t, &self.vertices)
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.lut.clone()
  }

  fn length(&self) -> f64 {
    1.0
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.vertices[0], *self.vertices.last().unwrap())
  }
}

impl Transformable for BezierSpline {
  fn transform(&mut self, transform: &Transform) {
    for v in  &mut self.vertices {
      *v = transform.apply(*v);
    }
  }
}


// Iterate through an unordered list of connected sketch elements
// in an orderly fashion. Returned trim bounds are consistently oriented.
pub struct RegionIterator<'a> {
  region: &'a Vec<TrimmedCurve>,
  elem: Option<&'a TrimmedCurve>,
  first_elem: &'a TrimmedCurve,
  point: Point3,
}

impl<'a> RegionIterator<'a> {
  pub fn new(region: &'a Vec<TrimmedCurve>) -> Self {
    Self {
      region,
      elem: Some(&region[0]),
      first_elem: &region[0],
      point: region[0].bounds.0,
    }
  }
}

impl<'a> Iterator for RegionIterator<'a> {
  type Item = TrimmedCurve;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(elem) = self.elem {
      let mut output = elem.clone();
      self.point = if elem.bounds.0.almost(self.point) {
        elem.bounds.1
      } else {
        // Reverse bounds, such that output item bounds are consistently oriented
        output.bounds = (output.bounds.1, output.bounds.0);
        elem.bounds.0
      };
      self.elem = self.region.iter().find(|other_elem| {
        (other_elem.bounds.0.almost(self.point) || other_elem.bounds.1.almost(self.point))
        && !ptr::eq(*other_elem, elem)
        && !ptr::eq(*other_elem, self.first_elem)
      });
      Some(output)
    } else {
      None
    }
  }
}


// // Iterate through an unordered list of connected sketch elements
// // in an orderly fashion. Returned trim bounds are consistently oriented.
// pub struct WireIterator<'a> {
//   wire: &'a Vec<TrimmedCurve>,
//   elem: Option<&'a TrimmedCurve>,
//   first_elem: &'a TrimmedCurve,
//   point: Point3,
// }

// impl<'a> WireIterator<'a> {
//   pub fn new(wire: &'a Vec<TrimmedCurve>) -> Self {
//     //XXX this assumes elems are already ordered...
//     let bounds = wire[0].bounds;
//     let next_bounds = wire[1].bounds;
//     let first_point = if bounds.0.almost(next_bounds.0) || bounds.0.almost(next_bounds.1) {
//       bounds.1
//     } else {
//       bounds.0
//     };
//     Self {
//       wire,
//       elem: Some(&wire[0]),
//       first_elem: &wire[0],
//       point: first_point,
//       // point: wire[0].bounds.0,
//     }
//   }
// }

// impl<'a> Iterator for WireIterator<'a> {
//   type Item = TrimmedCurve;

//   fn next(&mut self) -> Option<Self::Item> {
//     if let Some(elem) = self.elem {
//       let mut output = elem.clone();
//       self.point = if elem.bounds.0.almost(self.point) {
//         elem.bounds.1
//       } else {
//         // Reverse bounds, such that output item bounds are consistently oriented
//         output.bounds = (output.bounds.1, output.bounds.0);
//         elem.bounds.0
//       };
//       //XXX ... this assumes unordered
//       self.elem = self.wire.iter().find(|other_elem| {
//         (other_elem.bounds.0.almost(self.point) || other_elem.bounds.1.almost(self.point))
//         && !ptr::eq(*other_elem, elem)
//         && !ptr::eq(*other_elem, self.first_elem)
//       });
//       Some(output)
//     } else {
//       None
//     }
//   }
// }


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  #[test]
  fn line_length() {
    let lines = test_data::parallel_lines();
    assert_eq!(lines[0].length(), 1.0);
  }

  #[test]
  fn split_crossing_lines() {
    let lines = test_data::crossing_lines();
    let segments = lines[0].split_with(&lines[1].clone().into_enum());
    assert_eq!(segments.len(), 2, "{} segments found instead of 2", segments.len());
    assert_eq!(segments[0].length(), 0.5, "Segment had wrong length");
  }

  #[test]
  fn split_touching_lines() {
    let lines = test_data::rectangle();
    let segments = lines[0].split_with(&lines[1].clone().into_enum());
    assert_eq!(segments.len(), 1, "{} segments found instead of 1", segments.len());
    assert_eq!(segments[0].length(), 2.0, "Segment had wrong length");
  }

  #[test]
  fn split_t_section1() {
    let lines = test_data::t_section();
    let segments = lines[0].split_with(&lines[1].clone().into_enum());
    assert_eq!(segments.len(), 2, "{} segments found instead of 2", segments.len());
    assert_eq!(segments[0].length(), 1.0, "Segment had wrong length");
    assert_eq!(segments[1].length(), 1.0, "Segment had wrong length");
  }

  #[test]
  fn split_t_section2() {
    let lines = test_data::t_section();
    let segments = lines[1].split_with(&lines[0].clone().into_enum());
    assert_eq!(segments.len(), 1, "{} segments found instead of 1", segments.len());
    assert_eq!(segments[0].length(), 1.0, "Segment had wrong length");
  }

  #[test]
  fn angle_90() {
    let lines = test_data::crossing_lines();
    let angle = lines[0].angle_to(&lines[1]);
    assert_eq!(angle, std::f64::consts::PI / 2.0);
  }

  #[test]
  fn angle_left() {
    let lines = test_data::angle_left();
    let angle = lines[0].angle_to(&lines[1]);
    assert_eq!(angle, std::f64::consts::PI / 2.0);
  }

  #[test]
  fn angle_right() {
    let lines = test_data::angle_right();
    let angle = lines[0].angle_to(&lines[1]);
    assert_eq!(angle, std::f64::consts::PI / 2.0);
  }
}
