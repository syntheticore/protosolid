use std::fmt::Debug;

use uuid::Uuid;
use serde::{ Serialize, Deserialize };

use crate::internal::*;
use crate::transform::*;
use crate::geom2d;
use crate::wire::PolyLine;
use crate::surface::SurfaceArea;

pub(crate) mod intersection;
pub use intersection::CurveIntersection;
pub use intersection::CurveIntersectionType;


/// Base trait for all parametric curves.
///
/// Curves are parametrized in the range 0-1.
/// Sampling a generic curve outside this range is considered undefined behaviour.

pub trait Curve: Transformable {
  fn sample(&self, t: f64) -> Point3;
  fn unsample(&self, p: Point3) -> f64; // p is expected to touch the curve
  fn tangent_at(&self, t: f64) -> Vec3;
  fn curvature_at(&self, t: f64) -> f64;
  fn length_between(&self, start: f64, end: f64) -> f64;
  fn tesselate(&self) -> PolyLine;

  fn endpoints(&self) -> (Point3, Point3) {
    (self.sample(0.0), self.sample(1.0))
  }

  fn other_endpoint(&self, point: &Point3) -> Point3 {
    let (start, end) = self.endpoints();
    if point.almost(start) { end } else { start }
  }

  fn tesselate_fixed(&self, steps: u32) -> PolyLine {
    (0..=steps).map(|i| self.sample(i as f64 / steps as f64) ).collect()
  }

  fn tesselate_adaptive(&self, max_deviation: f64, max_angle: Deg<f64>, trims: (f64, f64)) -> PolyLine {
    let mut poly = vec![(trims.0, self.sample(trims.0)), (trims.1, self.sample(trims.1))];
    self.tesselate_adaptive_recurse(&mut poly, 0, max_deviation, max_angle.into());
    poly.iter().map(|pair| pair.1 ).collect()
  }

  fn tesselate_adaptive_recurse(&self, poly: &mut Vec<(f64, Point3)>, index: usize, max_deviation: f64, max_angle: Rad<f64>) {
    let j = index + 1;
    let center = (poly[index].1 + poly[j].1.to_vec()) / 2.0;
    let trims = (poly[index].0, poly[j].0);
    let t = (trims.0 + trims.1) / 2.0;
    let sample = self.sample(t);
    let angle = (sample - poly[index].1).angle(poly[j].1 - sample);
    if sample.distance(center) > max_deviation || angle > max_angle {
      poly.insert(j, (t, sample));
      self.tesselate_adaptive_recurse(poly, j, max_deviation, max_angle);
      self.tesselate_adaptive_recurse(poly, index, max_deviation, max_angle);
    }
  }

  fn is_closed(&self) -> bool {
    let (start, end) = self.endpoints();
    start == end
  }

  fn length(&self) -> f64 {
    self.length_between(0.0, 1.0)
  }

  fn param_at_length(&self, length: f64) -> f64 {
    length / self.length()
  }

  fn midpoint(&self) -> Point3 {
    self.sample(self.param_at_length(self.length() / 2.0))
  }

  //XXX Potentially not correct for every type
  fn closest_point(&self, p: &Point3) -> Point3 {
    self.sample(self.unsample(*p))
  }

  fn contains_point(&self, p: Point3) -> bool {
    let t = self.unsample(p);
    t >= 0.0 && t <= 1.0 && self.sample(t).almost(p)
  }
}


/// All curve types that are not a variant of another BasisCurve type.

pub trait BasisCurve: Curve {
  fn intersect(&self, other: &CurveType) -> Vec<CurveIntersectionType>;
  fn into_enum(self) -> CurveType;
}


/// All curves that can be split at a parameter.
///
/// Provides convenience methods for more complex splitting situations.

pub trait Splittable: BasisCurve + Clone {
  fn split_at(&self, t: f64) -> Option<(Self, Self)> where Self: Sized;

  fn split_with(&self, cutter: &CurveType) -> Option<Vec<CurveType>> {
    let intersections = self.intersect(cutter);
    let intersections: Vec<&CurveIntersection> = intersections.iter().filter_map(|isect|
      isect.get_splitting_intersection()
    ).collect();
    if intersections.len() == 0 { return None }
    let points = intersections.iter().map(|hit| hit.point ).collect();
    self.split_at_points(&points) //XXX use params instead
  }

  fn split_at_points(&self, points: &Vec<Point3>) -> Option<Vec<CurveType>> {
    let params = points.iter().map(|p| self.unsample(*p) ).collect();
    self.split_at_params(params)
  }

