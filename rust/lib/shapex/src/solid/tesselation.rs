use crate::solid::*;
use crate::mesh::*;


impl Meshable for Solid {
  fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for shell in &self.shells {
      let is_inner = !ptr::eq(shell, &self.shells[0]);
      for face in &shell.faces {
        let mut face_mesh = face.borrow().get_surface().tesselate();
        if is_inner { face_mesh.invert_normals() }
        mesh.append(face_mesh);
      }
    }
    mesh
  }
}


#[cfg(test)]
mod tests {

}
