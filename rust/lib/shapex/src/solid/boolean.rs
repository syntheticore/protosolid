use crate::solid::*;


#[derive(Debug, Clone, Copy)]
pub enum BooleanType {
  Create,
  Join,
  Cut,
  Intersection,
  Difference,
}

pub trait Boolean {
  fn create(&mut self, tool: Self);
  fn join(&mut self, tool: Self);
  fn cut(&mut self, tool: Self);
  fn intersect(&mut self, tool: Self);
  fn difference(&mut self, tool: Self);
  fn boolean(&mut self, tool: Self, op: BooleanType);
}


impl Boolean for Compound {
  fn create(&mut self, mut tool: Self) { self.solids.append(&mut tool.solids) }
  fn join(&mut self, mut tool: Self) { self.solids.append(&mut tool.solids) }
  fn cut(&mut self, _tool: Self) { todo!() }
  fn intersect(&mut self, _tool: Self) { todo!() }
  fn difference(&mut self, _tool: Self) { todo!() }

  fn boolean(&mut self, tool: Self, op: BooleanType) {
    match op {
      BooleanType::Create => self.create(tool),
      BooleanType::Join => self.join(tool),
      _ => todo!()
    }
  }
}


#[cfg(test)]
mod tests {

}
