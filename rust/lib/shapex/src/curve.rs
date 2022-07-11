use std::convert::TryInto;

use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::base::*;
use crate::transform::*;
use crate::geom2d;
use crate::intersection;
use crate::intersection::CurveIntersection;
use crate::intersection::CurveIntersectionType;

// use crate::log;


pub trait Curve: Transformable {
  fn sample(&self, t: f64) -> Point3;
  fn unsample(&self, p: &Point3) -> f64; // p is expected to touch the curve
  fn length_between(&self, start: f64, end: f64) -> f64;
  fn tesselate(&self) -> PolyLine;
  fn into_enum(self) -> CurveType;

  fn endpoints(&self) -> (Point3, Point3) {
    (self.sample(0.0), self.sample(1.0))
  }

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

  fn length(&self) -> f64 {
    self.length_between(0.0, 1.0)
  }

  fn param_at_length(&self, length: f64) -> f64 {
    length / self.length()
  }

  fn midpoint(&self) -> Point3 {
    self.sample(0.5)
  }

  //XXX Potentially not correct for every type
  fn closest_point(&self, p: &Point3) -> Point3 {
    self.sample(self.unsample(p))
  }

  fn is_point_on_curve(&self, p: Point3) -> bool {
    let t = self.unsample(&p);
    t >= 0.0 && t <= 1.0
  }
}

impl std::fmt::Debug for dyn Curve {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

  pub fn get_id(&self) -> Uuid {
    match self {
      Self::Line(line) => line.id,
      Self::Arc(arc) => arc.id,
      Self::Circle(circle) => circle.id,
      Self::BezierSpline(spline) => spline.id,
    }
  }

  pub fn set_id(&mut self, id: Uuid) {
    match self {
      Self::Line(line) => line.id = id,
      Self::Arc(arc) => arc.id = id,
      Self::Circle(circle) => circle.id = id,
      Self::BezierSpline(spline) => spline.id = id,
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
      }.iter().map(|seg| seg.clone().into_enum() ).collect(),

      // Circle
      Self::Circle(circle) => match cutter {
        Self::Line(cutter)
          => if let Some((arc_l, arc_r)) = circle.split_with_line(cutter) {
            vec![arc_l.into_enum(), arc_r.into_enum()]
          } else { vec![self.clone()] },

        Self::Arc(cutter)
          => if let Some((arc_l, arc_r)) = circle.split_with_arc(cutter) {
            vec![arc_l.into_enum(), arc_r.into_enum()]
          } else { vec![self.clone()] },

        Self::Circle(cutter)
          => if let Some((arc_l, arc_r)) = circle.split_with_circle(cutter) {
            vec![arc_l.into_enum(), arc_r.into_enum()]
          } else { vec![self.clone()] },

        Self::BezierSpline(cutter)
          => {
            let arcs = circle.split_with_spline(cutter);
            if arcs.len() > 0 {
              arcs.iter().map(|seg| seg.clone().into_enum() ).collect()
            } else {
              vec![self.clone()]
            }
          },
      }

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
      if self == other { continue } //OPT Compare by ID
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
  pub id: Uuid,
  pub base: CurveType,
  pub trims: (f64, f64),
  pub bounds: (Point3, Point3),
  pub cache: CurveType,
  pub is_forward: bool
}

impl TrimmedCurve {
  pub fn new(elem: CurveType) -> Self {
    Self {
      id: Uuid::new_v4(),
      base: elem.clone(),
      trims: (0.0, 1.0),
      bounds: elem.as_curve().endpoints(),
      cache: elem,
      is_forward: true,
    }
  }

  pub fn from_bounds(base: CurveType, bounds: (Point3, Point3), cache: CurveType) -> Self {
    let mut this = Self {
      id: Uuid::new_v4(),
      base,
      trims: (0.0, 1.0),
      bounds: (Point3::origin(), Point3::origin()),
      cache,
      is_forward: true,
    };
    this.set_bounds(bounds);
    this
  }

  pub fn set_bounds(&mut self, bounds: (Point3, Point3)) {
    let curve = self.base.as_curve();
    let trims = (curve.unsample(&bounds.0), curve.unsample(&bounds.1));
    self.is_forward = true;
    self.trims = if trims.0 <= trims.1 {
      (trims.0, trims.1)
    } else {
      self.is_forward = false;
      (1.0 - trims.0, 1.0 - trims.1)
    };
    self.bounds = bounds;
  }

