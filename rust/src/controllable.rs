use shapex::*;


pub trait Controllable {
  fn get_handles(&self) -> Vec<Point3>;
  fn set_handles(&mut self, handles: Vec<Point3>);
  fn get_snap_points(&self) -> Vec<Point3>;
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
    vec![endpoints.0, endpoints.1]
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    if let Ok(mut copy) = Self::from_points(handles[0], handles[1], handles[2]) {
      copy.id = self.id;
      *self = copy;
    }
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


impl Controllable for Spline {
  fn get_handles(&self) -> Vec<Point3> {
    self.controls.clone()
  }

  fn set_handles(&mut self, handles: Vec<Point3>) {
    let mut copy = Self::new(handles);
    copy.id = self.id;
    *self = copy;
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
    CurveType::Spline(spline) => spline,
  }
}

pub fn as_controllable_mut(elem: &mut CurveType) -> &mut dyn Controllable {
  match elem {
    CurveType::Line(line) => line,
    CurveType::Arc(arc) => arc,
    CurveType::Circle(circle) => circle,
    CurveType::Spline(spline) => spline,
  }
}
