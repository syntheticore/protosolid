use crate::base::*;
// use crate::surface::*;
use crate::solid::*;

use crate::geom2d;
use crate::geom3d;

// use crate::log;


pub fn extrude(profile: &Profile, distance: f64) -> Result<Solid, String> {
  //XXX use poly_from_wirebounds once circles are handled
  let poly = geom2d::tesselate_wire(&profile[0]);
  // #[cfg(debug_assertions)]
  // assert!(!geom2d::is_clockwise(&poly));
  let plane = geom3d::detect_plane(&poly)?;
  // plane.flip();
  let plane_normal = plane.normal() * distance;
  // let plane_normal = plane.as_transform().transform_vector(Vec3::new(0.0, 0.0, distance));
  // log!("Generated plane {:?}", plane_normal);
  // log!("Work plane {:?}", normal);
  let mut solid = Solid::new_lamina(profile[0].clone(), plane.into_enum());
  let shell = &mut solid.shells[0];
  let face = if distance >= 0.0 {
    shell.faces.last()
  } else {
    shell.faces.first()
  }.unwrap().clone();
  shell.sweep(&face, plane_normal);
  if distance > 200.0 {
    Err(format!("Maximum extrusion distance exceeded {:?}", profile.len()))
  } else {
    Ok(solid)
  }
}

pub fn fillet_edges(_solid: &mut Solid, _edges: Vec<&Edge>) {

}

pub fn make_cube(dx: f64, dy: f64, dz: f64) -> Result<Solid, String> {
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

pub fn make_cylinder(radius: f64, height: f64) -> Result<Solid, String> {
  let region = vec![vec![
    TrimmedCurve::new(Circle::new(Point3::origin(), radius).into_enum())
  ]];
  extrude(&region, height)
}


#[cfg(test)]
mod tests {
  use super::*;
  // use crate::test_data;

  #[test]
  fn cube() {
    let cube = make_cube(1.5, 1.5, 1.5).unwrap();
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
    let cube = make_cylinder(1.0, 1.0).unwrap();
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