  pub fn other_bound(&self, p: &Point3) -> Point3 {
    let (start, end) = self.bounds;
    if p.almost(start) { end } else { start }
  }

  pub fn flip(&mut self) {
    self.trims = (1.0 - self.trims.1, 1.0 - self.trims.0);
    self.bounds = (self.bounds.1, self.bounds.0);
    self.is_forward = !self.is_forward
  }

  fn param_to_base(&self, t: f64) -> f64 {
    let out = self.trims.0 + t * (self.trims.1 - self.trims.0);
    if self.is_forward { out } else { 1.0 - out }
  }

  fn param_from_base(&self, t_base: f64) -> f64 {
    let t_base = if self.is_forward { t_base } else { 1.0 - t_base };
    (t_base - self.trims.0) / (self.trims.1 - self.trims.0)
  }

  fn convert_intersection(&self, mut intersection: CurveIntersection) -> (CurveIntersection, bool) {
    let t = self.param_from_base(intersection.t);
    intersection.t = t;
    (intersection, t >= 0.0 && t <= 1.0)
  }

  pub fn intersect(&self, other: &Self) -> CurveIntersectionType {
    let mut intersection = intersection::intersect(&self.base, &other.base);
    match intersection {
      CurveIntersectionType::None
      | CurveIntersectionType::Contained //XXX Needs to be checked
        => intersection,
      CurveIntersectionType::Extended(ref mut isects)
        => {
          *isects = isects.iter()
          .map(|isect| self.convert_intersection(isect.clone()).0 )
          .collect();
          intersection
        },
      CurveIntersectionType::Touch(isect)
        => {
          let converted = self.convert_intersection(isect);
          if converted.1 {
            CurveIntersectionType::Touch(converted.0)
          } else {
            CurveIntersectionType::Extended(vec![converted.0])
          }
        },
      CurveIntersectionType::Pierce(ref mut isects)
      | CurveIntersectionType::Cross(ref mut isects)
        => {
          let converted: Vec<(CurveIntersection, bool)> = isects.iter().map(|isect|
            self.convert_intersection(isect.clone())
          ).collect();
          let filtered: Vec<CurveIntersection> = converted.iter().filter_map(|result|
            if result.1 {
              Some(result.0.clone())
            } else { None }
          ).collect();
          if filtered.len() > 0 {
            *isects = filtered;
            intersection
          } else {
            CurveIntersectionType::Extended(converted.into_iter().map(|result| result.0 ).collect())
          }
        },
    }
  }
}

impl Transformable for TrimmedCurve {
  fn transform(&mut self, transform: &Matrix4) {
    self.base.as_curve_mut().transform(transform);
    self.cache.as_curve_mut().transform(transform);
    self.bounds = (transform.transform_point(self.bounds.0), transform.transform_point(self.bounds.1));
  }
}

impl Curve for TrimmedCurve {
  fn sample(&self, t: f64) -> Point3 {
    self.base.as_curve().sample(self.param_to_base(t))
  }

  fn unsample(&self, p: &Point3) -> f64 {
    self.param_from_base(self.base.as_curve().unsample(p))
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(4)
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.base.as_curve().length_between(self.param_to_base(start), self.param_to_base(end))
  }

  fn into_enum(self) -> CurveType {
    self.base //XXX should actually trim the curve
  }
}


pub type PolyLine = Vec<Point3>;


/// Elements in a region are sorted in a closed loop and connected by their endpoints
pub type Region = Vec<TrimmedCurve>;


/// Wires fulfill all properties of regions, but their element's
/// bounds are ordered in the direction of the loop
pub type Wire = Vec<TrimmedCurve>;


/// Profiles contain one or more wires, representing the outer and inner rings
/// The outer ring runs counter-clockwise and inner rings run clockwise
pub type Profile = Vec<Wire>;


/// A finite line segment between two points
/// # Examples
///
/// ```
/// use shapex::*;
/// let line = Line::new(Point3::origin(), Point3::new(1.0, 0.0, 0.0));
/// assert_eq!(line.midpoint(), Point3::new(0.5, 0.0, 0.0))
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

