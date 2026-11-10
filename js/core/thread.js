import * as THREE from 'three'

import { namedBody, polygon } from './geom3d.js'
import { shapex as sx, modelTolerance, point3, vector3, threeVector } from './shapex.js'
import { fail } from './errors.js'

// ISO 261 pitches by nominal diameter
const coarsePitches = [
  [1, 0.25], [1.2, 0.25], [1.4, 0.3], [1.6, 0.35], [2, 0.4], [2.5, 0.45], [3, 0.5], [3.5, 0.6],
  [4, 0.7], [5, 0.8], [6, 1], [7, 1], [8, 1.25], [10, 1.5], [12, 1.75], [14, 2], [16, 2],
  [18, 2.5], [20, 2.5], [22, 2.5], [24, 3], [27, 3], [30, 3.5], [33, 3.5], [36, 4], [39, 4],
  [42, 4.5], [45, 4.5], [48, 5], [52, 5], [56, 5.5], [60, 5.5], [64, 6],
]
const finePitches = [
  [8, 1], [10, 1.25], [12, 1.5], [14, 1.5], [16, 1.5], [18, 1.5], [20, 1.5], [22, 1.5],
  [24, 2], [27, 2], [30, 2], [33, 2], [36, 3], [39, 3], [42, 3], [45, 3], [48, 3],
  [52, 4], [56, 4], [60, 4], [64, 4],
]

// Height of the fundamental triangle and depth of the basic profile
const triangleHeight = pitch => pitch * Math.sqrt(3) / 2
const threadDepth = pitch => 5 / 8 * triangleHeight(pitch)

// Pitch of the standard size whose external major or internal minor
// diameter is closest to the given face diameter
export function standardPitch(diameter, internal, series) {
  const table = series == 'fine' ? finePitches : coarsePitches
  const faceDiameter = ([nominal, pitch]) => internal ? nominal - 2 * threadDepth(pitch) : nominal
  return table.reduce((best, size) =>
    Math.abs(faceDiameter(size) - diameter) < Math.abs(faceDiameter(best) - diameter) ? size : best)[1]
}

// The axis, radius and axial extent of a cylindrical face, and whether its
// material lies inside the cylinder (a shaft) or around it (a bore).
export function cylinderFrame(face) {
  const surface = face.geom().surface()
  if(surface.kind != 'cylinder') fail('Threads require cylindrical faces')
  const origin = threeVector(surface.axis.origin())
  const direction = threeVector(surface.axis.direction()).normalize()
  const radius = threeVector(surface.radialAxes[0]).length()
  const { positions, normals } = face.tesselate()
  let start = Infinity
  let end = -Infinity
  let outwards = 0
  const point = new THREE.Vector3()
  const normal = new THREE.Vector3()
  for(let i = 0; i < positions.length; i += 3) {
    point.fromArray(positions, i).sub(origin)
    const height = point.dot(direction)
    start = Math.min(start, height)
    end = Math.max(end, height)
    point.addScaledVector(direction, -height)
    outwards += normal.fromArray(normals, i).dot(point)
  }
  return { origin, direction, radius, start, end, external: outwards > 0 }
}

// Body from a closed polygon of [x, y] corners, naming faces after its sides
function polygonBody(componentId, points, tag, build, owner) {
  const { curves, names } = polygon(points.map(([x, y]) => new THREE.Vector2(x, y)), tag)
  return namedBody(componentId, curves, names, build, owner)
}

// Modelled ISO metric thread on a cylinder given by its `cylinderFrame`, as
// the body its groove removes. A shaft keeps its diameter as the major
// diameter, a bore as the minor diameter. The groove runs between the axial
// heights `start` and `end`. `tag` distinguishes the faces of several grooves.
export function threadGroove(componentId, frame, { pitch, start, end, rightHanded }, owner, tag) {
  if(!(pitch > 0)) fail('Thread pitch must be positive')
  if(end - start <= modelTolerance.linear) fail('Thread has no length')
  const { origin, direction, radius, external } = frame

  // Groove section between the flanks of neighboring teeth. Its open side
  // reaches beyond the face, so the cut leaves no sliver of the surface.
  const depth = threadDepth(pitch)
  const clearance = triangleHeight(pitch) / 16
  const slope = Math.tan(Math.PI / 6)
  const [bottom, top, rootHalf] = external ?
    [radius - depth, radius + clearance, pitch / 8] :
    [radius + depth, radius - clearance, pitch / 16]
  if(Math.min(bottom, top) <= modelTolerance.linear) fail('Thread is deeper than the cylinder')
  const openHalf = rootHalf + Math.abs(top - bottom) * slope
  const section = [[bottom, -rootHalf], [top, -openHalf], [top, openHalf], [bottom, rootHalf]]

  // Overrun both limits by a full turn, then trim the groove to them
  const radial = new THREE.Vector3(1, 0, 0)
  if(Math.abs(radial.dot(direction)) > 0.9) radial.set(0, 1, 0)
  radial.addScaledVector(direction, -radial.dot(direction)).normalize()
  const base = origin.clone().addScaledVector(direction, start - pitch)
  const plane = sx.Plane3.new(point3(base), vector3(radial), vector3(direction), modelTolerance)
  const axis = sx.Axis3.new(point3(base), vector3(direction))
  const turns = (end - start) / pitch + 2
  const hand = rightHanded ? 1 : -1
  const helix = polygonBody(componentId, section, `${tag}/groove`, profile =>
    profile.coil(plane, axis, 2 * Math.PI * turns * hand, pitch), owner)

  // A prism between the limit planes, wider than the groove
  const reach = Math.max(bottom, top) + pitch
  const limitBase = origin.clone().addScaledVector(direction, start)
  const across = direction.clone().cross(radial)
  const limitPlane = sx.Plane3.new(point3(limitBase), vector3(radial), vector3(across), modelTolerance)
  const limits = polygonBody(componentId, [[-reach, -reach], [reach, -reach], [reach, reach], [-reach, reach]],
    `${tag}/limits`, profile => profile.extrude(limitPlane, vector3(direction.clone().multiplyScalar(end - start))), owner)
  return helix.boolean(limits, 'intersect')
}
