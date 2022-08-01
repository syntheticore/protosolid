use crate::internal::*;
use crate::solid::*;
use crate::geom3d;
use crate::surface::intersection;
use crate::surface::SurfaceType;


pub fn extrude(profile: &Profile, distance: f64) -> Result<Compound, String> {
  let wire = profile.rings[0].clone();
  let vec = profile.plane.normal() * distance;
  let mut solid = Solid::lamina(wire, PlanarSurface::new(profile.plane.clone()).into_enum());
  let shell = &mut solid.shells[0];
  let is_forward = distance >= 0.0;
  let face = if is_forward {
    shell.faces.last()
  } else {
    shell.faces.first()
  }.unwrap().clone();
  let transform = Matrix4::from_translation(vec);
  shell.sweep(
    &face,
    &transform,
    |point| {
      Line::new(point + vec, point).into_enum()
    },
    |tcurve| {
      match &tcurve.base {
        CurveType::Line(_)
        => PlanarSurface::new(Plane::from_triangle(
          tcurve.bounds.0,
          tcurve.bounds.0 + vec,
          tcurve.bounds.1,
        )).into_enum(),

        CurveType::Circle(circle)
        => RevolutionSurface::cylinder(Axis::new(circle.plane.origin, vec), circle.radius, vec.magnitude()).into_enum(),

        CurveType::Arc(arc)
        => {
          let axis = if is_forward {
            Axis::new(arc.plane.origin, vec)
          } else {
            Axis::new(arc.plane.origin + vec, -vec)
          };
          let mut surface = RevolutionSurface::cylinder(axis, arc.radius, vec.magnitude());
          surface.u_bounds = arc.bounds;
          if tcurve.is_forward() != is_forward {
            surface.flip();
          }
          surface.into_enum()
        },

        CurveType::Spline(spline)
        => {
          let mut surface = SplineSurface::tabulated(spline, vec);
          if tcurve.is_forward() {
            surface.flip();
          }
          surface.into_enum()
        },
      }
    }
  );
  if distance > 200.0 {
    Err(format!("Maximum extrusion distance exceeded"))
  } else {
    Ok(solid.into_compound())
  }
}


pub fn revolve(profile: &Profile, mut axis: geom3d::Axis, angle: Deg<f64>) -> Result<Compound, String> {
  let wire = profile.rings[0].clone();
  let mut solid = Solid::lamina(wire, PlanarSurface::new(profile.plane.clone()).into_enum());
  let shell = &mut solid.shells[0];
  if axis.direction.dot(profile.plane.u).signum() < 0.0 {
    axis.flip();
  }
  let is_forward = angle >= Deg(0.0);
  let face = if is_forward {
    shell.faces.last()
  } else {
    shell.faces.first()
  }.unwrap().clone();
  let transform = geom3d::rotation_about_axis(&axis, angle);
  shell.sweep(
    &face,
    &transform,
    |point| {
      let p_axis = axis.closest_point(point);
      let radius = p_axis.distance(point);
      let mut plane: Plane = (&axis).into();
      plane.origin = p_axis;
      let mut arc = Arc::from_plane(plane, radius, 0.0, 1.0);
      let t = arc.unsample(&point);
      arc.bounds.0 = t;
      arc.bounds.1 = t + (angle / Deg(360.0));
      arc.into_enum()
    },
    |tcurve| {
      let mut tcurve = tcurve.clone();
      if is_forward {
        tcurve.flip();
      }
      let mut surface = RevolutionSurface::with_bounds(axis.clone(), tcurve.clone(), (0.0, angle / Deg(360.0)));
      if !is_forward {
        surface.flip();
      }
      surface.into_enum()
    }
  );
  Ok(solid.into_compound())
}


pub fn draft(faces: &Vec<Ref<Face>>, fixed_plane: &Plane, angle: Deg<f64>) -> Result<(), String> {
  for face in faces {
    let mut face = face.borrow_mut();
    match &face.surface {
      SurfaceType::Planar(plane) => {
        if let Some(intersection) = intersection::plane_plane(&plane.plane, fixed_plane) {
          if let Some(line) = intersection.get_line() {
            let axis = Axis::from_points(line.endpoints());
            face.surface.as_surface_mut().rotate_about_axis(&axis, angle);
          }
        }
      },
      SurfaceType::Revolution(_) => todo!(),
      SurfaceType::Spline(_) => todo!(),
    }
  }
  Ok(())
}


pub fn make_cube(dx: f64, dy: f64, dz: f64) -> Result<Compound, String> {
  let mut points = vec![
    Point3::new(0.0, 0.0, 0.0),
    Point3::new(dx, 0.0, 0.0),
    Point3::new(dx, dy, 0.0),
    Point3::new(0.0, dy, 0.0),
  ];
  let m = Matrix4::from_angle_z(Deg(45.0));
  points = points.into_iter().map(|p| m.transform_point(p) ).collect();
  let mut wire = Wire::new(vec![]);
  let mut iter = points.iter().peekable();
  while let Some(&p) = iter.next() {
    let next = if let Some(&next) = iter.peek() {
      next
    } else {
      &points[0]
    };
    wire.push(TrimmedCurve::new(Line::new(p, *next).into_enum()));
  }
  extrude(&Profile::new(Plane::new(), vec![wire]), dz)
}


pub fn make_cylinder(radius: f64, height: f64) -> Result<Compound, String> {
  let wire = Wire::new(vec![
    TrimmedCurve::new(Circle::new(Point3::origin(), radius).into_enum())
  ]);
  extrude(&Profile::new(Plane::new(), vec![wire]), height)
}


#[cfg(test)]
mod tests {
  use super::*;
  // use crate::test_data;

  #[test]
  fn cube() {
    let cube = &make_cube(1.5, 1.5, 1.5).unwrap().solids[0];
    let shell = &cube.shells[0];
    println!("\nCube finished");
    shell.print();
    // panic!("Test trap");
    assert_eq!(shell.vertices.len(), 8);
    assert_eq!(shell.edges.len(), 12);
    assert_eq!(shell.faces.len(), 6);
  }

  #[test]
  #[ignore]
  fn cylinder() {
    let cube = &make_cylinder(1.0, 1.0).unwrap().solids[0];
    let shell = &cube.shells[0];
    println!("\nCylinder finished");
    shell.print();
    assert_eq!(shell.vertices.len(), 2);
    assert_eq!(shell.edges.len(), 3);
    assert_eq!(shell.faces.len(), 3);
    assert_eq!(shell.faces[0].borrow().outer_ring.borrow().iter().count(), 1);
    assert_eq!(shell.faces[1].borrow().outer_ring.borrow().iter().count(), 1);
    assert_eq!(shell.faces[2].borrow().outer_ring.borrow().iter().count(), 4);
    // panic!("Test trap");
  }
}
