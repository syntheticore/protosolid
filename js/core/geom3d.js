import * as THREE from 'three'
import { ProfileReference, EdgeReference, FaceReference, SolidReference, PlanarReference, AxialReference, PointReference } from './references.js'
import { makeID } from './id.js'
import { markRaw } from 'vue'
import { resolveCompoundSelection, restoreCompoundSelections, identifyTopology, topologyItems } from './topology.js'
import { seedProfileTopology, seedImportTopology, trackTopology, trackOperations } from './topology-history.js'
import { captureTopologySelection, namespaceTopology } from './topological-naming.js'
import { normalFromMatrix, rotationFromNormal, EPSILON } from './utils.js'
import { fail } from './errors.js'
import {
  shapex as sx, modelTolerance, tessellationOptions, point2, threeVector,
  planeFromMatrix, axisFromMatrix, transformFromMatrix, consumeEntityIds, tessellateMesh,
} from './shapex.js'

function extrusionRange(geometry, origin, direction) {
  // Only the disposable prism extent comes from the mesh; Booleans retain the
  // exact body. A fixed mesh keeps modeling independent of display settings.
  const extentOptions = sx.TessellationOptions.new(0.1, 0.1, 24)
  const mesh = geometry.tessellate(extentOptions)
  let minimum = Infinity, maximum = -Infinity
  for(const point of mesh.positions) {
    const distance = threeVector(point).sub(origin).dot(direction)
    minimum = Math.min(minimum, distance)
    maximum = Math.max(maximum, distance)
  }
  if(!Number.isFinite(minimum) || !Number.isFinite(maximum)) {
    fail('Through all could not determine the body extent')
  }
  const padding = Math.max(extentOptions.chordTolerance * 4,
    modelTolerance.linear * 16, (maximum - minimum) * 0.01)
  return [minimum - padding, maximum + padding]
}

