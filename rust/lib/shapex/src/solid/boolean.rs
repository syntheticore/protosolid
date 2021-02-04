use crate::solid::*;


// #[derive(Debug)]
// pub enum BooleanType {
//   Add,
//   Subtract,
//   Intersection,
//   Difference,
// }

pub trait Boolean {
  fn add(&mut self, tool: Self);
  fn subtract(&mut self, tool: Self);
  fn intersect(&mut self, tool: Self);
  fn difference(&mut self, tool: Self);
}


impl Boolean for Compound {
  fn add(&mut self, mut tool: Self) { self.solids.append(&mut tool.solids) }
  fn subtract(&mut self, _tool: Self) { todo!() }
  fn intersect(&mut self, _tool: Self) { todo!() }
  fn difference(&mut self, _tool: Self) { todo!() }
}


#[cfg(test)]
mod tests {

}
