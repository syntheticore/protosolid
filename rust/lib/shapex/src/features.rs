use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::solid::*;


pub fn extrude(region: Vec<TrimmedCurve>, distance: f64) -> Result<Solid, String> {
  let mut solid = Solid::new_lamina(region, Plane::new().into_enum());
  let shell = &mut solid.shells[0];
  shell.sweep(&shell.faces.last().unwrap().clone(), Vec3::new(0.0, 0.0, distance));
  if distance > 200.0 {
    Err("Maximum extrusion distance exceeded".to_string())
  } else {
    Ok(solid)
  }
}

pub fn fillet_edges(_solid: &mut Solid, _edges: Vec<&Edge>) {

}

pub fn make_cube(dx: f64, dy: f64, dz: f64) -> Solid {
  let points = [
    Point3::new(0.0, 0.0, 0.0),
    Point3::new(dx, 0.0, 0.0),
    Point3::new(dx, dy, 0.0),
    Point3::new(0.0, dy, 0.0),
  ];
  let mut region = vec![];
  let mut iter = points.iter().peekable();
  while let Some(&p) = iter.next() {
    let next = if let Some(&next) = iter.peek() {
      next
    } else {
      &points[0]
    };
    region.push(TrimmedCurve::new(Line::new(p, *next).into_enum()));
  }
  extrude(region, dz).unwrap()
}

pub fn make_cylinder(radius: f64, height: f64) -> Solid {
  let region = vec![
    TrimmedCurve::new(Circle::new(Point3::new(0.0, 0.0, 0.0), radius).into_enum())
  ];
  extrude(region, height).unwrap()
}


#[cfg(test)]
mod tests {
  use super::*;
  // use crate::test_data;

  #[test]
  fn cube() {
    let cube = make_cube(1.5, 1.5, 1.5);
    let shell = &cube.shells[0];
    println!("\nCube finished");
    shell.print();
    // panic!("Test trap");
    assert_eq!(shell.vertices.len(), 8);
    assert_eq!(shell.edges.len(), 12);
    assert_eq!(shell.faces.len(), 6);
  }

  #[test]
  fn cylinder() {
    let cube = make_cylinder(1.0, 1.0);
    let shell = &cube.shells[0];
    println!("\nCylinder finished");
    shell.print();
    assert_eq!(shell.vertices.len(), 2);
    assert_eq!(shell.edges.len(), 3);
    assert_eq!(shell.faces.len(), 3);
    assert_eq!(shell.faces[0].borrow().outer_ring.borrow().iter().count(), 1);
    assert_eq!(shell.faces[1].borrow().outer_ring.borrow().iter().count(), 1);
    assert_eq!(shell.faces[2].borrow().outer_ring.borrow().iter().count(), 4);
    panic!("Test trap");
  }
}