export class Wire {
  constructor(region, startPoint) {
    region = region.map(seg => seg.clone())
    if(region.length == 0) fail('Wires may not be empty')
    else if(region.length >= 2) {
      // Find starting point from element order
      let bounds = region[0].endpoints()
      let next_bounds = region[1].endpoints()
      let point = startPoint || (bounds[0].almost(next_bounds[0]) || bounds[0].almost(next_bounds[1]) ? bounds[1] : bounds[0])

      // Flip curves to flow consistently along element order
      region.forEach(tcurve => {
        if (tcurve.endpoints()[1].almost(point)) {
          point = tcurve.endpoints()[0]
          tcurve.flip()
        } else {
          if (!tcurve.endpoints()[0].almost(point)) fail("Wire segments must be connected")
          point = tcurve.endpoints()[1]
        }
      })
    }
    let firstPoint = region[0].endpoints()[0]
    let lastPoint = region.slice(-1)[0].endpoints()[1]
    if (!firstPoint.almost(lastPoint)) fail("Wires must be closed")
    this.segments = region
  }
  // Validated boundary for profiles. Building it rejects loops that are not simple.
  geom() { return this.cachedWire ||= sx.Wire2.new(this.segments.map(segment => segment.geom()), modelTolerance) }
  // Sketch loops are face walks of the split sketch graph. A walk can retrace a
  // bridge between two loops, so orientation and containment use the exact
  // curves as a path, which need not be simple.
  path() { return this.cachedPath ||= sx.Path2.new(this.segments.map(segment => segment.geom()), true, modelTolerance) }
  isClockwise() { return this.path().signedArea() < 0 }
  reverse() {
    this.segments.reverse()
    this.segments.forEach(curve => curve.flip())
    this.cachedWire = null
    this.cachedPath = null
  }
  containsPoint(point) { return this.path().classifyPoint(point2(point)) == 'inside' }
  encloses(other) {
    // Adjacent sketch regions share split edges, but neither is a hole in the
    // other. Testing their shared endpoints makes them look enclosed because
    // those points lie exactly on both wire boundaries.
    const sharesSegment = this.segments.some(elem => other.segments.some(otherElem => elem.id == otherElem.id))
    if (sharesSegment) return false
    return other.segments.every(elem => this.containsPoint(elem.sample(0.5)))
  }
  tesselate() {
    let polyline = this.segments.flatMap(curve => {
      let poly = curve.tesselate()
      if (!poly) return []
      poly.pop()
      return poly
    })
    return polyline
  }
}
export class Profile {
  constructor(sketch, rings) {
    this.sketch = sketch
    this.rings = rings
    this.id = makeID()
  }
  geom() {
    const rings = this.rings.map(ring => ring.geom())
    return sx.Profile.new(rings[0], rings.slice(1), modelTolerance)
  }
  typename() {
    return 'Profile'
  }
  reference() {
    return new ProfileReference(this)
  }
  tesselate() {
    let polyRings = this.rings.map(wire => wire.tesselate())
    const contours = polyRings.map(ring => ring.map(p => new THREE.Vector2(p[0], p[1])))
    const indices = THREE.ShapeUtils.triangulateShape(contours[0], contours.slice(1)).flat()

    // Map back to original positions
    const vertices = polyRings.flat()
    let triangulatedVertices = indices.map(i => vertices[i])
    return {
      positions: triangulatedVertices.flat(),
      normals: triangulatedVertices.flatMap(v => [0.0, 0.0, 1.0])
    }
  }
  containsPoint(p) {
    return this.rings[0].containsPoint(p) && !this.rings.slice(1).some(wire => wire.containsPoint(p))
  }
  center() {
    const out = new THREE.Vector3()
    this.rings[0].segments.forEach(seg => out.add(seg.center()))
    out.divideScalar(this.rings[0].segments.length)
    return out.applyMatrix4(this.sketch.workplane)
  }
  normal() {
    return normalFromMatrix(this.sketch.workplane)
  }
  getBaseId(curve) {
    return curve.id.split('/')[0]
  }
  extrude(componentId, height, featureId, offset = 0) {
    if (!height) fail('Extrusion has no volume')
    const plane = planeFromMatrix(this.sketch.workplane)
    const geometry = this.geom().extrude(plane, plane.normal().scaled(height))
    const compound = new Compound(componentId, geometry)
    compound.seedProfileTopology([this], featureId)
    if (!offset) return compound
    const translation = this.normal().normalize().multiplyScalar(offset)
    return compound.transform(new THREE.Matrix4().makeTranslation(translation.x, translation.y, translation.z))
  }
  extrudeThroughAll(componentId, support, featureId, side = 'forward') {
    if (!support?.geom || !support.solids().length) fail('Through all requires an existing body')
    const profileDirection = this.normal().normalize()
    const origin = new THREE.Vector3().setFromMatrixPosition(this.sketch.workplane)
    const [minimum, maximum] = extrusionRange(support.geom(), origin, profileDirection)
    const start = side == 'forward' ? 0 : Math.min(0, minimum)
    const end = side == 'reverse' ? 0 : Math.max(0, maximum)
    if (end - start <= EPSILON) fail('Through all does not intersect an existing body in the selected direction')
    // Include separated bodies and
    // curved exit faces. A single prism avoids an internal seam for both sides.
    const tool = this.extrude(componentId, end - start, featureId, start)
    const result = tool.boolean(support, 'intersect')
    if (!result.solids().length || result.volume() <= EPSILON) {
      fail('Through all does not intersect an existing body in the selected direction')
    }
    return result
  }
  extrudeUpToFace(componentId, target, featureId, offset = 0, endOffset = 0) {
    if(!target) fail('Select a face to extrude up to')
    const body = target.solid.geom().toCompound()
    try {
      const geometry = this.geom().extrudeUpToFace(
        planeFromMatrix(this.sketch.workplane), body, target.geom(), offset, endOffset,
      )
      return new Compound(componentId, geometry).seedProfileTopology([this], featureId)
    } finally { body.free() }
  }
  revolve(componentId, axis, angle, featureId, offset = 0) {
    if (!angle) fail('Revolution has no volume')
    const geometry = this.geom().revolve(planeFromMatrix(this.sketch.workplane), axisFromMatrix(axis), angle)
    const compound = new Compound(componentId, geometry).seedProfileTopology([this], featureId)
    if (!offset) return compound
    const turn = axis.clone().multiply(new THREE.Matrix4().makeRotationZ(offset)).multiply(axis.clone().invert())
    return compound.transform(turn)
  }
  revolveUpToFace(componentId, axis, target, featureId, forward = true) {
    if(!target) fail('Select a face to revolve up to')
    const body = target.solid.geom().toCompound()
    try {
      const geometry = this.geom().revolveUpToFace(
        planeFromMatrix(this.sketch.workplane), axisFromMatrix(axis), body, target.geom(), forward,
      )
      return new Compound(componentId, geometry).seedProfileTopology([this], featureId)
    } finally { body.free() }
  }
  revolveBetweenFaces(componentId, axis, from, until, featureId, forward = true) {
    if(!from || !until) fail('Select both limiting faces')
    const fromBody = from.solid.geom().toCompound()
    const untilBody = until.solid.geom().toCompound()
    try {
      const geometry = this.geom().revolveBetweenFaces(
        planeFromMatrix(this.sketch.workplane), axisFromMatrix(axis),
        fromBody, from.geom(), untilBody, until.geom(), forward,
      )
      return new Compound(componentId, geometry).seedProfileTopology([this], featureId)
    } finally { fromBody.free(); untilBody.free() }
  }
  loft(componentId, others, untwist, featureId) {
    const profiles = [this, ...others]
    if(profiles.some(profile => profile.rings.length != 1)) {
      fail('Loft profiles with holes are not supported')
    }
    // Rational loft sections explicitly approximate derived curves at model tolerance.
    const section = profile => profile.geom().approximateRational(modelTolerance).profile
    const geometry = section(this).loft(
      planeFromMatrix(this.sketch.workplane),
      others.map(profile => ({ profile: section(profile), plane: planeFromMatrix(profile.sketch.workplane) })),
      untwist,
    )
    return new Compound(componentId, geometry).seedProfileTopology(profiles, featureId)
  }
  sweep(componentId, rail, featureId) {
    if(this.rings.length != 1) fail('Sweeps with holes are not supported')
    const railSource = rail instanceof Edge ? 'rail' : rail.id.split('/')[0]
    const profile = this.geom(), plane = planeFromMatrix(this.sketch.workplane)
    const geometry = rail.sketch ?
      profile.sweep(plane, planeFromMatrix(rail.sketch.workplane), rail.geom()) :
      profile.sweepAlongEdge(plane, rail.geom())
    return new Compound(componentId, geometry).seedProfileTopology([this], featureId, [railSource])
  }
  update() {
    const elements = this.sketch.profileElements()
    const cutElements = elements.flatMap(elem => elem.split(elements))
    const newWires = this.sketch.getWires(cutElements, false)
    if (this.serializedRingIds) {
      const rings = this.serializedRingIds.map(ring => ring.map(segId => cutElements.find(elem => elem.id == segId)))
      if (rings.some(ring => ring.some(seg => !seg))) {
        return {
          type: 'error',
          msg: "Profile was lost"
        }
      }
      this.rings = rings.map(ring => new Wire(ring))
      this.serializedRingIds = null
    }
    let wasRepairNeeded = false
    let error
    this.rings = this.rings.map(wire => {
      const segmentIds = new Set(wire.segments.map(seg => seg.id))
      const wireIds = new Set(wire.segments.map(seg => this.getBaseId(seg)))
      const replacementWire = newWires.map(newWire => {
        const newSegmentIds = new Set(newWire.segments.map(tcurve => tcurve.id))
        const newWireIds = new Set(newWire.segments.map(tcurve => this.getBaseId(tcurve)))
        const exactMatched = segmentIds.intersection(newSegmentIds).size
        const baseMatched = wireIds.intersection(newWireIds).size
        if (baseMatched > 0) {
          return [exactMatched, baseMatched, newSegmentIds.size, newWire]
        }
      }).filter(Boolean).sort((a, b) => b[0] - a[0] || b[1] - a[1] || Math.abs(segmentIds.size - a[2]) - Math.abs(segmentIds.size - b[2]))[0]
      if (!replacementWire) {
        error = {
          type: 'error',
          msg: "Profile was lost"
        }
        return wire
      }
      const [exactMatched,, newSize, newWire] = replacementWire
      if (exactMatched != segmentIds.size || exactMatched != newSize) wasRepairNeeded = true
      return newWire
    })
    if (error) return error
    if (wasRepairNeeded) return {
      type: 'warning',
      msg: 'Selection has been repaired'
    }
  }
  clone() {
    const clone = new Profile(this.sketch, [...this.rings])
    clone.id = this.id
    clone.serializedRingIds = this.serializedRingIds
    return clone
  }
}

