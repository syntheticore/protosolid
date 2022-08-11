use crate::solid::*;


/// All types that can validate and repair their inner structure without additional data.

pub trait Repairable {
  fn repair(&mut self) -> Result<(), String>;
}


impl Repairable for Compound {
  fn repair(&mut self) -> Result<(), String> {
    for solid in &mut self.solids {
      solid.validate()?;
      // solid.repair()?;
    }
    // self.join_solids();
    Ok(())
  }
}


impl Repairable for Solid {
  fn repair(&mut self) -> Result<(), String> {
    for shell in &mut self.shells {
      shell.repair()?;
    }
    Ok(())
  }
}


impl Repairable for Shell {
  fn repair(&mut self) -> Result<(), String> {
    for edge in &mut self.edges {
      edge.borrow_mut().repair()?;
    }
    Ok(())
  }
}


impl Repairable for Face {
  fn repair(self: &mut Face) -> Result<(), String> {
    for he in self.outer_ring.borrow().iter() {
      let edge = he.borrow().edge();
      edge.borrow_mut().repair()?;
    }
    Ok(())
  }
}


impl Repairable for Edge {
  fn repair(&mut self) -> Result<(), String> {
    let intersections = self.left_face().borrow().surface.intersect(&self.right_face().borrow().surface);
    if intersections.len() == 0 { return Err("Adjacent faces don't intersect".into()) }
    for intersection in intersections {
      match intersection {
        SurfaceIntersectionType::Contained
        => todo!(), //XXX Faces should be joined into one

        SurfaceIntersectionType::Touch(curve)
        | SurfaceIntersectionType::Cross(curve)
        | SurfaceIntersectionType::Extended(curve)
        => {
          let top_sect = curve.intersect_surface(&self.top_face().borrow().surface);
          let top_sect = top_sect.first().and_then(|isect| isect.get_point(true) ); //XXX Select correct intersection
          let bottom_sect = curve.intersect_surface(&self.bottom_face().borrow().surface);
          let bottom_sect = bottom_sect.first().and_then(|isect| isect.get_point(true) );
          if let (Some(top_bound), Some(bottom_bound)) = (top_sect, bottom_sect) {
            self.left_half.borrow().origin.borrow_mut().point = bottom_bound;
            self.right_half.borrow().origin.borrow_mut().point = top_bound;
            let id = self.curve.id();
            self.curve = curve;
            self.curve.set_id(id);
          } else {
            return Err("Edge could not be trimmed by surrounding faces".into())
          }
        },
      }
    }
    Ok(())
  }
}


#[cfg(test)]
mod tests {

}
