use crate::solid::*;


impl Compound {
  pub fn repair(&mut self) -> Result<(), String> {
    for solid in &mut self.solids {
      solid.validate()?;
      // solid.repair()?;
    }
    self.join_solids();
    Ok(())
  }

  fn join_solids(&mut self) {} //XXX
}


impl Solid {
  pub fn repair(&mut self) -> Result<(), String> {
    for shell in &mut self.shells {
      shell.repair()?;
    }
    Ok(())
  }
}


impl Shell {
  pub fn repair(&mut self) -> Result<(), String> {
    for edge in &mut self.edges {
      edge.borrow_mut().repair()?;
    }
    Ok(())
  }
}


impl Face {
  pub fn repair_edges(&self) -> Result<(), String> {
    for he in self.outer_ring.borrow().iter() {
      let edge = he.borrow().get_edge();
      edge.borrow_mut().repair()?;
    }
    Ok(())
  }
}


impl Edge {
  pub fn repair(&mut self) -> Result<(), String> {
    let intersections = self.get_left_face().borrow().surface.intersect(&self.get_right_face().borrow().surface);
    if intersections.len() == 0 { return Err("Adjacent faces don't intersect".into()) }
    for intersection in intersections {
      match intersection {
        SurfaceIntersectionType::Contained
        => todo!(), //XXX Faces should be joined into one

        SurfaceIntersectionType::Touch(curve)
        | SurfaceIntersectionType::Cross(curve)
        | SurfaceIntersectionType::Extended(curve)
        => {
          let top_sect = curve.intersect_surface(&self.get_top_face().borrow().surface);
          let top_sect = top_sect.first().and_then(|isect| isect.get_point(true) ); //XXX Select correct intersection
          let bottom_sect = curve.intersect_surface(&self.get_bottom_face().borrow().surface);
          let bottom_sect = bottom_sect.first().and_then(|isect| isect.get_point(true) );
          if let (Some(top_bound), Some(bottom_bound)) = (top_sect, bottom_sect) {
            self.left_half.borrow().origin.borrow_mut().point = bottom_bound;
            self.right_half.borrow().origin.borrow_mut().point = top_bound;
            let id = self.curve.get_id();
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