  fn split_at_params(&self, mut params: Vec<f64>) -> Option<Vec<CurveType>> {
    if params.len() == 0 { return None }
    params.sort_by(|a, b| a.partial_cmp(b).unwrap() );
    let mut segments = vec![];
    let mut curve = self.clone();
    let mut t_last = 0.0;
    for (i, t) in params.iter().enumerate() {
      let t_new = (t - t_last) / (1.0 - t_last);
      let (first, second) = curve.split_at(t_new).unwrap();
      segments.push(first.into_enum());
      if i == params.len() - 1 {
        segments.push(second.into_enum());
      } else {
        curve = second;
      }
      t_last = *t;
    }
    Some(segments)
  }
}


macro_rules! dispatch_curve {
  ($self:ident) => {
    match $self {
      Self::Line(curve) => curve,
      Self::Arc(curve) => curve,
      Self::Circle(curve) => curve,
      Self::Spline(curve) => curve,
    }
  };
  ($self:ident.$a:ident) => {
    match $self {
      Self::Line(curve) => curve.$a,
      Self::Arc(curve) => curve.$a,
      Self::Circle(curve) => curve.$a,
      Self::Spline(curve) => curve.$a,
    }
  };
  ($self:ident.$a:ident = $b:expr) => {
    match $self {
      Self::Line(curve) => curve.$a = $b,
      Self::Arc(curve) => curve.$a = $b,
      Self::Circle(curve) => curve.$a = $b,
      Self::Spline(curve) => curve.$a = $b,
    }
  };
  ($self:ident.$a:ident($b:expr)) => {
    match $self {
      Self::Line(curve) => curve.$a($b),
      Self::Arc(curve) => curve.$a($b),
      Self::Circle(curve) => curve.$a($b),
      Self::Spline(curve) => curve.$a($b),
    }
  };
}

/// Wrapper enum for all [BasisCurve] types.
///
/// This wrapper is used to pass curves around generically, while still being able to dispatch to their concrete types.
/// All contained types implement [BasisCurve], and can be accessed as such using the [as_curve](Self::as_curve) and [as_curve_mut](Self::as_curve_mut) convenience functions.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CurveType {
  Line(Line),
  Arc(Arc),
  Circle(Circle),
  Spline(Spline),
}

impl CurveType {
  pub fn as_curve(&self) -> &dyn BasisCurve {
    dispatch_curve!(self)
  }

  pub fn as_curve_mut(&mut self) -> &mut dyn BasisCurve {
    dispatch_curve!(self)
  }

  pub fn id(&self) -> Uuid {
    dispatch_curve!(self.id)
  }

  pub fn set_id(&mut self, id: Uuid) {
    dispatch_curve!(self.id = id)
  }

  pub fn split(&self, cutter: &Self) -> Option<Vec<Self>> {
    dispatch_curve!(self.split_with(cutter))
  }

  pub fn split_multi(&self, others: &Vec<Self>) -> Vec<Self> {
    let mut segments = vec![self.clone()];
    for other in others.iter() {
      if self == other { continue } //OPT Compare by ID
      segments = segments.iter().flat_map(|own| {
        own.split(&other).unwrap_or(vec![own.clone()])
      }).collect();
    }
    segments
  }
}


/// Bounded section of another curve.
///
/// Also useful for cheaply inverting the direction of a curve.
///
/// # Examples
/// ```
/// use shapex::*;
///
/// let mut curve = TrimmedCurve::new(
///   Line::new(
///     Point3::origin(),
///     Point3::new(1.0, 1.0, 1.0)
///   ).into_enum()
/// );
/// curve.flip();
/// assert_eq!(curve.sample(1.0), Point3::origin());
/// assert_eq!(curve.trims, (1.0, 0.0));
/// ```

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrimmedCurve {
  pub id: Uuid,
  pub base: CurveType,
  pub trims: (f64, f64),
  pub bounds: (Point3, Point3),
  pub cache: CurveType,
}

impl TrimmedCurve {
  pub fn new(elem: CurveType) -> Self {
    Self {
      id: Uuid::new_v4(),
      base: elem.clone(),
      trims: (0.0, 1.0),
      bounds: elem.as_curve().endpoints(),
      cache: elem,
    }
  }

  pub fn from_bounds(base: CurveType, bounds: (Point3, Point3), cache: CurveType) -> Self {
    let mut this = Self {
      id: Uuid::new_v4(),
      base,
      trims: (0.0, 1.0),
      bounds: (Point3::origin(), Point3::origin()),
      cache,
    };
    this.set_bounds(bounds);
    this
  }

