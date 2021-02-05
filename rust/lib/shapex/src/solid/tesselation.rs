use crate::solid::*;
use crate::mesh::*;


impl Meshable for Solid {
  fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for shell in &self.shells {
      for face in &shell.faces {
        let face_mesh = face.borrow().tesselate();
        mesh.append(face_mesh);
      }
    }
    mesh
  }
}

impl Meshable for Face {
  fn tesselate(&self) -> Mesh {
    let mut mesh = self.get_surface().tesselate();
    if self.flip_normal { mesh.invert_normals() }
    mesh
  }
}


#[cfg(test)]
mod tests {

}
