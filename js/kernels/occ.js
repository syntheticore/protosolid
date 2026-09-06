import * as THREE from 'three'

export class OccBackend {
  constructor(oc) {
    this.id = 'occ'
    this.title = 'OpenCascade'
    this.oc = oc
    this.capabilities = Object.freeze({
      extrude: true,
      boolean: true,
      transform: true,
      tessellate: true,
      revolve: true,
      loft: true,
      sweep: true,
      fillet: true,
      chamfer: true,
      draft: true,
      offset: true,
      split: true,
      compoundMerge: true,
      multiSolidBoolean: true,
    })
  }

  extrude(profile, height) {
    const face = profile.makeFace()
    const rotation = new THREE.Quaternion().setFromRotationMatrix(profile.sketch.workplane)
    const vector = new THREE.Vector3(0, 0, height).applyQuaternion(rotation)
    const direction = new this.oc.gp_Vec_4(vector.x, vector.y, vector.z)
    return new this.oc.BRepPrimAPI_MakePrism_1(face, direction, true, true).Shape()
  }

  boolean(first, second, operation) {
    const algorithms = {
      join: this.oc.BRepAlgoAPI_Fuse_3,
      cut: this.oc.BRepAlgoAPI_Cut_3,
      intersect: this.oc.BRepAlgoAPI_Common_3,
    }
    return new algorithms[operation](
      first,
      second,
      new this.oc.Message_ProgressRange_1(),
    )
  }

  transform(geometry, matrix) {
    const position = new THREE.Vector3().setFromMatrixPosition(matrix)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(matrix)
    const transform = new this.oc.gp_Trsf_1()
    transform.SetRotationPart(new this.oc.gp_Quaternion_2(
      rotation.x, rotation.y, rotation.z, rotation.w,
    ))
    transform.SetTranslationPart(new this.oc.gp_Vec_4(position.x, position.y, position.z))
    return new this.oc.BRepBuilderAPI_Transform_2(geometry, transform, true)
  }
}
