import * as THREE from 'three'
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js'
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js'
import materials from '../materials.js'
import HiddenLine from './hidden-line.js'

// Section views clip the model on the GPU instead of cutting its BREP. The
// cut surfaces are drawn as caps: for every plane, the surfaces behind it are
// counted into the stencil buffer, front faces negative and back faces
// positive. Points of the plane inside the material end up nonzero, and a quad
// on the plane fills exactly those with the section material.

let nextOrder = 1

const clippedVariants = new WeakMap()

// Planes lie slightly inside the removed half, by the offset, so surfaces on a
// plane don't get clipped by chance. Of those, surfaces only stay when their
// material is on the kept side, telling by their winding.
const sidedClipping = `
  #if NUM_CLIPPING_PLANES > 0
    vec3 sectionOutward = normalize( cross( dFdx( vClipPosition ), dFdy( vClipPosition ) ) );
    // Back side materials are drawn with flipped winding
    #ifdef FLIP_SIDED
      if( gl_FrontFacing ) sectionOutward = -sectionOutward;
    #else
      if( !gl_FrontFacing ) sectionOutward = -sectionOutward;
    #endif
    for( int i = 0; i < NUM_CLIPPING_PLANES; i ++ ) {
      vec4 plane = clippingPlanes[ i ];
      float margin = dot( sectionOutward, plane.xyz ) < 0.0 ? 0.0 : 2.0 * sectionOffset;
      if( dot( vClipPosition, plane.xyz ) > plane.w - margin ) discard;
    }
  #endif
`

function clipSided(material, offset, cacheKey) {
  const onBeforeCompile = material.onBeforeCompile
  material.onBeforeCompile = function(shader, renderer) {
    onBeforeCompile.call(this, shader, renderer)
    shader.uniforms.sectionOffset = offset
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <clipping_planes_pars_fragment>', '#include <clipping_planes_pars_fragment>\nuniform float sectionOffset;')
      .replace('#include <clipping_planes_fragment>', sidedClipping)
  }
  material.customProgramCacheKey = () => `${cacheKey()}:sided-clipping`
}

// The material, clipped by the given world space planes. Variants are cached
// per planes array, which callers keep and update in place.
export function clippedMaterial(material, planes) {
  if(!planes) return material
  let variants = clippedVariants.get(planes)
  if(!variants) clippedVariants.set(planes, variants = new Map())
  let variant = variants.get(material)
  if(!variant) {
    // Shader customizations are not copied by clone, nor is user data worth
    // serializing, which may hold compiled shaders.
    const userData = material.userData
    material.userData = {}
    variant = material.clone()
    material.userData = userData
    variant.userData = userData
    variant.onBeforeCompile = material.onBeforeCompile
    variant.customProgramCacheKey = material.customProgramCacheKey
    variant.alcProjectionPerspective = material.alcProjectionPerspective
    variant.clippingPlanes = planes
    // Lines have no side, they all stay on the plane
    if(!material.isShaderMaterial) clipSided(variant, planes.offset, () => material.customProgramCacheKey())
    // Follow later changes of the original, like theme or component colors
    variant.onBeforeRender = () => {
      if(material.color) variant.color.copy(material.color)
      if(material.emissive) variant.emissive.copy(material.emissive)
      variant.opacity = material.opacity
      if(material.isLineMaterial) variant.linewidth = material.linewidth
    }
    variants.set(material, variant)
  }
  return variant
}

// Whether a raycaster hit lies on geometry removed by its material's planes,
// deciding on surfaces lying on a plane like the shaders do
export function isClipped(hit) {
  const material = hit.object.material
  const planes = material?.clippingPlanes
  if(!planes?.length) return false
  const point = hit.pointOnLine || hit.point
  const outward = hit.face && !material.isShaderMaterial &&
    hit.face.normal.clone().transformDirection(hit.object.matrixWorld)
  return planes.some(plane => {
    const margin = outward && outward.dot(plane.normal) >= 0 ? 2 * planes.offset.value : 0
    return plane.distanceToPoint(point) < margin
  })
}