  pub fn tangent(&self) -> Vec3 {
    (self.points.1 - self.points.0).normalize()
  }

  pub fn angle_to(&self, other: &Self) -> f64 {
    self.tangent().dot(other.tangent()).acos()
  }

  pub fn split_with(&self, cutter: &CurveType) -> Vec<Line> {
    match intersection::intersect(&self.clone().into_enum(), cutter) {
      intersection::CurveIntersectionType::None
      | intersection::CurveIntersectionType::Contained
      | intersection::CurveIntersectionType::Touch(_)
      | intersection::CurveIntersectionType::Extended(_)
      => vec![self.clone()],

      intersection::CurveIntersectionType::Cross(hits)
      | intersection::CurveIntersectionType::Pierce(hits)
      => { //XXX points are not sorted along line
        let mut points: Vec<Point3> = hits.iter().map(|hit| hit.point ).collect();
        // Are we piercing or being pierced?
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
}

impl Curve for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn unsample(&self, p: &Point3) -> f64 {
    (p - self.points.0).magnitude() / self.length()
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(1)
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.sample(start).distance(self.sample(end))
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.points
  }

  fn into_enum(self) -> CurveType {
    CurveType::Line(self)
  }
}

impl Transformable for Line {
  fn transform(&mut self, transform: &Matrix4) {
    self.points = (
      transform.transform_point(self.points.0),
      transform.transform_point(self.points.1)
    );
  }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arc {
  pub id: Uuid,
  pub center: Point3,
  pub normal: Vec3,
  pub radius: f64,
  pub bounds: (f64, f64),
}

impl Arc {
  pub fn new(center: Point3, radius: f64, start: f64, end: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      center,
      normal: Vec3::unit_z(),
      radius,
      bounds: (start, end),
    }
  }

  pub fn from_points(p1: Point3, p2: Point3, p3: Point3) -> Result<Self, String> {
    let circle = Circle::from_points(p1, p2, p3)?;
    Ok(Self::new(circle.center, circle.radius, circle.unsample(&p1), circle.unsample(&p3)))
  }

  pub fn split_with_line(&self, _line: &Line) -> Vec<Arc> { vec![self.clone()] }

  pub fn split_with_arc(&self, _arc: &Arc) -> Vec<Arc> { vec![self.clone()] }

  pub fn split_with_circle(&self, _circle: &Circle) -> Vec<Arc> { vec![] }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Arc> { vec![] }
}

impl Curve for Arc {
  fn sample(&self, mut t: f64) -> Point3 {
    t = 1.0 - t;
    let range = self.bounds.1 - self.bounds.0;
    t = self.bounds.0 + t * range;
    t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y + t.cos() * self.radius,
      self.center.z,
    )
  }

  fn unsample(&self, p: &Point3) -> f64 {
    let circle = Circle::new(self.center, self.radius);
    let param = circle.unsample(p);
    let range = self.bounds.1 - self.bounds.0;
    1.0 - (param - self.bounds.0) / range
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(60)
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius * (start - end).abs()
  }

  fn into_enum(self) -> CurveType {
    CurveType::Arc(self)
  }
}

impl Transformable for Arc {
  fn transform(&mut self, transform: &Matrix4) {
    self.center = transform.transform_point(self.center);
    self.normal = transform.transform_vector(self.normal);
  }
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Circle {
  pub id: Uuid,
  pub center: Point3,
  pub normal: Vec3,
  pub radius: f64,
}

impl Circle {
  pub fn new(center: Point3, radius: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      center,
      normal: Vec3::unit_z(),
      radius,
    }
  }

