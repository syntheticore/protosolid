use std::convert::TryInto;

use shapex::*;


pub trait Controllable {
  fn get_handles(&self) -> Vec<Point3>;
  fn set_handles(&mut self, handles: Vec<Point3>);
  fn get_snap_points(&self) -> Vec<Point3>;

  fn set_initial_handles(&mut self, handles: Vec<Point3>) -> Result<(), String> {
    self.set_handles(handles);
    Ok(())
  }
}


impl Controllable for Line {
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


impl Controllable for Arc {
  fn get_handles(&self) -> Vec<Point3> {
    let endpoints = self.endpoints();
    vec![self.plane.origin, endpoints.0, endpoints.1]
  }

  // Three points on arc
  fn set_initial_handles(&mut self, handles: Vec<Point3>) -> Result<(), String> {
    let [p1, p2, p3]: [Point3; 3] = handles.try_into().unwrap();
    let circle = Circle::from_points(p1, p2, p3)?;
    self.plane.origin = circle.plane.origin;
    self.radius = circle.radius;
    self.bounds.0 = circle.unsample(&p1);
    self.bounds.1 = circle.unsample(&p3);
    Ok(())
  }

  // Endpoints + center
  fn set_handles(&mut self, handles: Vec<Point3>) {
    let [center, start, end]: [Point3; 3] = handles.try_into().unwrap();
    self.plane.origin = center;
    self.radius = (start - center).magnitude();
    let circle = Circle::new(self.plane.origin, self.radius);
    self.bounds.0 = circle.unsample(&start);
    self.bounds.1 = circle.unsample(&end);
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    let endpoints = self.endpoints();
    vec![self.plane.origin, endpoints.0, endpoints.1, self.midpoint()]
  }
}


impl Controllable for Circle {
  fn get_handles(&self) -> Vec<Point3> {
    vec![self.plane.origin]
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.plane.origin = handles[0];
    if handles.len() > 1 {
      let p = handles[1];
      self.radius = handles[0].distance(p);
    }
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    vec![self.plane.origin]
  }
}


impl Controllable for BezierSpline {
  fn get_handles(&self) -> Vec<Point3> {
    self.vertices.clone()
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    self.vertices = handles;
  }

  fn get_snap_points(&self) -> Vec<Point3> {
    self.get_handles()
  }
}


pub fn as_controllable(elem: &CurveType) -> &dyn Controllable {
  match elem {
    CurveType::Line(line) => line,
    CurveType::Arc(arc) => arc,
    CurveType::Circle(circle) => circle,
    CurveType::BezierSpline(spline) => spline,
  }
}

pub fn as_controllable_mut(elem: &mut CurveType) -> &mut dyn Controllable {
  match elem {
    CurveType::Line(line) => line,
    CurveType::Arc(arc) => arc,
    CurveType::Circle(circle) => circle,
    CurveType::BezierSpline(spline) => spline,
  }
}