  pub fn set_bounds(&mut self, bounds: (Point3, Point3)) {
    let curve = self.base.as_curve();
    self.trims = (curve.unsample(bounds.0), curve.unsample(bounds.1));
    // Fix circle trims
    if self.trims.0 == self.trims.1 {
      self.trims = (0.0, 1.0);
    }
    self.bounds = bounds;
  }

  pub fn other_bound(&self, p: &Point3) -> Point3 {
    let (start, end) = self.bounds;
    if p.almost(start) { end } else { start }
  }

  pub fn is_forward(&self) -> bool {
    self.trims.0 < self.trims.1
  }

  pub fn flip(&mut self) {
    self.trims = (self.trims.1, self.trims.0);
    self.bounds = (self.bounds.1, self.bounds.0);
  }

  pub fn param_to_base(&self, t: f64) -> f64 {
    self.trims.0 + t * (self.trims.1 - self.trims.0)
  }

  pub fn param_from_base(&self, t_base: f64) -> f64 {
    (t_base - self.trims.0) / (self.trims.1 - self.trims.0)
  }

  fn convert_intersection(&self, intersection: &mut CurveIntersection, other: &Self) {
    intersection.t1 = self.param_from_base(intersection.t1);
    intersection.t2 = other.param_from_base(intersection.t2);
  }

  pub fn intersect(&self, other: &Self) -> Vec<CurveIntersectionType> {
    let intersections = self.base.as_curve().intersect(&other.base);
    intersections.into_iter().map(|intersection| {
      match intersection {
        CurveIntersectionType::Contained //XXX Could now be Touch. Contained needs to store its range
        => intersection,

        CurveIntersectionType::Touch(mut isect)
        | CurveIntersectionType::Pierce(mut isect)
        | CurveIntersectionType::Cross(mut isect)
        | CurveIntersectionType::Extended(mut isect)
        => {
          self.convert_intersection(&mut isect, other);
          CurveIntersectionType::new(isect)
        },
      }
    }).collect()
  }
}

impl Curve for TrimmedCurve {
  fn sample(&self, t: f64) -> Point3 {
    self.base.as_curve().sample(self.param_to_base(t))
  }

  fn unsample(&self, p: Point3) -> f64 {
    self.param_from_base(self.base.as_curve().unsample(p))
  }

  fn tangent_at(&self, t: f64) -> Vec3 {
    let tangent = self.base.as_curve().tangent_at(self.param_to_base(t));
    if self.is_forward() {
      tangent
    } else {
      -tangent
    }
  }

  fn curvature_at(&self, t: f64) -> f64 {
    self.base.as_curve().curvature_at(self.param_to_base(t))
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.base.as_curve().length_between(self.param_to_base(start), self.param_to_base(end))
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_adaptive(0.025, Deg(20.0), (0.0, 1.0))
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.bounds
  }
}

impl Transformable for TrimmedCurve {
  fn transform(&mut self, transform: &Matrix4) {
    self.base.as_curve_mut().transform(transform);
    self.cache.as_curve_mut().transform(transform);
    self.bounds = (transform.transform_point(self.bounds.0), transform.transform_point(self.bounds.1));
  }
}


fn invert_intersections(mut intersections: Vec<CurveIntersectionType>) -> Vec<CurveIntersectionType> {
  for isect in &mut intersections {
    isect.invert();
  }
  intersections
}


/// Finite line segment between two [points](Point3).
///
/// # Examples
/// ```
/// use shapex::*;
///
/// let line = Line::new(Point3::origin(), Point3::new(1.0, 0.0, 0.0));
/// assert_eq!(line.midpoint(), Point3::new(0.5, 0.0, 0.0));
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
}

impl Curve for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn unsample(&self, p: Point3) -> f64 {
    let vec = p - self.points.0;
    (vec).dot(self.tangent()) / self.length()
  }

  fn tangent_at(&self, _t: f64) -> Vec3 {
    self.tangent()
  }

  fn curvature_at(&self, _t: f64) -> f64 {
    0.0
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(1)
  }

  fn is_closed(&self) -> bool { false }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.sample(start).distance(self.sample(end))
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.points
  }
}

impl BasisCurve for Line {
  fn intersect(&self, other: &CurveType) -> Vec<CurveIntersectionType> {
    match other {
      CurveType::Line(line) => intersection::line_line(self, line).map_or(vec![], |isect| vec![isect] ),
      CurveType::Circle(circle) => intersection::line_circle(self, circle),
      CurveType::Arc(arc) => intersection::line_arc(self, arc),
      CurveType::Spline(spline) => intersection::line_spline(self, spline),
    }
  }