// BREP

class Volumetric {
  area() {
    return this.geom ? this.geom().surfaceArea() : 0
  }
  volume() {
    if (!this.geom) return 0
    return this.geom().volume()
  }
  center() {
    if (!this.geom) return
    return threeVector(this.geom().centroid())
  }
}
export class Face {
  constructor(solid, geom, id, tessId) {
    this.solid = solid
    this.geom = () => geom
    this.id = id
    this.kernelId = geom.id()
    this.tessId = tessId || makeID()
  }
  typename() {
    return 'Face'
  }
  area() {
    return this.geom().area()
  }
  center() {
    return threeVector(this.geom().centroid())
  }
  planarReference() {
    if (!this.getPlane()) throw new Error("Cannot provide plane from non-planar face")
    return new PlanarReference(new FaceReference(this))
  }
  axialReference() {
    if (!this.getAxis()) throw new Error("Cannot provide axis from non-cylindrical face")
    return new AxialReference(new FaceReference(this))
  }
  pointReference() {
    if (!this.getPoint()) throw new Error("Cannot provide point from non-spherical face")
    return new PointReference(new FaceReference(this))
  }
  faceReference() {
    return new FaceReference(this)
  }
  getPlane() {
    const surface = this.geom().surface()
    if(surface.kind != 'plane') return
    const plane = surface.value
    const u = threeVector(plane.u()).normalize()
    const v = threeVector(plane.v())
    v.addScaledVector(u, -v.dot(u)).normalize()
    if(this.geom().orientation() == 'reversed') v.negate()
    return new THREE.Matrix4()
      .makeBasis(u, v, new THREE.Vector3().crossVectors(u, v).normalize())
      .setPosition(threeVector(plane.origin()))
  }
  getAxis() {
    const surface = this.geom().surface()
    if(surface.kind != 'cylinder') return
    return rotationFromNormal(threeVector(surface.axis.direction()).normalize())
      .setPosition(threeVector(surface.axis.origin()))
  }
  getPoint() {
    const surface = this.geom().surface()
    if(surface.kind != 'sphere') return
    return new THREE.Matrix4().setPosition(threeVector(surface.center))
  }
  connectedFaces() {
    const edgeIds = new Set(consumeEntityIds(this.geom().boundaryEdges()))
    return this.solid.edges().filter(edge => edgeIds.has(edge.kernelId)).flatMap(edge => edge.connectedFaces()).filter((face, index, faces) => face && face != this && faces.indexOf(face) == index)
  }
  tesselate() {
    if (this.cachedTesselation) return this.cachedTesselation
    this.cachedTesselation = this.solid.faceTessellation(this)
    return this.cachedTesselation
  }
  tesselateAt(linearTolerance, angularTolerance) {
    return tessellateMesh(this.geom(), {
      chordTolerance: linearTolerance,
      angularTolerance
    })
  }
}
export class Edge {
  constructor(solid, geom, id, tessId) {
    this.solid = solid
    this.geom = () => geom
    this.id = id
    this.kernelId = geom.id()
    this.tessId = tessId || makeID()
  }
  typename() {
    return 'Edge'
  }
  length() {
    const curve = this.geom().curve()
    if(curve instanceof sx.Line3) {
      const [start, end] = this.vertices()
      return start.distanceTo(end)
    }
    if(curve instanceof sx.Circle3) {
      return curve.radius() * Math.abs(this.geom().trimmedCurve().range().span())
    }
    const points = this.tesselate()
    return points.reduce((length, point, index) =>
      index ? length + point.distanceTo(points[index - 1]) : length, 0)
  }
  axialReference() {
    if (!this.getAxis()) throw new Error("Cannot provide axis from non-linear edge")
    return new AxialReference(this.edgeReference())
  }
  edgeReference() {
    return new EdgeReference(this)
  }
  pathReference() {
    return this.edgeReference()
  }