  pub fn from_points(p1: Point3, p2: Point3, p3: Point3) -> Result<Self, String> {
    let d1 = Vec3::new(p2.y - p1.y, p1.x - p2.x, 0.0);
    let d2 = Vec3::new(p3.y - p2.y, p2.x - p3.x, 0.0);
    let k = geom2d::cross_2d(d2, d1);
    if k.almost(0.0) {
      return Err("Points may not lie on the same line".to_string());
    }
    let s1 = (p1 + p2.to_vec()) / 2.0;
    let s2 = (p2 + p3.to_vec()) / 2.0;
    let l = d1.x * (s2.y - s1.y) - d1.y * (s2.x - s1.x);
    let m = l / k;
    let center = s2 + d2 * m;
    let d = center - p1;
    let radius = (d.x * d.x + d.y * d.y).sqrt();
    Ok(Self::new(center, radius))
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

  pub fn split_with_line(&self, line: &Line) -> Option<(Arc, Arc)> {
    let intersection = intersection::line_circle(line, self);
    match intersection {
      intersection::CurveIntersectionType::Cross(hits) => {
        if hits.len() == 2 {
          let t1 = self.unsample(&hits[0].point);
          let t2 = self.unsample(&hits[1].point);
          Some((
            Arc::new(self.center, self.radius, t1, t2),
            Arc::new(self.center, self.radius, t2, t1),
          ))
        } else {
          None
        }
      },
      _ => None,
    }
  }

  pub fn split_with_arc(&self, _arc: &Arc) -> Option<(Arc, Arc)> { None }

  pub fn split_with_circle(&self, _circle: &Circle) -> Option<(Arc, Arc)> { None }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Arc> { vec![] }
}

impl Curve for Circle {
  fn sample(&self, t: f64) -> Point3 {
    let t = 1.0 - t;
    let t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y + t.cos() * self.radius,
      self.center.z,
    )
  }

  fn unsample(&self, p: &Point3) -> f64 {
    let p = p - self.center;
    let atan2 = p.x.atan2(p.y) / std::f64::consts::PI / 2.0;
    1.0 - if atan2 < 0.0 {
      1.0 + atan2
    } else {
      atan2
    }
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(80)
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.circumfence() * (start - end).abs()
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }

  fn into_enum(self) -> CurveType {
    CurveType::Circle(self)
  }
}

impl Transformable for Circle {
  fn transform(&mut self, transform: &Matrix4) {
    self.center = transform.transform_point(self.center);
    self.normal = transform.transform_vector(self.normal);
  }
}


const LUT_STEPS: usize = 10;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

  fn unsample_recursive(&self, sample1: (f64, f64), sample2: (f64, f64), target: &Point3) -> f64 {
    if sample1.0.almost(sample2.0) { return sample1.0 }
    let t_center = (sample1.0 + sample2.0) / 2.0;
    let p_center = self.sample(t_center);
    let dist_center = p_center.distance2(*target);
    if p_center.almost(*target) { return t_center }
    let sample_center = (t_center, dist_center);
    if sample1.1 < sample2.1 {
      self.unsample_recursive(sample1, sample_center, target)
    } else {
      self.unsample_recursive(sample_center, sample2, target)
    }
  }

  pub fn split_with_line(&self, _line: &Line) -> Vec<Self> { vec![self.clone()] }

  pub fn split_with_arc(&self, _arc: &Arc) -> Vec<Self> { vec![self.clone()] }

  pub fn split_with_circle(&self, _circle: &Circle) -> Vec<Self> { vec![self.clone()] }

  pub fn split_with_spline(&self, _spline: &BezierSpline) -> Vec<Self> { vec![self.clone()] }
}

impl Curve for BezierSpline {
  fn sample(&self, t: f64) -> Point3 {
    self.real_sample(t, &self.vertices)
  }

  //XXX Exact solution -> Page 219 https://scholarsarchive.byu.edu/cgi/viewcontent.cgi?article=1000&context=facpub
  fn unsample(&self, point: &Point3) -> f64 {
    self.unsample_recursive(
      (0.0, self.sample(0.0).distance2(*point)),
      (1.0, self.sample(1.0).distance2(*point)),
      point,
    )
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.lut.clone()
  }

  fn length_between(&self, _start: f64, _end: f64) -> f64 { //XXX use bounds
    let mut last_p = self.lut[0];
    self.lut.iter().fold(0.0, |acc, p| {
      let dist = last_p.distance(*p);
      last_p = *p;
      acc + dist
    })
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.vertices[0], *self.vertices.last().unwrap())
  }

  fn param_at_length(&self, length: f64) -> f64 {
    length / self.length() //XXX take non uniform chord lengths into account
  }

  fn midpoint(&self) -> Point3 {
    self.sample(self.param_at_length(self.length() / 2.0))
  }

  fn into_enum(self) -> CurveType {
    CurveType::BezierSpline(self)
  }
}