function stencilMaterial(side, operation, plane, offset) {
  const material = new THREE.MeshBasicMaterial({
    side,
    colorWrite: false,
    depthWrite: false,
    depthTest: false,
    stencilWrite: true,
    stencilFunc: THREE.AlwaysStencilFunc,
    stencilFail: operation,
    stencilZFail: operation,
    stencilZPass: operation,
    clippingPlanes: [plane],
  })
  clipSided(material, offset, () => 'section-stencil')
  return material
}

export default class SectionCaps {
  constructor() {
    // World space planes, shared with the clipped materials of the component,
    // with how far they were moved into the removed half
    this.planes = []
    this.planes.offset = { value: 0 }
    this.group = new THREE.Group()
    this.group.matrixAutoUpdate = false
    this.layers = []
    this.geometry = new THREE.BufferGeometry()
    this.faces = null
    this.order = nextOrder++
    this.outline = null
    window.alcRenderer.add(this.group, false)
  }

  // Sections are given as world space matrices, whose Z axis is the plane
  // normal. Side keeps the half behind the plane.
  update({ sections, transform, faces, color, outlineMaterial, hiddenLines }) {
    if(faces != this.faces) this.setFaces(faces)
    if(sections.length != this.planes.length) this.setPlaneCount(sections.length, color)

    const bounds = this.geometry.boundingBox.clone().applyMatrix4(transform)
    const center = bounds.getCenter(new THREE.Vector3())
    const diagonal = bounds.getSize(new THREE.Vector3()).length()
    const size = Math.max(diagonal * 2, 1e-3)

    // Faces lying on a plane would be clipped and counted by chance. The
    // shaders decide on them by side, within the offset.
    const offset = this.planes.offset.value = diagonal * 1e-4
    const origin = new THREE.Vector3()
    sections.forEach((section, index) => {
      const normal = new THREE.Vector3().setFromMatrixColumn(section.matrix, 2).normalize()
      if(section.side) normal.negate()
      this.planes[index].setFromNormalAndCoplanarPoint(normal, origin.setFromMatrixPosition(section.matrix))
      this.planes[index].constant += offset
    })
    const up = new THREE.Vector3(0, 0, 1)
    this.layers.forEach((layer, index) => {
      const plane = this.planes[index]
      layer.back.matrix.copy(transform)
      layer.front.matrix.copy(transform)
      layer.cap.matrix.compose(
        plane.projectPoint(center, new THREE.Vector3()),
        new THREE.Quaternion().setFromUnitVectors(up, plane.normal),
        new THREE.Vector3(size, size, 1),
      )
    })

    this.updateOutline(transform, outlineMaterial, hiddenLines)
  }

