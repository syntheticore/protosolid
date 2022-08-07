use crate::solid::*;


/// All types that have a closed boundary, separating space into what's inside and outside the volume.

pub trait Volume: SurfaceArea {
  fn volume(&self) -> f64;
  fn contains_point(&self, p: Point3) -> bool;
}


impl SurfaceArea for Solid {
  fn area(&self) -> f64 {
    self.shells.iter().fold(0.0, |acc, shell| acc + shell.area() )
  }
}

impl Volume for Solid {
  fn volume(&self) -> f64 {
    self.shells[0].volume() - self.shells.iter().skip(1).fold(0.0, |acc, shell| acc + shell.volume() )
  }

  fn contains_point(&self, p: Point3) -> bool {
    self.shells[0].contains_point(p) && !self.shells.iter().skip(1).any(|shell| shell.contains_point(p) )
  }
}


impl SurfaceArea for Shell {
  fn area(&self) -> f64 {
    self.faces.iter().fold(0.0, |acc, face| acc + face.borrow().area() )
  }
}

impl Volume for Shell {
  fn volume(&self) -> f64 {
    0.0 //XXX
  }

  fn contains_point(&self, p: Point3) -> bool {
    let ray = TrimmedCurve::new(Line::new(p, p + Vec3::unit_x() * 9999999.0).into_enum());
    let num_hits: usize = self.faces.iter().flat_map(|face| {
      let intersections = ray.intersect_surface(&face.borrow().make_surface());
      intersections.iter().map(|isect| match isect {
        CurveSurfaceIntersectionType::Pierce(_)
        | CurveSurfaceIntersectionType::Cross(_)
          => 1,
        _ => 0,
      }).collect::<Vec<usize>>()
    }).sum();
    num_hits % 2 != 0
  }
}


impl SurfaceArea for Face {
  fn area(&self) -> f64 {
    self.make_surface().area()
  }
}


#[cfg(test)]
mod tests {

}