impl Transformable for BezierSpline {
  fn transform(&mut self, transform: &Matrix4) {
    for v in  &mut self.vertices {
      *v = transform.transform_point(*v);
    }
  }
}


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

  #[test]
  fn unsample_arc() {
    let arc = Arc::from_points(
      Point3::new(1.0, 0.0, 0.0),
      Point3::new(1.5, 0.1, 0.0),
      Point3::new(2.0, 0.0, 0.0),
    ).unwrap();
    almost_eq(arc.sample(0.0), Point3::new(1.0, 0.0, 0.0));
    almost_eq(arc.sample(1.0), Point3::new(2.0, 0.0, 0.0));
    almost_eq(arc.unsample(&Point3::new(1.0, 0.0, 0.0)), 0.0);
    almost_eq(arc.unsample(&Point3::new(2.0, 0.0, 0.0)), 1.0);
  }

  #[test]
  fn unsample_circle() {
    let circle = Circle::new(Point3::origin(), 1.0);

    almost_eq(circle.sample(0.000), Point3::new(0.0,  1.0, 0.0));
    almost_eq(circle.sample(0.125), Point3::new(0.7071067811865475, 0.7071067811865476, 0.0));
    almost_eq(circle.sample(0.250), Point3::new(1.0,  0.0, 0.0));
    almost_eq(circle.sample(0.375), Point3::new(0.7071067811865476, -0.7071067811865475, 0.0));
    almost_eq(circle.sample(0.500), Point3::new(0.0, -1.0, 0.0));
    almost_eq(circle.sample(0.625), Point3::new(-0.7071067811865475, -0.7071067811865477, 0.0));
    almost_eq(circle.sample(0.750), Point3::new(-1.0, 0.0, 0.0));
    almost_eq(circle.sample(0.875), Point3::new(-0.7071067811865477, 0.7071067811865475, 0.0));
    almost_eq(circle.sample(1.000), Point3::new(0.0,  1.0, 0.0));

    almost_eq(circle.unsample(&Point3::new(0.0, 1.0, 0.0)),                                 0.000);
    almost_eq(circle.unsample(&Point3::new(0.7071067811865475, 0.7071067811865476, 0.0)),   0.125);
    almost_eq(circle.unsample(&Point3::new(1.0, 0.0, 0.0)),                                 0.250);
    almost_eq(circle.unsample(&Point3::new(0.7071067811865476, -0.7071067811865475, 0.0)),  0.375);
    almost_eq(circle.unsample(&Point3::new(0.0, -1.0, 0.0)),                                0.500);
    almost_eq(circle.unsample(&Point3::new(-0.7071067811865475, -0.7071067811865477, 0.0)), 0.625);
    almost_eq(circle.unsample(&Point3::new(-1.0, 0.0, 0.0)),                                0.750);
    almost_eq(circle.unsample(&Point3::new(-0.7071067811865477, 0.7071067811865475, 0.0)),  0.875);
    almost_eq(circle.unsample(&Point3::new(0.0, 1.0, 0.0)),                                 0.000);
  }

  #[test]
  fn unsample_spline() {
    let spline = test_data::s_curve();
    let p = spline.sample(0.5);
    assert_eq!(p, Point3::origin());
    assert_eq!(0.5, spline.unsample(&p));
  }

  #[test]
  fn flip_trimmed_curve() {
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)).into_enum();
    let bounds = (Point3::new(0.0, 0.6, 0.0), Point3::new(0.0, 0.1, 0.0));
    let mut trimmed = TrimmedCurve::from_bounds(line.clone(), bounds, line.clone());
    println!("As constructed: {:#?}", trimmed);
    println!("{:#?}", trimmed.endpoints());
    almost_eq(trimmed.endpoints().0, bounds.0);
    almost_eq(trimmed.endpoints().1, bounds.1);
    trimmed.flip();
    println!("Flipped: {:#?}", trimmed);
    println!("{:#?}", trimmed.endpoints());
    almost_eq(trimmed.endpoints().0, bounds.1);
    almost_eq(trimmed.endpoints().1, bounds.0);
  }
}