  setFaces(faces) {
    this.faces = faces
    const meshes = faces.map(face => face.tesselate().positions)
    const positions = new Float32Array(meshes.reduce((sum, mesh) => sum + mesh.length, 0))
    let offset = 0
    meshes.forEach(mesh => {
      positions.set(mesh, offset)
      offset += mesh.length
    })
    const geometry = new THREE.BufferGeometry()
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3))
    geometry.computeBoundingBox()
    geometry.computeBoundingSphere()
    this.layers.forEach(layer => {
      layer.back.geometry = geometry
      layer.front.geometry = geometry
    })
    this.geometry.dispose()
    this.geometry = geometry
  }

  setPlaneCount(count, color) {
    this.disposeLayers()
    this.planes.length = count
    for(let i = 0; i < count; i++) this.planes[i] ||= new THREE.Plane()
    this.layers = this.planes.map((plane, index) => {
      // Each plane is counted and capped on its own, the stencil cleared after
      const order = 1 + this.order + index / (count + 1)
      const back = new THREE.Mesh(this.geometry, stencilMaterial(THREE.BackSide, THREE.IncrementWrapStencilOp, plane, this.planes.offset))
      const front = new THREE.Mesh(this.geometry, stencilMaterial(THREE.FrontSide, THREE.DecrementWrapStencilOp, plane, this.planes.offset))
      const capMaterial = materials.makeSectionSurface(color)
      Object.assign(capMaterial, {
        // Follows the component's color
        color,
        clippingPlanes: this.planes.filter(other => other != plane),
        stencilWrite: true,
        stencilRef: 0,
        stencilFunc: THREE.NotEqualStencilFunc,
        stencilFail: THREE.ReplaceStencilOp,
        stencilZFail: THREE.ReplaceStencilOp,
        stencilZPass: THREE.ReplaceStencilOp,
        // Faces kept on the plane stay in front of the cap
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 1,
      })
      const cap = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), capMaterial)
      cap.onAfterRender = renderer => renderer.clearStencil()
      back.renderOrder = front.renderOrder = order
      cap.renderOrder = order + 0.5 / (count + 1)
      ;[back, front, cap].forEach(mesh => {
        mesh.matrixAutoUpdate = false
        mesh.frustumCulled = false
        mesh.raycast = () => {}
        this.group.add(mesh)
      })
      return { back, front, cap }
    })
  }

  // The cut's outline along the tessellation, standing in for the edges a
  // BREP cut would have.
  updateOutline(transform, material, hiddenLines) {
    if(this.outline) {
      this.group.remove(this.outline)
      this.outline.geometry.dispose()
      this.outline.children[0].material.dispose()
      this.outline = null
    }
    if(!material) return
    const positions = this.outlinePositions(transform)
    if(!positions.length) return
    const geometry = new LineSegmentsGeometry()
    geometry.setPositions(positions)
    geometry.setColors(Array(positions.length).fill(1))
    this.outline = new LineSegments2(geometry, material)
    this.outline.computeLineDistances()
    this.outline.matrixAutoUpdate = false
    this.outline.matrix.copy(transform)
    this.outline.raycast = () => {}
    const hidden = new HiddenLine(this.outline)
    hidden.visible = hiddenLines
    this.outline.add(hidden)
    this.group.add(this.outline)
  }

  outlinePositions(transform) {
    const inverse = transform.clone().invert()
    const planes = this.planes.map(plane => plane.clone().applyMatrix4(inverse))
    const offset = this.planes.offset.value
    const positions = this.geometry.getAttribute('position').array
    const out = []
    const points = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()]
    const distances = [0, 0, 0]
    const crossings = []
    const start = new THREE.Vector3()
    const end = new THREE.Vector3()
    for(let i = 0; i < positions.length; i += 9) {
      points.forEach((point, k) => point.fromArray(positions, i + k * 3))
      planes.forEach((plane, index) => {
        // Distances to the actual plane, points within the offset on it.
        // Triangles of faces lying on the plane are no part of the outline.
        points.forEach((point, k) => {
          const distance = plane.distanceToPoint(point) - offset
          distances[k] = Math.abs(distance) > offset ? distance : 0
        })
        if(Math.min(...distances) > 0 || Math.max(...distances) < 0) return
        crossings.length = 0
        for(let k = 0; k < 3; k++) {
          const a = distances[k]
          const b = distances[(k + 1) % 3]
          if(a == 0) crossings.push(points[k].clone())
          if(a * b < 0) crossings.push(points[k].clone().lerp(points[(k + 1) % 3], a / (a - b)))
        }
        if(crossings.length != 2) return
        start.copy(crossings[0])
        end.copy(crossings[1])
        // Keep the part inside the other planes
        let from = 0
        let to = 1
        for(let other = 0; other < planes.length && from < to; other++) {
          if(other == index) continue
          const a = planes[other].distanceToPoint(start)
          const b = planes[other].distanceToPoint(end)
          if(a < 0 && b < 0) to = from
          else if(a < 0) from = Math.max(from, a / (a - b))
          else if(b < 0) to = Math.min(to, a / (a - b))
        }
        if(from >= to) return
        out.push(...start.clone().lerp(end, from).toArray(), ...start.clone().lerp(end, to).toArray())
      })
    }
    return out
  }

  disposeLayers() {
    this.layers.forEach(layer => {
      this.group.remove(layer.back, layer.front, layer.cap)
      layer.back.material.dispose()
      layer.front.material.dispose()
      layer.cap.material.dispose()
      layer.cap.geometry.dispose()
    })
    this.layers = []
  }

  dispose() {
    this.disposeLayers()
    this.updateOutline(null, null)
    this.geometry.dispose()
    window.alcRenderer.remove(this.group)
  }
}
