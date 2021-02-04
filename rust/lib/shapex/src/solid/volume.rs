use crate::solid::*;


pub trait Volume {
  fn surface_area(&self) -> f64;
  fn volume(&self) -> f64;
}


impl Volume for Solid {
  fn surface_area(&self) -> f64 {
    self.shells.iter().fold(0.0, |acc, shell| acc + shell.surface_area() )
  }

  fn volume(&self) -> f64 {
    self.shells[0].volume() - self.shells.iter().skip(1).fold(0.0, |acc, shell| acc + shell.volume() )
  }
}


impl Volume for Shell {
  fn surface_area(&self) -> f64 {
    self.faces.iter()
    .fold(0.0, |acc, face| acc + face.borrow().get_surface().area() )
  }

  fn volume(&self) -> f64 {
    0.0 //XXX
  }
}


#[cfg(test)]
mod tests {

}
