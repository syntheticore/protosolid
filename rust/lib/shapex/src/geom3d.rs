use crate::base::*;
use crate::Plane;
use crate::PolyLine;


#[derive(Debug, Clone)]
pub struct Axis {
  pub origin: Point3,
  pub direction: Vec3,
}


// Find suitable points in wire to build a matching plane
pub fn detect_plane(poly: &PolyLine) -> Result<Plane, String> {
  //XXX use points with greatest distance as start points
  // let v1 = (poly[2] - poly[1]).normalize(); // Skip duplicate first point
  // if let Some(p3) = poly.iter().skip(3).min_by(|p1, p2| {
  //   let v2 = (*p1 - poly[1]).normalize();
  //   let v3 = (*p2 - poly[1]).normalize();
  //   v1.dot(v2).abs().partial_cmp(&v1.dot(v3).abs()).unwrap()
  // }) {
  //   if v1.almost((p3 - poly[1]).normalize()) {
  //     Err(format!("P3 {:?} p2 {:?} p1 {:?}", p3, poly[2], poly[1]))
  //   } else {
  //     Ok(Plane::from_triangle(
  //       poly[1],
  //       poly[2],
  //       *p3,
  //     ))
  //   }
  // } else { Err("Could not detect plane from wire".into()) }
  let v1 = (poly[1] - poly[0]).normalize();
  if let Some(p3) = poly.iter().skip(2).min_by(|p1, p2| {
    let v2 = (*p1 - poly[0]).normalize();
    let v3 = (*p2 - poly[0]).normalize();
    v1.dot(v2).abs().partial_cmp(&v1.dot(v3).abs()).unwrap()
  }) {
    if v1.almost((p3 - poly[0]).normalize()) {
      Err(format!("P3 {:?} p2 {:?} p1 {:?}", p3, poly[1], poly[0]))
    } else {
      Ok(Plane::from_triangle(
        *p3,
        poly[1],
        poly[0],
      ))
    }
  } else { Err("Could not detect plane from wire".into()) }
}

pub fn transform_from_location_and_normal(origin: Point3, normal: Vec3) -> Matrix4 {
  let up = Vec3::new(0.0, 0.0, 1.0);
  let x_axis = if normal.dot(up).abs().almost(1.0) {
    Vec3::new(1.0, 0.0, 0.0)
  } else {
    up.cross(normal).normalize()
  };
  let y_axis = normal.cross(x_axis);
  Matrix4::from_cols(
    x_axis.extend(origin.x),
    y_axis.extend(origin.y),
    normal.extend(origin.z),
    Vec4::unit_w()
  ).transpose()
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
