use crate::solid::*;
use crate::mesh::*;


impl Meshable for Solid {
  fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for shell in &self.shells {
      mesh.append(shell.tesselate());
    }
    mesh
  }
}


impl Meshable for Shell {
  fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for face in &self.faces {
      mesh.append(face.borrow().tesselate());
    }
    mesh
  }
}


impl Meshable for Face {
  fn tesselate(&self) -> Mesh {
    let mut mesh = self.make_surface().tesselate();
    if self.flip_normal { mesh.invert_normals() }
    mesh
  }
}


#[cfg(test)]
mod tests {

}
