use crate::solid::*;

// use crate::log;


impl Compound {
  pub fn repair(&mut self) -> Result<(), String> {
    for solid in &mut self.solids {
      solid.repair()?;
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


impl Edge {
  pub fn repair(&mut self) -> Result<(), String> {
    match self.get_left_face().borrow().surface.intersect(&self.get_right_face().borrow().surface) {
      SurfaceIntersection::None
      => return Err("Adjacent faces don't intersect".into()),

      SurfaceIntersection::Contained
      => todo!(), //XXX Faces should be joined into one

      SurfaceIntersection::Touch(curve)
      | SurfaceIntersection::Cross(curve)
      | SurfaceIntersection::Extended(curve)
      => {
        let top_sect = curve.intersect_surface(&self.get_top_face().borrow().surface);
        let bottom_sect = curve.intersect_surface(&self.get_bottom_face().borrow().surface);
        if let (Some(top_bound), Some(bottom_bound)) = (top_sect.get_point(), bottom_sect.get_point()) {
          self.left_half.borrow().origin.borrow_mut().point = bottom_bound;
          self.right_half.borrow().origin.borrow_mut().point = top_bound;
          let id = self.curve.get_id();
          self.curve = curve;
          self.curve.set_id(id);
        }
        Ok(())
      },
    }
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


#[cfg(test)]
mod tests {

}
