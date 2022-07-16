use crate::base::*;
use crate::solid::*;
use crate::geom2d;
use crate::geom3d;
use crate::geom3d::Axis;
use crate::surface::intersection;
use crate::surface::SurfaceType;

// use crate::log;


pub fn extrude(profile: &Profile, distance: f64) -> Result<Compound, String> {
  //XXX use poly_from_wirebounds once circles are handled
  let poly = geom2d::tesselate_wire(&profile[0]);
  let plane = geom3d::plane_from_points(&poly)?;
  let plane_normal = plane.normal() * distance;
  let mut solid = Solid::new_lamina(profile[0].clone(), plane.into_enum());
  let shell = &mut solid.shells[0];
  let face = if distance >= 0.0 {
    shell.faces.last()
  } else {
    shell.faces.first()
  }.unwrap().clone();
  shell.sweep(&face, plane_normal);
  if distance > 200.0 {
    Err(format!("Maximum extrusion distance exceeded"))
  } else {
    Ok(solid.into_compound())
  }
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
  let mut region = vec![vec![]];
  let mut iter = points.iter().peekable();
  while let Some(&p) = iter.next() {
    let next = if let Some(&next) = iter.peek() {
      next
    } else {
      &points[0]
    };
    region[0].push(TrimmedCurve::new(Line::new(p, *next).into_enum()));
  }
  extrude(&region, dz)
}

pub fn make_cylinder(radius: f64, height: f64) -> Result<Compound, String> {
  let region = vec![vec![
    TrimmedCurve::new(Circle::new(Point3::origin(), radius).into_enum())
  ]];
  extrude(&region, height)
}

pub fn draft(faces: &Vec<Ref<Face>>, fixed_plane: &Plane, angle: Deg<f64>) -> Result<(), String> {
  for face in faces {
    let mut face = face.borrow_mut();
    match &face.surface {
      SurfaceType::Planar(plane) => {
        let isect = intersection::plane_plane(plane, fixed_plane);
        if let Some(line) = isect.get_line() {
          let axis = Axis::from_points(line.endpoints());
          face.surface.as_surface_mut().rotate_about_axis(axis, angle);
        }
      },
      SurfaceType::Cylindrical(_) => todo!(),
    }
  }
  Ok(())
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