  // Polyline in component space
  getPath() {
    return this.tesselate().map(point => point.clone())
  }
  isSemantic() {
    return this.solid.compound.geom().isSemanticEdge(this.geom())
  }
  getAxis() {
    const curve = this.geom().curve()
    if(!(curve instanceof sx.Line3)) return
    return rotationFromNormal(threeVector(curve.direction()).normalize())
      .setPosition(threeVector(curve.origin()))
  }
  connectedFaces() {
    const ids = consumeEntityIds(this.geom().connectedFaces())
    return ids.map(id => this.solid.faces().find(face => face.kernelId == id)).filter(Boolean)
  }
  center() {
    const points = this.tesselate()
    if (!points.length) return new THREE.Vector3()
    const middle = (points.length - 1) / 2
    const lower = points[Math.floor(middle)]
    const upper = points[Math.ceil(middle)]
    return lower.clone().add(upper).multiplyScalar(0.5)
  }
  vertices() {
    return this.geom().endpoints().map(threeVector)
  }
  tesselate() {
    if (this.cachedTesselation) return this.cachedTesselation
    this.cachedTesselation = this.geom().tessellate(tessellationOptions).points.map(threeVector)
    return this.cachedTesselation

  }
}
export class Solid extends Volumetric {
  constructor(compound, geom) {
    super()
    this.compound = compound
    this.geom = () => geom
    this.kernelId = geom.id()
  }
  typename() {
    return 'Solid'
  }
  faces() {
    if (this.cachedFaces) return this.cachedFaces
    this.cachedFaces = this.geom().faces().map(face => new Face(this, face))
    this.cachedFaces.forEach(face => face.id = face.kernelId)
    return this.cachedFaces
  }
  edges() {
    if (this.cachedEdges) return this.cachedEdges
    this.cachedEdges = this.geom().edges().map(edge => new Edge(this, edge))
    this.cachedEdges.forEach(edge => edge.id = edge.kernelId)
    return this.cachedEdges
  }
  faceTessellation(face) {
    this.cachedFaceTessellations ??= new Map()
    if (!this.cachedFaceTessellations.has(face.kernelId)) {
      // Cache the mesh prepared during feature evaluation for the viewport.
      const mesh = tessellateMesh(face.geom())
      this.cachedFaceTessellations.set(face.kernelId, mesh)
    }
    return this.cachedFaceTessellations.get(face.kernelId)
  }
  cloneCached(newCompound) {
    const clone = new Solid(newCompound, this.geom())
    clone.id = this.id
    clone.cachedFaceTessellations = this.cachedFaceTessellations
    clone.topology = this.topology
    clone.topologyKind = this.topologyKind
    clone.cachedFaces = this.faces().map(face => new Face(clone, face.geom(), face.id, face.tessId))
    clone.cachedEdges = this.edges().map(edge => new Edge(clone, edge.geom(), edge.id, edge.tessId))
    clone.cachedFaces.forEach((face, i) => {
      face.topology = this.faces()[i].topology
      face.topologyKind = 'face'
    })
    clone.cachedEdges.forEach((edge, i) => {
      edge.topology = this.edges()[i].topology
      edge.topologyKind = 'edge'
    })
    return clone
  }
  toCompound() {
    const raw = this.geom().toCompound()
    const compound = new Compound(this.compound.componentId, raw)
    compound.cachedSolids = [this]
    return compound
  }
  tesselate() {
    const tesselations = this.faces().map(face => face.tesselate())
    return {
      positions: tesselations.map(tess => tess.positions).flat(),
      normals: tesselations.map(tess => tess.normals).flat()
    }
  }
  reference() {
    return new SolidReference(this)
  }
}
// Wrap bodies read from an exchange file, owned by the importing feature.
export function importedCompound(componentId, geometry, owner) {
  return seedImportTopology(new Compound(componentId, geometry), owner)
}