  fn into_enum(self) -> CurveType {
    CurveType::Line(self)
  }
}

impl Splittable for Line {
  fn split_at(&self, t: f64) -> Option<(Self, Self)> {
    if EPSILON < t && t < 1.0 - EPSILON {
      let p = self.sample(t);
      Some((Line::new(self.points.0, p), Line::new(p, self.points.1)))
    } else {
      None
    }
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


/// Circular arc between two parameters on a [Circle].
///
/// # Examples
/// ```
/// use shapex::*;
///
/// let arc = Arc::new(Point3::origin(), 1.0, 0.25, 0.75);
/// assert_eq!(arc.midpoint(), Point3::new(0.0, 1.0, 0.0));
/// ```

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arc {
  pub id: Uuid,
  pub plane: Plane,
  pub radius: f64,
  pub bounds: (f64, f64),
}

impl Arc {
  pub fn new(center: Point3, radius: f64, start: f64, end: f64) -> Self {
    let plane = Plane::from_point(center);
    Self::from_plane(plane, radius, start, end)
  }

  pub fn from_plane(plane: Plane, radius: f64, start: f64, end: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      plane,
      radius,
      bounds: (start, end),
    }
  }

  pub fn circle(center: Point3, radius: f64) -> Self {
    Self::new(center, radius, 0.0, 1.0)
  }

  pub fn from_points(p1: Point3, p2: Point3, p3: Point3) -> Result<Self, String> {
    let circle = Circle::from_points(p1, p2, p3)?;
    let t_center = circle.unsample(p2);
    let mut end_params = [circle.unsample(p1), circle.unsample(p3)];
    end_params.sort_by(|a, b| a.partial_cmp(b).unwrap() );
    let bounds = if end_params[0] <= t_center && t_center <= end_params[1] {
      end_params
    } else {
      [end_params[1], end_params[0] + 1.0]
    };
    let mut arc = Self::new(circle.plane.origin, circle.radius, bounds[0], bounds[1]);
    if !p1.almost(arc.sample(0.0)) {
      arc.invert();
    }
    Ok(arc)
  }

  pub fn range(&self) -> f64 {
    self.bounds.1 - self.bounds.0
  }

  pub fn param_to_circle(&self, t: f64) -> f64 {
    let t = self.bounds.0 + t * self.range();
    t
  }

  pub fn param_from_circle(&self, t: f64) -> f64 {
    let mut range = self.bounds.1 - self.bounds.0;
    if range.almost(0.0) {
      range = 1.0;
    }
    (t - self.bounds.0) / range
  }

  pub fn invert(&mut self) {
    self.bounds = (self.bounds.1, self.bounds.0);
  }

  pub fn flip(&mut self) {
    self.bounds = (self.bounds.1 - 1.0, self.bounds.0);
  }

  // fn overflow(t: f64) -> f64 {
  //   if t > 1.0 {
  //     t % 1.0
  //   } else if t < 0.0 {
  //     1.0 + t % 1.0
  //   } else {
  //     t
  //   }
  // }
}

impl Curve for Arc {
  fn sample(&self, t: f64) -> Point3 {
    let mut t = self.param_to_circle(t);
    t *= std::f64::consts::PI * 2.0;
    self.plane.sample(t.cos() * self.radius, t.sin() * self.radius)
  }

  fn unsample(&self, p: Point3) -> f64 {
    let circle = Circle::from_plane(self.plane.clone(), self.radius);
    let mut t = circle.unsample(p);
    // Offset circle param such that the produced arc parameters are in the 0-1 range
    // instead of wrapping into this range, for arcs with bounds < 0 or > 1
    let ordered = sort_tuple2(self.bounds.0, self.bounds.1);
    t = if ordered.1 > 1.0 && t <= ordered.1 % 1.0 + EPSILON {
      t + 1.0
    } else if ordered.0 < 0.0 && 1.0 - t <= ordered.0.abs() % 1.0 + EPSILON {
      t - 1.0
    } else {
      t
    };
    self.param_from_circle(t)
  }

  fn tangent_at(&self, t: f64) -> Vec3 {
    let mut t = self.param_to_circle(t);
    t *= std::f64::consts::PI * 2.0;
    let v = Vec3::new(t.sin(), -t.cos(), 0.0);
    self.plane.as_transform().transform_vector(v)
  }

