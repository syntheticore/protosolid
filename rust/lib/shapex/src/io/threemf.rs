use crate::mesh::Mesh;


pub fn export(meshes: &Vec<Mesh>, unit: &str) -> String {
  let mut file = format!(r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="{}"
  xml:lang="en-US"
  xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <metadata name="Application">Alchemy</metadata>
  <resources>
  <basematerials id="1">
    <base name="Green" displaycolor="#21BB4CFF" />
  </basematerials>"##, unit);
  for (id, mesh) in meshes.iter().enumerate() {
    file.push_str(&format!(r##"
    <object id="{}" type="model" pid="1" pindex="0">
      <mesh>
        <vertices>"##, id + 2));
    for vertex in &mesh.vertices {
      file.push_str(&format!(
        "\n<vertex x=\"{}\" y=\"{}\" z=\"{}\" />",
        vertex.x, vertex.y, vertex.z,
      ));
    }
    file.push_str(r##"
        </vertices>
        <triangles>"##);
    for i in (0..mesh.faces.len() - 2).step_by(3) {
      file.push_str(&format!(
        "\n<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\" />",
        mesh.faces[i], mesh.faces[i + 1], mesh.faces[i + 2],
      ));
    }
    file.push_str(r##"
        </triangles>
      </mesh>
    </object>"##);
  }
  file.push_str(r##"
    <object id="99" type="model">
      <components>
        <component objectid="2" />
      </components>
    </object>
  </resources>
  <build>"##);
  for (id, _) in meshes.iter().enumerate() {
    file.push_str(&format!("\n    <item objectid=\"{}\" printable=\"1\" />", id + 2));
  }
  file.push_str(r##"
  </build>
</model>
"##);
  file
}