export class Compound extends Volumetric {
  constructor(componentId, geometry) {
    super()
    this.componentId = componentId
    if(geometry) {
      const raw = markRaw(geometry)
      this.geom = () => raw
    }
  }
  topologySelection(item, policy) {
    return captureTopologySelection(item, policy)
  }
  resolveTopology(selection) {
    return resolveCompoundSelection(this, selection)
  }
  restoreTopologySelections(feature) {
    restoreCompoundSelections(this, feature)
  }
  namespaceTopology(role) {
    namespaceTopology(topologyItems(this), role)
    identifyTopology(this)
    return this
  }
  solids() {
    if (!this.geom) return []
    if (this.cachedSolids) return this.cachedSolids
    this.cachedSolids = this.geom().solids().map(solid => new Solid(this, solid))
    return this.cachedSolids
  }
  clone(geometry) { return new Compound(this.componentId, geometry || this.geom?.()) }
  cloneCached(geom, cachedSolids) {
    const clone = this.clone(geom)
    clone.topologyFailures = this.topologyFailures
    cachedSolids ||= this.solids()
    clone.cachedSolids = cachedSolids.map(solid => solid.cloneCached(clone))
    return clone
  }
  cloneForComponent(componentId) {
    const clone = this.cloneCached()
    clone.componentId = componentId
    for (const solid of clone.solids()) solid.id = componentId + solid.id.slice(this.componentId.length)
    return clone
  }
  seedProfileTopology(profiles, owner, extraSources) {
    return seedProfileTopology(this, profiles, owner, extraSources)
  }

