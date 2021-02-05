use crate::Mesh;


pub fn export(mesh: &Mesh, name: &str) -> String {
  let mut file = format!("solid {}", name);
  for i in (0..mesh.faces.len() - 2).step_by(3) {
    file.push_str(&format!("\nfacet normal {} {} {}", 0.0, 0.0, 0.0));
    file.push_str("\nouter loop");
    for j in i ..= i + 2 {
      let vertex = mesh.vertices[mesh.faces[j]];
      file.push_str(&format!("\nvertex {} {} {}", vertex.x, vertex.y, vertex.z));
    }
    file.push_str("\nendloop");
    file.push_str("\nendfacet");
  }
  file.push_str(&format!("\nendsolid {}\n", name));
  file
}


#[cfg(test)]
mod tests {
use crate::mesh::Meshable;
use crate::features::make_cube;

  #[test]
  fn stl() {
    let cube = make_cube(1.5, 1.5, 1.5).unwrap();
    let _stl = super::export(&cube.tesselate(), "Cube");
  }
}