  fn curvature_at(&self, _t: f64) -> f64 {
    1.0 / self.radius
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed((80.0 * self.range().abs()) as u32)
  }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius * (start - end).abs() * self.range().abs()
  }
}

impl BasisCurve for Arc {
  fn intersect(&self, other: &CurveType) -> Vec<CurveIntersectionType> {
    match other {
      CurveType::Line(line) => invert_intersections(intersection::line_arc(line, self)),
      CurveType::Circle(_circle) => vec![],
      CurveType::Arc(_arc) => vec![],
      CurveType::Spline(_spline) => vec![],
    }
  }

  fn into_enum(self) -> CurveType {
    CurveType::Arc(self)
  }
}

impl Splittable for Arc {
  fn split_at(&self, t: f64) -> Option<(Self, Self)> {
    if EPSILON < t && t < 1.0 - EPSILON {
      let t = self.param_to_circle(t);
      Some((
        Arc::from_plane(self.plane.clone(), self.radius, self.bounds.0, t),
        Arc::from_plane(self.plane.clone(), self.radius, t, self.bounds.1),
      ))
    } else {
      None
    }
  }
}

impl Transformable for Arc {
  fn transform(&mut self, transform: &Matrix4) {
    self.plane.transform(transform);
  }
}


/// A full circle.
///
/// Cannot be split at a single parameter. Yields [Arc]s when split.
/// Sampling the curve at parameters outside the 0-1 range will result in wrapping around the circle.
///
/// # Examples
/// ```
/// use shapex::*;
///
/// let circle = Circle::new(Point3::origin(), 1.0);
/// assert_eq!(circle.diameter(), 2.0);
/// assert_eq!(circle.sample(0.5), circle.sample(1.5));
/// ```

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Circle {
  pub id: Uuid,
  pub plane: Plane,
  pub radius: f64,
}

impl Circle {
  pub fn new(center: Point3, radius: f64) -> Self {
    let plane = Plane::from_point(center);
    Self::from_plane(plane, radius)
  }

  pub fn from_plane(plane: Plane, radius: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      plane,
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
}

impl Curve for Circle {
  fn sample(&self, t: f64) -> Point3 {
    let t = t * std::f64::consts::PI * 2.0;
    self.plane.sample(t.cos() * self.radius, t.sin() * self.radius)
  }

  fn unsample(&self, p: Point3) -> f64 {
    let Point2 { x, y } = self.plane.unsample(p);
    let atan2 = y.atan2(x) / std::f64::consts::PI / 2.0;
    if atan2 < 0.0 {
      1.0 + atan2
    } else {
      atan2
    }
  }

  fn tangent_at(&self, t: f64) -> Vec3 {
    let t = t * std::f64::consts::PI * 2.0;
    let v = Vec3::new(t.sin(), -t.cos(), 0.0);
    self.plane.as_transform().transform_vector(v)
  }

  fn curvature_at(&self, _t: f64) -> f64 {
    1.0 / self.radius
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_fixed(80)
  }

  fn is_closed(&self) -> bool { true }

  fn length_between(&self, start: f64, end: f64) -> f64 {
    self.circumfence() * (start - end).abs()
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }
}

impl BasisCurve for Circle {
  fn intersect(&self, other: &CurveType) -> Vec<CurveIntersectionType> {
    match other {
      CurveType::Line(line) => invert_intersections(intersection::line_circle(line, self)),
      CurveType::Circle(_circle) => vec![],
      CurveType::Arc(_arc) => vec![],
      CurveType::Spline(_spline) => vec![],
    }
  }

  fn into_enum(self) -> CurveType {
    CurveType::Circle(self)
  }
}

impl Splittable for Circle {
  fn split_at(&self, _t: f64) -> Option<(Self, Self)> {
    None
  }