  // Application IDs describe ancestry. Retention is a kernel-history query.
  preservedFaces(originalFaces) {
    const faces = this.faces()
    const preservedIds = new Set(originalFaces.flatMap(original => {
      const resolution = this.geom().resolve(original.geom())
      const ids = consumeEntityIds(resolution.entities)
      return resolution.kind == 'preserved' ? ids : []
    }))
    return new Set(faces.filter(face => preservedIds.has(face.kernelId)))
  }
  trackTopology(parents, operation, bodyParents) {
    return trackTopology(this, parents, operation, bodyParents)
  }
  faces() {
    return this.solids().flatMap(solid => solid.faces())
  }
  edges() {
    return this.solids().flatMap(solid => solid.edges())
  }
  subset(solids) {
    if(!solids.length) return new Compound(this.componentId)
    const compounds = solids.map(solid => solid.toCompound())
    const geometry = compounds[0].geom().combined(compounds.slice(1).map(compound => compound.geom()))
    return trackTopology(new Compound(this.componentId, geometry), compounds, "combine")
  }
  transform(plane) {
    return new Compound(this.componentId, this.geom().transformed(transformFromMatrix(plane))).trackTopology([this], "transform")
  }
  merge(others) {
    const geometry = this.geom().combined(others.map(other => other.geom()))
    return new Compound(this.componentId, geometry).trackTopology([this, ...others], "combine")
  }