  fn split_at_points(&self, points: &Vec<Point3>) -> Option<Vec<CurveType>> {
    if points.len() >= 2 {
      let mut params: Vec<f64> = points.iter().map(|p| self.unsample(*p) ).collect();
      params.sort_by(|a, b| a.partial_cmp(b).unwrap() );
      let first_arc = Arc::new(self.plane.origin, self.radius, params[0], params[1]);
      let second_arc = Arc::new(self.plane.origin, self.radius, params[1] - 1.0, params[0]);
      if points.len() > 2 {
        let remaining_points = points.iter().skip(2).cloned().collect();
        let mut arcs = vec![first_arc.into_enum()];
        arcs.append(&mut second_arc.split_at_points(&remaining_points).unwrap());
        Some(arcs)
      } else {
        Some(vec![first_arc.into_enum(), second_arc.into_enum()])
      }
    } else {
      None
    }
  }
}

impl SurfaceArea for Circle {
  fn area(&self) -> f64 {
    std::f64::consts::PI * self.radius.powf(2.0)
  }
}

impl Transformable for Circle {
  fn transform(&mut self, transform: &Matrix4) {
    self.plane.transform(transform);
  }
}


/// Non-uniform rational basis spline.
///
/// The shape of the curve is determined by its degree and a set of weighted control vertices, as well as a knot vector,
/// that controls how curve parameters are mapped to control vertices.
///
/// The standard constructor will determine the curve's degree automatically from the number of control vertices supplied
/// and use the highest degree possible up to a maximum of 5. A clamped, but otherwise uniform knot vector is used,
/// such that the parameter range of the actual curve is 0.0 - 1.0.
///
/// # Examples
/// ```
/// use shapex::*;
///
/// let spline = Spline::new(vec![
///   Point3::new(0.0, 0.0, 0.0),
///   Point3::new(0.5, 0.0, 1.0),
///   Point3::new(1.0, 0.0, 0.0),
/// ]);
/// assert_eq!(spline.degree, 2);
/// assert_eq!(spline.tangent_at(0.5), Vec3::new(1.0, 0.0, 0.0));
/// ```

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spline {
  pub id: Uuid,
  pub degree: usize,
  pub controls: Vec<Point3>,
  pub knots: Vec<f64>,
  pub weights: Vec<f64>,
}

impl Spline {
  pub fn new(controls: Vec<Point3>) -> Self {
    let n = controls.len();
    if n < 2 { panic!("Splines need at least two control vertices") }
    let degree = (n - 1).min(5);
    Self {
      id: Uuid::new_v4(),
      degree,
      controls,
      knots: Self::clamped_knots(n, degree),
      weights: vec![1.0; n],
    }
  }

  #[allow(dead_code)]
  fn uniform_knots(n: usize, degree: usize) -> Vec<f64> {
    if degree >= n { return vec![] }
    let num_knots = n + degree + 1;
    (0..num_knots).map(|i| i as f64 ).collect()
  }

  #[allow(dead_code)]
  fn clamped_knots(n: usize, degree: usize) -> Vec<f64> {
    if degree >= n { return vec![] }
    let d = degree + 1;
    [
      vec![0.0; d],
      (1..=n - d).map(|i| i as f64 ).collect(),
      vec![(n - d + 1) as f64; d]
    ].concat()
  }

  fn real_split(&self, t: f64, controls: &[Point3], left: &mut Vec<Point3>, right: &mut Vec<Point3>) -> Point3 {
    if controls.len() == 1 {
      let p = controls[0];
      left.push(p);
      right.push(p);
      p
    } else {
      let len = controls.len() - 1;
      let mut new_controls: Vec<Point3> = vec![];
      for i in 0..len {
        if i == 0 { left.push(controls[i]) }
        if i == len - 1 { right.push(controls[i + 1]) }
        new_controls.push(controls[i] * (1.0 - t) + (controls[i + 1] * t).to_vec());
      }
      self.real_split(t, &new_controls, left, right)
    }
  }

  // https://stackoverflow.com/questions/25453159/getting-consistent-normals-from-a-3d-cubic-bezier-path
  pub fn normal(&self, t: f64) -> Vec3 {
    let derivative = self.derive();
    let tan = derivative.sample(t).to_vec().normalize();
    let tan2 = (tan + derivative.derive().sample(t).to_vec()).normalize();
    let c = tan2.cross(tan);
    c.cross(tan).normalize()
  }

  pub fn derive(&self) -> Self {
    let len = self.controls.len() - 1;
    let controls = (0..len).map(|i| (self.controls[i + 1] - self.controls[i].to_vec()) * len as f64 ).collect();
    Self::new(controls)
  }

  fn unsample_recursive(&self, sample1: (f64, f64), sample2: (f64, f64), target: Point3) -> f64 {
    if sample1.0 == sample2.0 { return sample1.0 }
    let t_center = (sample1.0 + sample2.0) / 2.0;
    let p_center = self.sample(t_center);
    let dist_center = p_center.distance2(target);
    if p_center == target { return t_center }
    let sample_center = (t_center, dist_center);
    if sample1.1 < sample2.1 {
      self.unsample_recursive(sample1, sample_center, target)
    } else {
      self.unsample_recursive(sample_center, sample2, target)
    }
  }
}

impl Curve for Spline {
  fn sample(&self, t: f64) -> Point3 {
    let n = self.controls.len();
    // Remap t to actual curve range
    let low = self.knots[self.degree];
    let high = self.knots[n];
    let t = low + t * (high - low);
    // Find knot interval that contains t
    let span = (self.degree..n).find(|&i| t <= self.knots[i + 1] ).unwrap();
    // Premultiply weights
    let mut homogeneous: Vec<Vec4> = (0..n).map(|i| (self.controls[i].to_vec() * self.weights[i]).extend(self.weights[i]) ).collect();
    // de Boor's algorithm
    for l in 1..=self.degree + 1 {
      for i in (span - self.degree + l..=span).rev() {
        let alpha = (t - self.knots[i]) / (self.knots[i + self.degree + 1 - l] - self.knots[i]);
        homogeneous[i] = (homogeneous[i] * alpha) + (homogeneous[i - 1] * (1.0 - alpha));
      }
    }
    Point3::from_vec(homogeneous[span].truncate() / homogeneous[span].w)
  }

  //XXX Exact solution -> Page 219 (Computer Aided Geometric Design) https://scholarsarchive.byu.edu/cgi/viewcontent.cgi?article=1000&context=facpub
  fn unsample(&self, point: Point3) -> f64 {
    self.unsample_recursive(
      (0.0, self.sample(0.0).distance2(point)),
      (1.0, self.sample(1.0).distance2(point)),
      point,
    )
  }

  fn tangent_at(&self, t: f64) -> Vec3 {
    self.derive().sample(t).to_vec().normalize()
  }

  fn curvature_at(&self, _t: f64) -> f64 {
    todo!()
  }

  fn tesselate(&self) -> Vec<Point3> {
    self.tesselate_adaptive(0.025, Deg(20.0), (0.0, 1.0))
  }

  fn length_between(&self, start: f64, end: f64) -> f64 { //XXX Replace with proper solution
    let mut last_p = self.sample(start);
    self.tesselate_adaptive(0.025, Deg(20.0), (start, end)).iter().fold(0.0, |acc, p| {
      let dist = last_p.distance(*p);
      last_p = *p;
      acc + dist
    })
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.controls[0], *self.controls.last().unwrap())
  }
}

impl BasisCurve for Spline {
  fn intersect(&self, other: &CurveType) -> Vec<CurveIntersectionType> {
    match other {
      CurveType::Line(line) => invert_intersections(intersection::line_spline(line, self)),
      CurveType::Circle(_circle) => vec![],
      CurveType::Arc(_arc) => vec![],
      CurveType::Spline(_spline) => vec![],
    }
  }

  fn into_enum(self) -> CurveType {
    CurveType::Spline(self)
  }
}

impl Splittable for Spline {
  fn split_at(&self, t: f64) -> Option<(Self, Self)> {
    if t.almost(0.0) || t.almost(1.0) { return None }
    let mut left = vec![];
    let mut right = vec![];
    self.real_split(t, &self.controls, &mut left, &mut right);
    Some((Self::new(left), Self::new(right)))
  }
}