  boolean(other, op) {
    if (!other.geom) fail("Tool body has no volume")
    if (op == 'create') {
      if (!this.geom) return this.cloneCached(other.geom(), other.solids())
      return this.merge([other])
    }
    if (!this.geom) {
      if (op == 'cut' || op == 'intersect') return this
      return this.cloneCached(other.geom(), other.solids())
    }
    const method = { join: 'union', cut: 'difference', intersect: 'intersection' }[op]
    const geometry = this.geom()[method](other.geom())
    return new Compound(this.componentId, geometry).trackTopology([this, other], op, op == 'cut' ? [this] : [this, other])
  }
  fillet(edges, radius, followTangentEdges = true) {
    const geometry = this.geom().filletWithOptions(edges.map(edge => edge.geom()), sx.FilletOptions.new(radius).withTangentPropagation(followTangentEdges))
    return new Compound(this.componentId, geometry).trackTopology([this], 'fillet')
  }
  chamfer(edges, distance, {
    type = 'equal',
    distance2 = distance,
    angle = Math.PI / 4,
    flipSides = false,
    followTangentEdges = true
  } = {}) {
    const referenceFaces = type == 'equal' ? [] : edges.map(edge => edge.connectedFaces().sort((a, b) => a.id.localeCompare(b.id))[flipSides ? 1 : 0].geom())
    const geometry = this.geom().chamfer(edges.map(edge => edge.geom()), distance, followTangentEdges, {
      type,
      referenceFaces,
      value: type == 'distance-angle' ? angle : distance2
    })
    return new Compound(this.componentId, geometry).trackTopology([this], 'chamfer')
  }
  offsetFaces(faces, distance) {
    if (!faces?.length) fail('Select one or more faces to offset')
    if (!Number.isFinite(distance)) fail('Offset distance must be finite')
    if (distance == 0) return this
    const geometry = this.geom().offsetFaces(faces.map(face => face.geom()), distance)
    return new Compound(this.componentId, geometry).trackTopology([this], 'offset-faces')
  }
  shell(openFaces, distance, joinType = 'rounded') {
    openFaces ||= []
    const geometry = this.geom().shell(openFaces.map(face => face.geom()), distance, joinType)
    return new Compound(this.componentId, geometry).trackTopology([this], 'shell')
  }
  draft(faces, plane, angle) {
    const geometry = this.geom().draft(faces.map(face => face.geom()), planeFromMatrix(plane), angle)
    return new Compound(this.componentId, geometry).trackTopology([this], 'draft')
  }
  dissolveFaces(faces) {
    const geometry = this.geom().dissolveFaces(faces.map(face => face.geom()))
    return new Compound(this.componentId, geometry).trackTopology([this], 'remove-faces')
  }
  removeShapes(shapes) {
    if (shapes.some(shape => !(shape instanceof Solid))) {
      fail('Only solids can be removed here')
    }
    const geometry = this.geom().withoutSolids(shapes.map(shape => shape.geom()))
    return new Compound(this.componentId, geometry).trackTopology([this], 'remove-solids')
  }
  replaceShapes(shapes, replacements, algoName) {
    if (shapes.length != 1 || replacements.length != 1 || !(shapes[0] instanceof Face) || !(replacements[0] instanceof Face)) {
      fail('Select one face and one replacement face')
    }
    const geometry = this.geom().replaceFace(shapes[0].geom(), replacements[0].geom())
    return new Compound(this.componentId, geometry).trackTopology([this], algoName || 'replace-face')
  }
  moveFaces(faces, transform) {
    const geometry = this.geom().moveFaces(faces.map(face => face.geom()), transformFromMatrix(transform))
    return new Compound(this.componentId, geometry).trackTopology([this], 'move-faces')
  }
  split(plane, keep) {
    if (!this.geom) return this
    const operations = ['left', 'right'].map(branch => ({
      branch,
      operation: 'split',
      compound: new Compound(this.componentId, this.geom().split(planeFromMatrix(plane), branch))
    }))
    const nonempty = operations.map(operation => operation.compound).filter(compound => compound.solids().length)
    if (!nonempty.length) return new Compound(this.componentId)
    const out = new Compound(this.componentId, nonempty[0].geom().combined(nonempty.slice(1).map(compound => compound.geom())))
    trackOperations(out, [this], operations)
    if (keep == 'both') return out
    const retained = new Set(operations[keep == 'left' ? 0 : 1].compound.solids().map(solid => solid.kernelId))
    return out.subset(out.solids().filter(solid => retained.has(solid.kernelId))).cloneCached()
  }
  halfCut(plane, side) {
    if(!this.geom) return this
    return new Compound(this.componentId, this.geom().split(planeFromMatrix(plane), side ? 'left' : 'right')).trackTopology([this], "section")
  }
  tesselate() {
    if (!this.geom) return {
      positions: [],
      normals: []
    }
    return tessellateMesh(this.geom())
  }
}