impl Transformable for Spline {
  fn transform(&mut self, transform: &Matrix4) {
    for p in  &mut self.controls {
      *p = transform.transform_point(*p);
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
    almost_eq!(arc.sample(0.0), Point3::new(1.0, 0.0, 0.0));
    almost_eq!(arc.sample(1.0), Point3::new(2.0, 0.0, 0.0));
    almost_eq!(arc.unsample(Point3::new(1.0, 0.0, 0.0)), 0.0);
    almost_eq!(arc.unsample(Point3::new(2.0, 0.0, 0.0)), 1.0);
  }

  #[test]
  fn unsample_arc_trimmed() {
    let plane = Plane {
      origin: Point3::new(-32.0, -2.0, 0.0),
      u: Vec3::new(0.0, -1.0, 0.0),
      v: Vec3::new(0.0, 0.0, -1.0),
    };
    let arc = Arc::from_plane(plane, 19.0, 0.5, 1.2);
    almost_eq!(arc.unsample(arc.sample(1.0)), 1.0);
    let mut tcurve = TrimmedCurve::new(arc.into_enum());
    tcurve.set_bounds(tcurve.bounds);
    almost_eq!(tcurve.trims, (0.0, 1.0));
  }

  #[test]
  fn unsample_circle() {
    let circle = Circle::new(Point3::origin(), 1.0);

    almost_eq!(circle.sample(0.000), Point3::new(1.0,  0.0, 0.0));
    almost_eq!(circle.sample(0.125), Point3::new(0.7071067811865475, 0.7071067811865476, 0.0));
    almost_eq!(circle.sample(0.250), Point3::new(0.0,  1.0, 0.0));
    almost_eq!(circle.sample(0.375), Point3::new(-0.7071067811865476, 0.7071067811865475, 0.0));
    almost_eq!(circle.sample(0.500), Point3::new(-1.0, 0.0, 0.0));
    almost_eq!(circle.sample(0.625), Point3::new(-0.7071067811865475, -0.7071067811865477, 0.0));
    almost_eq!(circle.sample(0.750), Point3::new(0.0, -1.0, 0.0));
    almost_eq!(circle.sample(0.875), Point3::new(0.7071067811865477, -0.7071067811865475, 0.0));
    almost_eq!(circle.sample(1.000), Point3::new(1.0,  0.0, 0.0));

    almost_eq!(circle.unsample(Point3::new(1.0, 0.0, 0.0)),                                 0.000);
    almost_eq!(circle.unsample(Point3::new(0.7071067811865475, 0.7071067811865476, 0.0)),   0.125);
    almost_eq!(circle.unsample(Point3::new(0.0, 1.0, 0.0)),                                 0.250);
    almost_eq!(circle.unsample(Point3::new(-0.7071067811865476, 0.7071067811865475, 0.0)),  0.375);
    almost_eq!(circle.unsample(Point3::new(-1.0, 0.0, 0.0)),                                0.500);
    almost_eq!(circle.unsample(Point3::new(-0.7071067811865475, -0.7071067811865477, 0.0)), 0.625);
    almost_eq!(circle.unsample(Point3::new(0.0, -1.0, 0.0)),                                0.750);
    almost_eq!(circle.unsample(Point3::new(0.7071067811865477, -0.7071067811865475, 0.0)),  0.875);
    almost_eq!(circle.unsample(Point3::new(1.0, 0.0, 0.0)),                                 0.000);
  }

  #[test]
  fn unsample_spline() {
    let spline = test_data::s_curve();
    let p = spline.sample(0.5);
    almost_eq!(p, Point3::origin());
    almost_eq!(0.5, spline.unsample(p));
    almost_eq!(0.0, spline.unsample(Point3::new(-1.5, -1.0, 0.0)));
    almost_eq!(1.0, spline.unsample(Point3::new(1.5, 1.0, 0.0)));
  }

  #[test]
  fn flip_trimmed_curve() {
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)).into_enum();
    let bounds = (Point3::new(0.0, 0.6, 0.0), Point3::new(0.0, 0.1, 0.0));
    let mut trimmed = TrimmedCurve::from_bounds(line.clone(), bounds, line.clone());
    almost_eq!(trimmed.endpoints().0, bounds.0);
    almost_eq!(trimmed.endpoints().1, bounds.1);
    trimmed.flip();
    almost_eq!(trimmed.endpoints().0, bounds.1);
    almost_eq!(trimmed.endpoints().1, bounds.0);
  }

  #[test]
  fn negative_trims() {
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)).into_enum();
    let bounds = (Point3::new(0.0, -1.0, 0.0), Point3::new(0.0, 0.5, 0.0));
    let trimmed = TrimmedCurve::from_bounds(line.clone(), bounds, line.clone());
    almost_eq!(trimmed.trims.0, -1.0);
    almost_eq!(trimmed.trims.1, 0.5);
  }

  #[test]
  fn closest_point() {
    let line = Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0));
    let p = Point3::new(1.0, 2.0, 1.0);
    almost_eq!(line.closest_point(&p), Point3::new(0.0, 2.0, 0.0));
  }

  #[test]
  fn split_arc() {
    let arc = Arc::new(Point3::origin(), 1.0, -0.51, 0.12);
    arc.split_at(0.8).unwrap();
  }

  #[test]
  fn circle_to_arc() {
    let arc = Arc::new(Point3::origin(), 1.0, 0.59, 1.24);
    let t = arc.param_from_circle(0.48);
    assert!(!is_between(t, 0.0, 1.0));
    almost_eq!(0.0, arc.unsample(arc.sample(0.0)));
    almost_eq!(1.0, arc.unsample(arc.sample(1.0)));
  }

    #[test]
  fn circle_to_arc2() {
    let arc = Arc::new(Point3::origin(), 1.0, -0.2, 0.7);
    let t = arc.param_from_circle(0.48);
    assert!(is_between(t, 0.0, 1.0));
    almost_eq!(0.0, arc.unsample(arc.sample(0.0)));
    almost_eq!(1.0, arc.unsample(arc.sample(1.0)));
  }
}
