import * as THREE from 'three'
import earcut from 'earcut'

import { ProfileReference, EdgeReference, FaceReference, SolidReference, PlanarReference, AxialReference, PointReference } from './references.js'
import { makeID } from './id.js'
import { Line } from './geom2d.js'
import { ShapexGeometry } from '../kernels/shapex.js'
import Expression from './expression.js'
import preferences from '../preferences.js'
import {
  arrayRange,
  rotationFromNormal,
  isClockwise,
  arrayFromOcList,
  arrayFromOcVec,
  ocVecFromVec,
  ocDirFromVec,
  ocAx1FromMatrix,
  ocPlnFromMatrix,
  ocListOfShapeFromArray,
  matrixFromOcPln,
  normalFromMatrix,
  vecFromOc,
  collectShapes,
  exploreShape,
  transformGeometry,
  EPSILON,
} from './utils.js'

const DEFAULT_TESSELLATION_TOLERANCE = 0.4
const DEFAULT_ANGULAR_TESSELLATION_TOLERANCE = 0.2
const EDGE_LINEAR_TESSELLATION_RATIO = 0.25
const EDGE_ANGULAR_TESSELLATION_RATIO = 0.5

export function getViewportTessellationTolerance() {
  try {
    const tolerance = new Expression(preferences.tessellationTolerance).getBase()
    return Number.isFinite(tolerance) && tolerance > 0
      ? tolerance
      : DEFAULT_TESSELLATION_TOLERANCE
  } catch(_) {
    return DEFAULT_TESSELLATION_TOLERANCE
  }
}

export function getViewportAngularTessellationTolerance() {
  try {
    const degrees = new Expression(
      preferences.tessellationAngularTolerance,
      [],
      'angle',
    ).getBase()
    const tolerance = THREE.MathUtils.degToRad(degrees)
    return Number.isFinite(tolerance) && tolerance > 0
      ? tolerance
      : DEFAULT_ANGULAR_TESSELLATION_TOLERANCE
  } catch(_) {
    return DEFAULT_ANGULAR_TESSELLATION_TOLERANCE
  }
}


export class Wire {
  constructor(region, startPoint) {
    region = region.map(seg => seg.clone() )
    if(region.length == 0) throw "Wires may not be empty"
    else if(region.length >= 2) {
      // Find starting point from element order
      let bounds = region[0].endpoints()
      let next_bounds = region[1].endpoints()
      let point = startPoint ||
        ((bounds[0].almost(next_bounds[0]) || bounds[0].almost(next_bounds[1])) ? bounds[1] : bounds[0])

      // Flip curves to flow consistently along element order
      region.forEach(tcurve => {
        if(tcurve.endpoints()[1].almost(point)) {
          point = tcurve.endpoints()[0]
          tcurve.flip()
        } else {
          if(!tcurve.endpoints()[0].almost(point)) throw "Wire segments must be connected"
          point = tcurve.endpoints()[1]
        }
      })
    }

    let firstPoint = region[0].endpoints()[0]
    let lastPoint = region.slice(-1)[0].endpoints()[1]
    if(!firstPoint.almost(lastPoint)) throw "Wires must be closed"

    this.segments = region
  }

  isClockwise() {
    const cage = this.cage()
    return isClockwise(cage)
  }

  reverse() {
    this.segments.reverse()
    this.segments.forEach(tcurve => tcurve.flip() )
  }

  cage() {
    let polyline = this.segments.flatMap(curve => [curve.sample(0.0), curve.sample(0.5)] )
    // polyline.push(this.segments[0].endpoints()[0])
    return polyline
  }

  containsPoint(p) {
    const ray = new Line(p, p.clone().add(new THREE.Vector3(999999.0, 0.0, 0.0)))
    // const perElem = this.segments.map(elem => ray.intersect([elem]) )
    // const num_hits = perElem.iter().enumerate().flatMap((i, intersections) => {
    //   intersections.iter().map(isect => match isect {
    //     Cross(_) => 1,
    //     Pierce(_) => {
    //       let j = (i + 1) % this.segments.length
    //       let next_intersections = perElem[j];
    //       if i != j && next_intersections.iter().any(next_isect => matches!(next_isect, Pierce(_)) ) {
    //         0
    //       } else {
    //         1
    //       }
    //     },
    //     _ => 0,
    //   })
    // }).sum()
    const numHits = this.segments.flatMap(elem => ray.intersect([elem]) ).length
    return numHits % 2 != 0
  }

  encloses(other) {
    // Adjacent sketch regions share split edges, but neither is a hole in the
    // other. Testing their shared endpoints makes them look enclosed because
    // those points lie exactly on both wire boundaries.
    const sharesSegment = this.segments.some(elem =>
      other.segments.some(otherElem => elem.id == otherElem.id)
    )
    if(sharesSegment) return false
    return other.segments.every(elem => this.containsPoint(elem.sample(0.5)) )
  }

  tesselate() {
    let polyline = this.segments.flatMap(curve => {
      let poly = curve.tesselate()
      if(!poly) return []
      poly.pop()
      return poly
    })
    return polyline
  }

  geometry() {
    let wire = new window.oc.oc.BRepBuilderAPI_MakeWire_1()
    this.segments.forEach(seg => {
      const geom = seg.geom()
      const plane = new window.oc.oc.gp_Pln_1()
      const curve = seg.to3d ? seg.to3d(plane) : window.oc.oc.GeomAPI.To3d(geom, plane)
      const edge = new window.oc.oc.BRepBuilderAPI_MakeEdge_24(curve).Edge()
      wire.Add_1(edge)
    })
    wire = wire.Wire()
    window.oc.oc.BOPTools_AlgoTools.OrientEdgesOnWire(wire)
    return wire
  }

  transformed(workplane) {
    const shape = transformGeometry(this.geometry(), workplane).Shape()
    return window.oc.oc.TopoDS.Wire_1(shape)
  }
}


export class Profile {
  constructor(sketch, rings) {
    // this.component = comp
    this.sketch = sketch
    this.rings = rings
    this.id = makeID() //XXX is this being used?
  }

  reference() {
    return new ProfileReference(this)
  }

  tesselate() {
    let polyRings = this.rings.map(wire => wire.tesselate() )

    const outerRing = polyRings[0].flatMap(p => [p[0], p[1]] )
    let holeRings = polyRings.slice(1).map(ring => ring.flatMap(p => [p[0], p[1]]) )

    let holeIndices = []
    let indexOffset = outerRing.length / 2 // Each point has 2 coordinates (x, y)
    for(let hole of holeRings) {
      holeIndices.push(indexOffset)
      indexOffset += hole.length / 2
    }

    let indices = earcut([...outerRing, ...holeRings.flat()], holeIndices, 2)

    // Map back to original positions
    const vertices = polyRings.flat()
    let triangulatedVertices = indices.map(i => vertices[i] )

    return {
      positions: triangulatedVertices.flat(),
      normals: triangulatedVertices.flatMap(v => [0.0, 0.0, 1.0]),
    }
  }

  containsPoint(p) {
    return this.rings[0].containsPoint(p) && !this.rings.slice(1).some(wire => wire.containsPoint(p) )
  }

  center() {
    const out = new THREE.Vector3()
    this.rings[0].segments.forEach(seg => out.add(seg.center()) )
    out.divideScalar(this.rings[0].segments.length)
    return out.applyMatrix4(this.sketch.workplane)
  }

  normal() {
    return normalFromMatrix(this.sketch.workplane)
  }

  getBaseId(curve) {
    return curve.id.split('/')[0]
  }

  makeFace() {
    const wires = this.rings.map(ring => ring.transformed(this.sketch.workplane) )
    const face = new window.oc.oc.BRepBuilderAPI_MakeFace_15(wires[0], true)
    wires.slice(1).forEach(wire => face.Add(wire) )
    return face.Face()
  }

  makeCompound(componentId, shape, featureId, originals) {
    const compound = new Compound(componentId, window.oc.oc.TopoDS.Solid_1(shape))

    originals ||= this.rings.flatMap(ring => ring.segments )
    const solid = compound.solids()[0]

    // Name faces
    const faces = solid.faces()
    originals.forEach((seg, i) => {
      faces[i].id = '/' + featureId + '/swept/' + this.getBaseId(seg)
    })

    // Full-turn revolutions of closed profiles can produce a periodic solid
    // with no separate bottom/top cap faces (e.g. a circle becomes a torus).
    // Only name caps when the result actually contains them.
    if(faces.length >= originals.length + 2) {
      faces[faces.length - 2].id = '/' + featureId + '/bottom'
      faces[faces.length - 1].id = '/' + featureId + '/top'
    }

    // Name all edges in new solid according to their connected faces
    solid.edges().forEach(edge => {
      const faces = edge.connectedFaces()
      edge.id = '(' + faces[0].id + '|' + faces[1].id + ')'
    })

    return compound
  }

  extrude(componentId, height, featureId, offset = 0) {
    if(!height) throw { type: 'error', msg: 'Extrusion has no volume' }
    if(window.kernel.modeling.id == 'shapex') {
      const geometry = window.kernel.modeling.extrude(this, height)
      const compound = new Compound(componentId, geometry)
      compound.nameShapexExtrusion(this, featureId)
      if(!offset) return compound
      const translation = this.normal().normalize().multiplyScalar(offset)
      return compound.transform(new THREE.Matrix4().makeTranslation(translation.x, translation.y, translation.z))
    }
    let face = this.makeFace()
    const rot = new THREE.Quaternion().setFromRotationMatrix(this.sketch.workplane)
    if(offset) {
      const translation = new THREE.Vector3(0, 0, offset).applyQuaternion(rot)
      face = transformGeometry(face, new THREE.Matrix4().makeTranslation(translation.x, translation.y, translation.z)).Shape()
    }
    const dir = ocVecFromVec(new THREE.Vector3(0,0,height).applyQuaternion(rot))
    const prism = new window.oc.oc.BRepPrimAPI_MakePrism_1(face, dir, true, true)
    // const args = new window.oc.oc.TopTools_ListOfShape_1()
    // args.Append_1(face)
    // const history = new window.oc.oc.BRepTools_History_2(args, prism)
    return this.makeCompound(componentId, prism.Shape(), 'extrude-' + featureId)
  }

  extrudeThroughAll(componentId, support, featureId, side = 'forward') {
    const fail = msg => { throw { type: 'error', msg } }
    if(!support?.geom || !support.solids().length) fail('Through all requires an existing body')

    const profileDirection = this.normal().normalize()
    if(window.kernel.modeling.id == 'shapex') {
      const origin = new THREE.Vector3().setFromMatrixPosition(this.sketch.workplane)
      const [minimum, maximum] = window.kernel.modeling.extrusionRange(
        support.kernelGeometry, origin, profileDirection,
      )
      const start = side == 'forward' ? 0 : Math.min(0, minimum)
      const end = side == 'reverse' ? 0 : Math.max(0, maximum)
      if(end - start <= EPSILON) fail('Through all does not intersect an existing body in the selected direction')
      // Match OCC's removed-material tool, including separated bodies and
      // curved exit faces. A single prism avoids an internal seam for both sides.
      const tool = this.extrude(componentId, end - start, featureId, start)
      const result = tool.boolean(support, 'intersect', featureId).repair()
      if(!result.solids().length || result.volume() <= EPSILON) {
        fail('Through all does not intersect an existing body in the selected direction')
      }
      if(!result.validate()) fail('Through all produced no valid extrusion volume')
      return result
    }

    const oc = window.oc.oc
    const extrudeSide = sign => {
      const base = this.makeFace()
      const directionVector = profileDirection.clone().multiplyScalar(sign)
      const direction = ocDirFromVec(directionVector)
      const sketchFace = new oc.TopoDS_Face()
      // Ask OpenCascade to cut the support directly. Its feature-only mode can
      // report success while returning no solid when the profile belongs to a
      // detached sketch, so recover the exact swept tool as the material that
      // the direct cut removed from the support.
      const prism = new oc.BRepFeat_MakePrism_2(
        support.geom(), base, sketchFace, direction, 0, false,
      )
      try {
        prism.PerformUntilEnd()
        if(!prism.IsDone()) return

        const remainder = new Compound(componentId, prism.Shape())
        const result = support.boolean(remainder, 'cut')
        if(!result.solids().length || result.volume() <= EPSILON) return

        const baseFace = new Face(null, oc.TopoDS.Face_1(base))
        const baseCenter = baseFace.center()
        const baseArea = baseFace.area()
        const isBase = face => {
          if(face.IsSame(base)) return true
          const candidate = new Face(null, oc.TopoDS.Face_1(face))
          return candidate.center().distanceTo(baseCenter) <= EPSILON &&
            Math.abs(candidate.area() - baseArea) <= EPSILON * Math.max(1, baseArea)
        }
        const sharesSurface = (a, b) => {
          const first = oc.BRep_Tool.Surface_2(oc.TopoDS.Face_1(a))
          const second = oc.BRep_Tool.Surface_2(oc.TopoDS.Face_1(b))
          try { return first.get().isAliasOf(second.get()) }
          finally { first.delete(); second.delete() }
        }
        const originals = this.rings.flatMap(ring => ring.segments)
        const sources = collectShapes(base, 'edge').map((edge, index) => ({
          id: '/extrude-' + featureId + '/swept/' + this.getBaseId(originals[index]),
          generated: arrayFromOcList(prism.Generated(edge)),
        }))
        const origin = this.center()
        const faceNamesUsed = {}
        result.solids().forEach(solid => {
          solid.faces().forEach(face => {
            const source = sources.find(source =>
              source.generated.some(generated => sharesSurface(face.geom(), generated))
            )
            const cap = isBase(face.geom()) ? (sign > 0 ? 'bottom' : 'top') :
              (face.center().sub(origin).dot(profileDirection) < 0 ? 'bottom' : 'top')
            const name = source?.id || '/extrude-' + featureId + '/' + cap
            faceNamesUsed[name] = (faceNamesUsed[name] || 0) + 1
            face.id = name + (faceNamesUsed[name] > 1 ? '/' + faceNamesUsed[name] : '')
          })
        })

        const edgeNamesUsed = {}
        result.solids().flatMap(solid => solid.edges()).forEach(edge => {
          const name = '(' + edge.connectedFaces().map(face => face.id).join('|') + ')'
          edgeNamesUsed[name] = (edgeNamesUsed[name] || 0) + 1
          edge.id = name + '/' + edgeNamesUsed[name]
        })
        return result
      } finally {
        prism.delete()
        sketchFace.delete()
        direction.delete()
      }
    }

    const signs = side == 'both' ? [1, -1] : [side == 'reverse' ? -1 : 1]
    const halves = signs.map(extrudeSide).filter(Boolean)
    if(!halves.length) fail('Through all does not intersect an existing body in the selected direction')
    const result = halves.slice(1).reduce((combined, half) => combined.boolean(half, 'join'), halves[0]).repair()
    if(!result.solids().length || !result.validate() || result.volume() <= EPSILON) {
      fail('Through all produced no valid extrusion volume')
    }
    return result
  }

  extrudeUpToFace(componentId, target, featureId, offset = 0) {
    if(window.kernel.modeling.id == 'shapex') {
      if(!target) throw { type: 'error', msg: 'Select a face to extrude up to' }
      return new Compound(componentId, window.kernel.modeling.extrudeUpToFace(this, target, offset))
        .inheritShapexTopology([], 'extrude', featureId).nameShapexExtrusion(this, featureId)
    }
    const oc = window.oc.oc
    const fail = msg => { throw { type: 'error', msg } }
    if(!target) fail('Select a face to extrude up to')

    const direction = this.normal().normalize()
    const origin = this.center()
    const plane = target.getPlane()
    if(plane) {
      const normal = normalFromMatrix(plane)
      const denominator = normal.dot(direction)
      if(Math.abs(denominator) <= oc.Precision.Angular()) fail('The target plane is parallel to the extrusion direction')
      // The location of the trimmed face is irrelevant: use the intersection
      // with its underlying plane to choose the extrusion direction.
      const distance = new THREE.Vector3().setFromMatrixPosition(plane).sub(origin).dot(normal) / denominator
      if(distance < 0) direction.negate()
    } else if(target.center().sub(origin).dot(direction) < 0) {
      direction.negate()
    }

    // Work along local +Z with the offset start at the origin.
    const frame = rotationFromNormal(direction).setPosition(origin.clone().addScaledVector(direction, offset))
    const inverseFrame = frame.clone().invert()
    const planeGeometry = plane && ocPlnFromMatrix(inverseFrame.clone().multiply(plane))
    const planeBuilder = plane && new oc.BRepBuilderAPI_MakeFace_3(planeGeometry)
    const targetTransform = !plane && transformGeometry(target.solid.geom(), inverseFrame)
    const targetShape = planeBuilder ? planeBuilder.Face() : targetTransform.ModifiedShape(target.geom())
    const baseTransform = transformGeometry(this.makeFace(), frame.clone().setPosition(origin).invert())
    const base = baseTransform.Shape()
    const prismDirection = ocDirFromVec(new THREE.Vector3(0, 0, 1))
    const sketchFace = new oc.TopoDS_Face()
    // Mode 2 returns just the extrusion; Feature applies the chosen Boolean.
    // Curved limits need their owning solid as the basis for face selection.
    const prism = new oc.BRepFeat_MakePrism_2(
      targetTransform ? targetTransform.Shape() : base, base, sketchFace, prismDirection, 2, false,
    )
    const originals = this.rings.flatMap(ring => ring.segments)
    try {
      prism.Perform_2(targetShape)
      if(!prism.IsDone()) fail('The selected face could not trim the extrusion')

      const candidates = collectShapes(prism.Shape(), 'solid').filter(solid =>
        collectShapes(solid, 'face').some(face => face.IsSame(base))
      )
      if(candidates.length != 1) fail(plane ?
        'The target plane must lie entirely beyond the extrusion start' :
        'The selected face must cover the entire extrusion beyond its start')

      const result = new Compound(componentId, candidates[0])
      if(result.center().z <= EPSILON) fail('The selected face is behind the extrusion start')
      const solid = result.solids()[0]
      const sources = collectShapes(base, 'edge').map((edge, index) => ({
        id: '/extrude-' + featureId + '/swept/' + this.getBaseId(originals[index]),
        edge,
      }))
      solid.faces().forEach(face => {
        if(face.geom().IsSame(base)) {
          face.id = '/extrude-' + featureId + '/bottom'
          return
        }
        // Generated() can retain the untrimmed sides for curved limits.
        // The unchanged base edges identify their actual adjacent side faces.
        const edges = collectShapes(face.geom(), 'edge')
        const source = sources.find(source => edges.some(edge => edge.IsSame(source.edge)))
        face.id = source?.id || '/extrude-' + featureId + '/top'
      })
      const namesUsed = {}
      solid.edges().forEach(edge => {
        const name = '(' + edge.connectedFaces().map(face => face.id).join('|') + ')'
        namesUsed[name] = (namesUsed[name] || 0) + 1
        edge.id = name + '/' + namesUsed[name]
      })
      if(!result.validate() || result.volume() <= EPSILON) fail('Up to face produced no valid extrusion volume')
      return result.transform(frame)
    } finally {
      prism.delete()
      sketchFace.delete()
      prismDirection.delete()
      if(planeBuilder) planeBuilder.delete()
      if(planeGeometry) planeGeometry.delete()
      if(targetTransform) targetTransform.delete()
      baseTransform.delete()
    }
  }

  revolve(componentId, axis, angle, featureId, offset = 0) {
    if(!angle) throw { type: 'error', msg: 'Revolution has no volume' }
    if(window.kernel.modeling.id == 'shapex') {
      const geometry = window.kernel.modeling.revolve(this, axis, angle)
      const compound = new Compound(componentId, geometry).inheritShapexTopology([], 'revolve', featureId)
      if(!offset) return compound
      const turn = axis.clone().multiply(new THREE.Matrix4().makeRotationZ(offset)).multiply(axis.clone().invert())
      return compound.transform(turn)
    }
    let face = this.makeFace()
    if(offset) {
      const turn = axis.clone().multiply(new THREE.Matrix4().makeRotationZ(offset)).multiply(axis.clone().invert())
      face = transformGeometry(face, turn).Shape()
    }
    const ax = ocAx1FromMatrix(axis)
    let revolution = new window.oc.oc.BRepPrimAPI_MakeRevol_1(face, ax, angle, true)

    // A profile segment lying on the revolution axis collapses instead of
    // producing a swept face. Exclude it from the positional face-name map.
    const axisOrigin = new THREE.Vector3().setFromMatrixPosition(axis)
    const axisDirection = normalFromMatrix(axis)
    const originals = this.rings
      .flatMap(ring => ring.segments )
      .filter(segment => ![0.0, 0.5, 1.0].every(u => {
        const point = segment.sample(u).applyMatrix4(this.sketch.workplane)
        return point.sub(axisOrigin).cross(axisDirection).length() < EPSILON
      }))

    return this.makeCompound(componentId, revolution.Shape(), 'revolve-' + featureId, originals)
  }

  revolveUpToFace(componentId, axis, target, featureId, forward = true, from = null) {
    if(window.kernel.modeling.id == 'shapex') {
      if(!target) throw { type: 'error', msg: 'Select a face to revolve up to' }
      return new Compound(componentId, window.kernel.modeling.revolveUpToFace(this, axis, target, forward))
        .inheritShapexTopology([], 'revolve', featureId)
    }
    const oc = window.oc.oc
    const fail = msg => { throw { type: 'error', msg } }
    if(!target) fail('Select a face to revolve up to')

    const frame = axis.clone()
    if(!forward) frame.multiply(new THREE.Matrix4().makeRotationX(Math.PI))
    const inverseAxis = frame.clone().invert()
    const plane = target.getPlane()
    const localStart = inverseAxis.clone().multiply(this.sketch.workplane)
    const localPlane = plane && inverseAxis.clone().multiply(plane)
    if(localPlane) {
      // The two-plane overload expects matching crossing orientations at
      // both limits. Orient the target using the first forward intersection.
      const center = this.center().applyMatrix4(inverseAxis)
      const normal = normalFromMatrix(localPlane)
      const point = new THREE.Vector3().setFromMatrixPosition(localPlane)
      const a = normal.x * center.x + normal.y * center.y
      const b = -normal.x * center.y + normal.y * center.x
      const c = normal.z * center.z - normal.dot(point)
      const radius = Math.hypot(a, b)
      if(radius <= EPSILON || Math.abs(c) >= radius) fail('The target plane does not cross the revolution')
      const phase = Math.atan2(b, a)
      const delta = Math.acos(-c / radius)
      const angles = [phase - delta, phase + delta].map(angle => (angle % (2 * Math.PI) + 2 * Math.PI) % (2 * Math.PI))
      const angle = Math.min(...angles)
      if(angle <= oc.Precision.Angular()) fail('The target plane intersects the starting profile')
      const startNormal = normalFromMatrix(localStart)
      const startPoint = new THREE.Vector3().setFromMatrixPosition(localStart)
      if(Math.abs(normal.z) <= oc.Precision.Angular() && Math.abs(normal.dot(point)) <= EPSILON &&
         Math.abs(startNormal.z) <= oc.Precision.Angular() && Math.abs(startNormal.dot(startPoint)) <= EPSILON) {
        // Radial planes define an exact angle, including profiles touching
        // the axis where the limited-feature algorithm can lose the start cap.
        return this.revolve(componentId, axis, forward ? angle : -angle, featureId)
      }
      if(angle > Math.PI) {
        // An infinite start plane also intersects the sweep at 180 degrees.
        // Advance past that ambiguity only when the first half misses the limit.
        const first = this.revolve(componentId, axis, forward ? Math.PI : -Math.PI, featureId)
        const limitPlane = ocPlnFromMatrix(plane)
        const section = new oc.BRepAlgoAPI_Section_5(first.geom(), limitPlane, true)
        try {
          if(!section.IsDone() || collectShapes(section.Shape(), 'vertex').length) {
            fail('The target plane crosses the profile at incompatible revolution angles')
          }
        } finally {
          section.delete()
          limitPlane.delete()
        }
        const turn = axis.clone().multiply(new THREE.Matrix4().makeRotationZ(Math.PI)).multiply(axis.clone().invert())
        const remaining = this.clone()
        remaining.sketch = { workplane: turn.clone().multiply(this.sketch.workplane) }
        remaining.makeFace = () => oc.TopoDS.Face_1(transformGeometry(this.makeFace(), turn).Shape())
        return first.boolean(remaining.revolveUpToFace(componentId, axis, target, featureId, forward), 'join').repair()
      }
      const crossing = -a * Math.sin(angle) + b * Math.cos(angle)
      const startCrossing = normalFromMatrix(localStart).dot(new THREE.Vector3(-center.y, center.x, 0))
      if(crossing * startCrossing < 0) localPlane.multiply(new THREE.Matrix4().makeRotationX(Math.PI))
    }
    const planeGeometry = plane && ocPlnFromMatrix(localPlane)
    const planeBuilder = plane && new oc.BRepBuilderAPI_MakeFace_3(planeGeometry)
    const targetTransform = !plane && transformGeometry(target.solid.geom(), inverseAxis)
    const targetShape = planeBuilder ? planeBuilder.Face() : targetTransform.ModifiedShape(target.geom())
    const fromTransform = from && (from.solid.geom().IsSame(target.solid.geom()) ? targetTransform :
      transformGeometry(from.solid.geom(), inverseAxis))
    const fromShape = fromTransform && fromTransform.ModifiedShape(from.geom())
    const baseTransform = transformGeometry(this.makeFace(), inverseAxis)
    const base = baseTransform.Shape()
    const startPlane = plane && ocPlnFromMatrix(localStart)
    const startBuilder = plane && new oc.BRepBuilderAPI_MakeFace_3(startPlane)
    const startShape = startBuilder ? startBuilder.Face() : base
    const rotationAxis = ocAx1FromMatrix(new THREE.Matrix4())
    const sketchFace = new oc.TopoDS_Face()
    const support = fromTransform && fromTransform != targetTransform ? new Compound(componentId, targetTransform.Shape())
      .merge([new Compound(componentId, fromTransform.Shape())]).geom() :
      targetTransform ? targetTransform.Shape() : base
    // Return the revolved tool only; the feature applies Join/Cut/Intersect.
    const revolution = new oc.BRepFeat_MakeRevol_2(
      support, base, sketchFace, rotationAxis, 2, false,
    )
    try {
      // The single-limit planar path in this kernel ignores feature-only mode.
      // The two-plane overload honors it and avoids adding the target body.
      if(fromShape) revolution.Perform_3(fromShape, targetShape)
      else if(plane) revolution.Perform_3(startShape, targetShape)
      else revolution.Perform_2(targetShape)
      if(!revolution.IsDone()) fail('The selected face could not trim the revolution')

      const baseFace = new Face(null, oc.TopoDS.Face_1(base))
      const baseCenter = baseFace.center()
      const baseArea = baseFace.area()
      const sharesSurface = (a, b) => {
        const first = oc.BRep_Tool.Surface_2(oc.TopoDS.Face_1(a))
        const second = oc.BRep_Tool.Surface_2(oc.TopoDS.Face_1(b))
        try { return first.get().isAliasOf(second.get()) }
        finally { first.delete(); second.delete() }
      }
      const isStart = face => {
        if(fromShape) return sharesSurface(face, fromShape)
        if(face.IsSame(base)) return true
        if(!plane) return false
        const cap = new Face(null, face)
        const capPlane = cap.getPlane()
        if(!capPlane || Math.abs(normalFromMatrix(capPlane).dot(normalFromMatrix(localStart))) < 1 - EPSILON) return false
        return cap.center().distanceTo(baseCenter) <= EPSILON &&
          Math.abs(cap.area() - baseArea) <= EPSILON * Math.max(1, baseArea)
      }
      // The two-limit builder can also return pieces outside the limits.
      // Keep only a solid with a cap on each selected surface.
      const candidates = collectShapes(revolution.Shape(), 'solid').filter(solid => {
        const faces = collectShapes(solid, 'face')
        return faces.some(isStart) && (!fromShape || faces.some(face => sharesSurface(face, targetShape)))
      })
      if(candidates.length != 1) fail(fromShape ? 'The limiting faces do not enclose a valid revolution' :
        'The selected face must cap the entire profile along the revolution')

      const result = new Compound(componentId, candidates[0])
      if(!result.validate() || result.volume() <= EPSILON) fail('Up to face produced no valid revolution volume')
      const originals = this.rings.flatMap(ring => ring.segments)
      const sources = collectShapes(base, 'edge').map((edge, index) => ({
        edge,
        generated: arrayFromOcList(revolution.Generated(edge)),
        id: '/revolve-' + featureId + '/swept/' + this.getBaseId(originals[index]),
      })).filter(source => source.generated.length)
      const solid = result.solids()[0]
      const faceNamesUsed = {}
      solid.faces().forEach(face => {
        if(isStart(face.geom())) {
          face.id = '/revolve-' + featureId + '/bottom'
          return
        }
        const edges = collectShapes(face.geom(), 'edge')
        const source = sources.find(source => edges.some(edge => edge.IsSame(source.edge)) ||
          source.generated.some(shape => sharesSurface(face.geom(), shape)))
        const name = source?.id || '/revolve-' + featureId + '/top'
        faceNamesUsed[name] = (faceNamesUsed[name] || 0) + 1
        face.id = name + (faceNamesUsed[name] > 1 ? '/' + faceNamesUsed[name] : '')
      })
      const namesUsed = {}
      solid.edges().forEach(edge => {
        const name = '(' + edge.connectedFaces().map(face => face.id).join('|') + ')'
        namesUsed[name] = (namesUsed[name] || 0) + 1
        edge.id = name + '/' + namesUsed[name]
      })
      // Merge the two-limit sweep at the profile plane before transforming
      // its periodic surface; repair also fixes the unified face's seam.
      return (fromShape ? result.repair() : result).transform(frame)
    } finally {
      revolution.delete()
      sketchFace.delete()
      rotationAxis.delete()
      baseTransform.delete()
      if(planeBuilder) planeBuilder.delete()
      if(planeGeometry) planeGeometry.delete()
      if(targetTransform) targetTransform.delete()
      if(fromTransform && fromTransform != targetTransform) fromTransform.delete()
      if(startBuilder) startBuilder.delete()
      if(startPlane) startPlane.delete()
    }
  }

  revolveBetweenFaces(componentId, axis, from, until, featureId, forward = true) {
    const fail = msg => { throw { type: 'error', msg } }
    if(!from || !until) fail('Select both limiting faces')
    if(window.kernel.modeling.id == 'shapex') {
      return new Compound(componentId, window.kernel.modeling.revolveBetweenFaces(this, axis, from, until, forward))
        .inheritShapexTopology([], 'revolve', featureId)
    }
    if(from.geom().IsSame(until.geom())) fail('Choose different starting and ending faces')
    // Build curved limits together: subtracting independent periodic sweeps
    // can lose their coincident side faces and falsely report crossing limits.
    if(!from.getPlane() && !until.getPlane()) {
      return this.revolveUpToFace(componentId, axis, until, featureId, forward, from)
    }

    const startsAtSketch = face => {
      const plane = face.getPlane()
      if(!plane) return false
      const normal = normalFromMatrix(this.sketch.workplane)
      const origin = new THREE.Vector3().setFromMatrixPosition(this.sketch.workplane)
      const position = new THREE.Vector3().setFromMatrixPosition(plane)
      return Math.abs(normal.dot(normalFromMatrix(plane))) >= 1 - EPSILON &&
        Math.abs(position.sub(origin).dot(normal)) <= EPSILON
    }
    const fromAtSketch = startsAtSketch(from)
    const untilAtSketch = startsAtSketch(until)
    if(fromAtSketch && untilAtSketch) fail('The limiting planes coincide')

    // Compose the validated single-limit sweeps. This also handles mixed
    // planar/curved limits, which the native two-limit overload rejects.
    const start = fromAtSketch ? null : this.revolveUpToFace(componentId, axis, from, featureId + '/from', forward)
    const end = untilAtSketch ? null : this.revolveUpToFace(componentId, axis, until, featureId + '/until', forward)
    let result
    if(!start) {
      result = end
    } else if(!end) {
      result = this.revolve(componentId, axis, forward ? 2 * Math.PI : -2 * Math.PI, featureId).boolean(start, 'cut')
    } else {
      const startOutsideEnd = start.boolean(end, 'cut')
      const endOutsideStart = end.boolean(start, 'cut')
      if(!startOutsideEnd.solids().length) {
        result = endOutsideStart
      } else if(!endOutsideStart.solids().length) {
        // The end is reached after passing the sketch again.
        result = this.revolve(componentId, axis, forward ? 2 * Math.PI : -2 * Math.PI, featureId)
          .boolean(start, 'cut').boolean(end, 'join')
      } else {
        fail('The limiting faces cross within the revolved profile')
      }
    }
    if(result.solids().length != 1 || !result.validate() || result.volume() <= EPSILON) {
      fail('The limiting faces do not enclose a valid revolution')
    }

    const prefix = '/revolve-' + featureId
    const solid = result.solids()[0]
    solid.faces().forEach(face => {
      face.id = face.id.replace(prefix + '/from/top', prefix + '/bottom')
        .replace(prefix + '/from/bottom', prefix + '/top')
        .replace(prefix + '/until/top', prefix + '/top')
        .replace(prefix + '/from/', prefix + '/')
        .replace(prefix + '/until/', prefix + '/')
    })
    const namesUsed = {}
    solid.edges().forEach(edge => {
      const name = '(' + edge.connectedFaces().map(face => face.id).join('|') + ')'
      namesUsed[name] = (namesUsed[name] || 0) + 1
      edge.id = name + '/' + namesUsed[name]
    })
    return result.repair()
  }

  loft(componentId, others, untwist, featureId) {
    const profiles = [this, ...others]
    if(profiles.some(profile => profile.rings.length != 1)) {
      throw { type: 'error', msg: 'Loft profiles with holes are not supported' }
    }
    if(window.kernel.modeling.id == 'shapex') {
      const geometry = window.kernel.modeling.loft(profiles, untwist)
      return new Compound(componentId, geometry).inheritShapexTopology([], 'loft', featureId)
    }

    const wires = profiles.map(profile =>
      profile.rings[0].transformed(profile.sketch.workplane)
    )
    const loft = new window.oc.oc.BRepOffsetAPI_ThruSections(true, false, 1.0e-06)
    wires.forEach(wire => loft.AddWire(wire) )
    loft.CheckCompatibility(untwist)
    loft.Build(new window.oc.oc.Message_ProgressRange_1())
    if(!loft.IsDone()) throw { type: 'error', msg: 'Loft could not be built' }

    const shape = loft.Shape()
    return new Compound(componentId, window.oc.oc.TopoDS.Solid_1(shape))
  }

  sweep(componentId, rail, bounds, featureId) {
    try {
      if(this.rings.length != 1) {
        throw { type: 'error', msg: 'Sweeps with holes are not supported' }
      }
      if(bounds[0] != 0.0 || bounds[1] != 1.0) {
        throw { type: 'error', msg: 'Partial rail sweeps are not supported' }
      }

      if(window.kernel.modeling.id == 'shapex') {
        const geometry = window.kernel.modeling.sweep(this, rail)
        return new Compound(componentId, geometry).inheritShapexTopology([], 'sweep', featureId)
      }

      // A wire profile produces a shell. Use the filled face so MakePipe
      // returns a solid that can participate in the feature boolean.
      const profile = this.makeFace()
      const plane = new window.oc.oc.gp_Pln_1()
      const curve = window.oc.oc.GeomAPI.To3d(rail.geom(), plane)
      const edge = new window.oc.oc.BRepBuilderAPI_MakeEdge_24(curve).Edge()
      const pathBuilder = new window.oc.oc.BRepBuilderAPI_MakeWire_1()
      pathBuilder.Add_1(edge)
      const path = window.oc.oc.TopoDS.Wire_1(
        transformGeometry(pathBuilder.Wire(), rail.sketch.workplane).Shape()
      )
      const pipe = new window.oc.oc.BRepOffsetAPI_MakePipe_1(path, profile)
      pipe.Build(new window.oc.oc.Message_ProgressRange_1())
      if(!pipe.IsDone()) throw { type: 'error', msg: 'Sweep could not be built' }

      return new Compound(componentId, window.oc.oc.TopoDS.Solid_1(pipe.Shape()))
    } catch(err) {
      if(typeof err == 'number') {
        const data = window.oc.oc.OCJS.getStandard_FailureData(err)
        throw { type: 'error', msg: data.GetMessageString() || 'OpenCascade sweep failed' }
      }
      throw err
    }
  }

  update() {
    const elements = this.sketch.profileElements()
    const cutElements = elements.flatMap(elem => elem.split(elements) )
    const newWires = this.sketch.getWires(cutElements, false)

    if(this.serializedRingIds) {
      const rings = this.serializedRingIds.map(ring =>
        ring.map(segId => cutElements.find(elem => elem.id == segId))
      )
      if(rings.some(ring => ring.some(seg => !seg))) {
        return { type: 'error', msg: "Profile was lost" }
      }
      this.rings = rings.map(ring => new Wire(ring))
      this.serializedRingIds = null
    }

    let wasRepairNeeded = false
    let error
    this.rings = this.rings.map(wire => {
      const segmentIds = new Set(wire.segments.map(seg => seg.id))
      const wireIds = new Set(wire.segments.map(seg => this.getBaseId(seg) ))
      const replacementWire = newWires.map(newWire => {
        const newSegmentIds = new Set(newWire.segments.map(tcurve => tcurve.id))
        const newWireIds = new Set(newWire.segments.map(tcurve => this.getBaseId(tcurve) ))
        const exactMatched = segmentIds.intersection(newSegmentIds).size
        const baseMatched = wireIds.intersection(newWireIds).size
        if(baseMatched > 0) {
          return [exactMatched, baseMatched, newSegmentIds.size, newWire]
        }
      }).filter(Boolean).sort((a, b) =>
        b[0] - a[0] ||
        b[1] - a[1] ||
        Math.abs(segmentIds.size - a[2]) - Math.abs(segmentIds.size - b[2])
      )[0]
      if(!replacementWire) {
        error = { type: 'error', msg: "Profile was lost" }
        return wire
      }
      const [exactMatched, , newSize, newWire] = replacementWire
      if(exactMatched != segmentIds.size || exactMatched != newSize) wasRepairNeeded = true
      return newWire
    })
    if(error) return error
    if(wasRepairNeeded) return { type: 'warning', msg: "Profile has been repaired" }
  }

  clone() {
    const clone = new Profile(this.sketch, [...this.rings])
    clone.id = this.id
    clone.serializedRingIds = this.serializedRingIds
    return clone
  }
}


// BREP

export class Shape {
  isSame(otherOrId) {
    return this.id == (otherOrId.id || otherOrId)
  }

  area() {
    if(!this.geom) return 0
    if(this.isShapex || this.solid?.compound?.isShapex || this.compound?.isShapex) {
      return meshAreaAndCenter(this.tesselate()).area
    }
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return gprops.Mass() // Mass really corresponds to surface area here
  }

  center() {
    if(this.solid?.compound?.isShapex) return meshAreaAndCenter(this.tesselate()).center
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return vecFromOc(gprops.CentreOfMass())
  }

  collectShapes(type) {
    const geom = this.geom()

    const constructors = {
      edge: Edge,
      face: Face,
      solid: Solid,
    }

    new window.oc.oc.BRepMesh_IncrementalMesh_2(
      geom,
      getViewportTessellationTolerance(), // linear deflection
      false,
      getViewportAngularTessellationTolerance(),
      false
    )

    return collectShapes(geom, type)
      .map(item => new constructors[type](this, item) )
  }
}


export class Volumetric extends Shape {
  // static repair(shape) {
  //   const purge = new window.oc.oc.TopOpeBRepTool_FuseEdges(shape, true)
  //   purge.Perform()
  //   shape = purge.Shape()

  //   shape = new window.oc.oc.ShapeUpgrade_ShellSewing().ApplySewing(shape, 0.01)

  //   console.log('open', window.oc.oc.BOPTools_AlgoTools.IsOpenShell(shell))
  //   const explorer = new window.oc.oc.TopExp_Explorer_2(shape, window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHELL, window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHAPE)
  //   while(explorer.More()) {
  //     const shell = window.oc.oc.TopoDS.Shell_1(explorer.Current())
  //     window.oc.oc.BOPTools_AlgoTools.OrientFacesOnShell(shell)
  //     explorer.Next()
  //   }

  //   return shape
  // }

  validate() {
    if(this.isShapex || this.compound?.isShapex) return true
    const analyzer = new window.oc.oc.BRepCheck_Analyzer(this.geom(), true, false)
    return analyzer.IsValid_2()
  }

  volume() {
    if(!this.geom) return 0
    if(this.isShapex || this.compound?.isShapex) {
      return window.kernel.modeling.volume(this.geom())
    }
    const gprops = new window.oc.oc.GProp_GProps_1() //XXX use GProp_GProps_2 to supply point close to center for better accuracy
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return gprops.Mass()
  }

  center() {
    if(!this.geom) return
    if(this.isShapex || this.compound?.isShapex) {
      return window.kernel.modeling.center(this.geom())
    }
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return vecFromOc(gprops.CentreOfMass())
  }
}


export class Face extends Shape {
  constructor(solid, geom, id, tessId) {
    super()
    this.solid = solid
    this.geom = () => geom
    this.id = id
    if(solid.compound.isShapex) this.kernelId = window.kernel.modeling.entityId(geom)
    this.tessId = tessId || makeID()
  }

  planarReference() {
    if(!this.getPlane()) throw "Cannot provide plane from non-planar face"
    return new PlanarReference(new FaceReference(this))
  }

  axialReference() {
    if(!this.getAxis()) throw "Cannot provide axis from non-cylindrical face"
    return new AxialReference(new FaceReference(this))
  }

  pointReference() {
    if(!this.getPoint()) throw "Cannot provide point from non-spherical face"
    return new PointReference(new FaceReference(this))
  }

  faceReference() {
    return new FaceReference(this)
  }

  getPlane() {
    if(this.solid.compound.isShapex) {
      return window.kernel.modeling.facePlane(this.geom())
    }
    const surface = window.oc.oc.BRep_Tool.Surface_2(this.geom())
    const isPlanar = new window.oc.oc.GeomLib_IsPlanarSurface(surface, 1.0e-7) //XXX Surface->IsKind(STANDARD_TYPE(Geom_Plane)) could be faster
    if(!isPlanar.IsPlanar()) return
    const plane = isPlanar.Plan()
    return matrixFromOcPln(plane)
  }

  getAxis() {
    if(this.solid.compound.isShapex) {
      return window.kernel.modeling.faceAxis(this.geom())
    }
    const surface = new window.oc.oc.BRepAdaptor_Surface_2(this.geom(), true)
    try {
      // OpenCascade raises Standard_NoSuchObject when the adapted surface is
      // not cylindrical. GeomAbs_SurfaceType is not part of our WASM export.
      const axis = surface.Cylinder().Axis()
      const direction = vecFromOc(axis.Direction()).normalize()
      const location = vecFromOc(axis.Location())
      return rotationFromNormal(direction).setPosition(location)
    } catch(_) {}
  }

  getPoint() {
    if(this.solid.compound.isShapex) {
      return window.kernel.modeling.facePoint(this.geom())
    }
    const surface = new window.oc.oc.BRepAdaptor_Surface_2(this.geom(), true)
    try {
      const location = vecFromOc(surface.Sphere().Location())
      return new THREE.Matrix4().setPosition(location)
    } catch(_) {}
  }

  normal(u, v, useCurvature) { //XXX use BOPTools_AlgoTools3D::GetNormalToSurface or BOPTools_AlgoTools3D::GetNormalToFaceOnEdge
    if(this.solid.compound.isShapex) {
      const plane = this.getPlane()
      if(plane) return normalFromMatrix(plane)
      const normals = this.tesselate().normals
      return normals.length ? new THREE.Vector3(...normals.slice(0, 3)).normalize() : undefined
    }
    const geom = this.geom()
    const uMin = { current: 0 }
    const uMax = { current: 0 }
    const vMin = { current: 0 }
    const vMax = { current: 0 }
    window.oc.oc.BRepTools.UVBounds_1(geom.get(), uMin, uMax, vMin, vMax)
    u = uMin.current + (uMax.current - uMin.current) * u
    v = vMin.current + (vMax.current - vMin.current) * v
    const surface = window.oc.oc.BRep_Tool.Surface_2(geom)
    const props = new window.oc.oc.GeomLProp_SLProps_1(surface, u, v, useCurvature ? 2 : 1, 0.01)
    return vecFromOc(props.Normal())
  }

  connectedFaces() {
    if(this.solid.compound.isShapex) {
      const edgeIds = new Set(window.kernel.modeling.faceBoundaryEdgeIds(this.geom()))
      return this.solid.edges()
        .filter(edge => edgeIds.has(edge.kernelId))
        .flatMap(edge => edge.connectedFaces())
        .filter((face, index, faces) => face && face != this && faces.indexOf(face) == index)
    }
    const edgeFaceMap = new window.oc.oc.TopTools_IndexedDataMapOfShapeListOfShape_1()
    window.oc.oc.TopExp.MapShapesAndAncestors(
      this.solid.geom(),
      window.oc.oc.TopAbs_ShapeEnum.TopAbs_EDGE,
      window.oc.oc.TopAbs_ShapeEnum.TopAbs_FACE,
      edgeFaceMap
    )
    const neighboors = []
    exploreShape(this.geom(), 'edge', edge => {
      let sharedFaces = arrayFromOcList(edgeFaceMap.ChangeFromKey(edge))
      neighboors.push(...sharedFaces.filter(sharedFace => !sharedFace.IsSame(this.geom()) ))
    })
    const unique = [...new Set(neighboors)]
    return unique.map(f =>
      this.solid.faces().find(sf => sf.geom().IsSame(f) )
    )
  }

  tesselate() {
    if(this.cachedTesselation) return this.cachedTesselation
    if(this.solid.compound.isShapex) {
      this.cachedTesselation = this.solid.shapexFaceTessellation(this)
      return this.cachedTesselation
    }
    const location = new window.oc.oc.TopLoc_Location_1()
    const triangulation = window.oc.oc.BRep_Tool.Triangulation(this.geom(), location, 0).get()
    let positions = []
    let normals = []
    if(triangulation) {
      triangulation.ComputeNormals()
      arrayRange(1, triangulation.NbTriangles()).forEach(i => {
        const triangle = triangulation.Triangle(i)
        const pos = arrayRange(1, 3).flatMap(j => arrayFromOcVec(triangulation.Node(triangle.Value(j))) )
        const norm = arrayRange(1, 3).flatMap(j => arrayFromOcVec(triangulation.Normal_1(triangle.Value(j))) )
        positions = positions.concat(pos)
        normals = normals.concat(norm)
      })
    }
    this.cachedTesselation = {
      positions,
      normals,
    }
    return this.cachedTesselation
  }
}


export class Edge extends Shape {
  constructor(solid, geom, id, tessId) {
    super()
    this.solid = solid
    this.geom = () => geom
    this.id = id
    if(solid.compound.isShapex) this.kernelId = window.kernel.modeling.entityId(geom)
    this.tessId = tessId || makeID()
  }

  axialReference() {
    if(!this.getAxis()) throw "Cannot provide axis from non-linear edge"
    return new AxialReference(this.edgeReference())
  }

  edgeReference() {
    return new EdgeReference(this)
  }

  isSemantic() {
    return !this.solid.compound.isShapex
      || window.kernel.modeling.isSemanticEdge(this.solid.compound.geom(), this.geom())
  }

  getAxis() {
    if(this.solid.compound.isShapex) {
      return window.kernel.modeling.edgeAxis(this.geom())
    }
    // BRep_Tool.Curve()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    if(curve.GetType() != window.oc.oc.GeomAbs_CurveType.GeomAbs_Line) return
    const line = curve.Line()
    const dir = vecFromOc(line.Direction()).normalize()
    const loc = vecFromOc(line.Location())
    return rotationFromNormal(dir).setPosition(loc)
  }

  connectedFaces() {
    if(this.solid.compound.isShapex) {
      const ids = window.kernel.modeling.edgeConnectedFaceIds(this.geom())
      return ids.map(id => this.solid.faces().find(face => face.kernelId == id)).filter(Boolean)
    }
    const edgeFaceMap = new window.oc.oc.TopTools_IndexedDataMapOfShapeListOfShape_1()
    window.oc.oc.TopExp.MapShapesAndAncestors(
      this.solid.geom(),
      window.oc.oc.TopAbs_ShapeEnum.TopAbs_EDGE,
      window.oc.oc.TopAbs_ShapeEnum.TopAbs_FACE,
      edgeFaceMap
    )
    let faces = edgeFaceMap.ChangeFromKey(this.geom())
    return [faces.First_1(), faces.Last_1()].map(f =>
      this.solid.faces().find(sf => sf.geom().IsSame(f) )
    )
  }

  center() {
    if(this.solid.compound.isShapex) {
      const points = this.tesselate()
      if(!points.length) return new THREE.Vector3()
      const middle = (points.length - 1) / 2
      const lower = points[Math.floor(middle)]
      const upper = points[Math.ceil(middle)]
      return lower.clone().add(upper).multiplyScalar(0.5)
    }
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    const middle = (curve.FirstParameter() + curve.LastParameter()) / 2.0
    return vecFromOc(curve.Value(middle))
  }

  vertices() {
    if(this.solid.compound.isShapex) {
      return window.kernel.modeling.edgeEndpoints(this.geom())
    }
    return collectShapes(this.geom(), 'vertex').map(vertex => vecFromOc(window.oc.oc.BRep_Tool.Pnt(vertex)) )
  }

  tesselate() {
    if(this.cachedTesselation) return this.cachedTesselation
    if(this.solid.compound.isShapex) {
      this.cachedTesselation = window.kernel.modeling.edgePolyline(this.geom())
      return this.cachedTesselation
    }
    // const location = new window.oc.oc.TopLoc_Location_1()
    // const poly = window.oc.oc.BRep_Tool.Polygon3D(this.geom(), location).get()
    // const nodes = poly.Nodes()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    this.cachedTesselation = tesselateCurve(curve)
    return this.cachedTesselation
  }
}


export class Solid extends Volumetric {
  constructor(compound, geom) {
    super()
    this.compound = compound
    this.geom = () => geom
  }

  typename() { return 'Solid' }

  faces() {
    if(this.cachedFaces) return this.cachedFaces
    if(this.compound.isShapex) {
      this.cachedFaces = window.kernel.modeling.solidFaces(this.geom())
        .map(face => new Face(this, face))
      this.cachedFaces.forEach(face => face.id = face.kernelId)
      return this.cachedFaces
    }
    this.cachedFaces = this.collectShapes('face')
    this.cachedFaces.forEach((face, i) => face.id = this.id + '/face/' + i )
    return this.cachedFaces
  }

  edges() {
    if(this.cachedEdges) return this.cachedEdges
    if(this.compound.isShapex) {
      this.cachedEdges = window.kernel.modeling.solidEdges(this.geom())
        .map(edge => new Edge(this, edge))
      this.cachedEdges.forEach(edge => edge.id = edge.kernelId)
      return this.cachedEdges
    }
    this.cachedEdges = this.collectShapes('edge')
    this.cachedEdges.forEach((edge, i) => edge.id = this.id + '/edge/' + i )
    return this.cachedEdges
  }

  shapexFaceTessellation(face) {
    if(!this.cachedShapexFaceTessellations) {
      const faces = this.faces()
      // Shapex tessellates each exact face with its analytical surface
      // normals. Do not average those normals across topological edges: doing
      // so rounds genuine CAD creases, and history is not a geometric
      // continuity test. Smooth cylinders and NURBS faces are already smooth
      // within their own tessellation.
      this.cachedShapexFaceTessellations = new Map(faces.map(candidate => [
        candidate.kernelId,
        window.kernel.modeling.mesh(candidate.geom()),
      ]))
    }
    return this.cachedShapexFaceTessellations.get(face.kernelId)
  }

  cloneCached(newCompound) {
    const clone = new Solid(newCompound, this.geom())
    clone.id = this.id
    clone.cachedFaces = this.faces().map(face => new Face(clone, face.geom(), face.id, face.tessId) )
    clone.cachedEdges = this.edges().map(edge => new Edge(clone, edge.geom(), edge.id, edge.tessId) )
    return clone
  }

  toCompound() {
    if(this.compound.isShapex) {
      const raw = window.kernel.modeling.solidToCompound(this.geom())
      const compound = new Compound(this.compound.componentId, new ShapexGeometry(raw))
      compound.cachedSolids = [this]
      return compound
    }
    const compound = new Compound(this.compound.componentId, this.geom())
    compound.cachedSolids = [this]
    return compound
  }

  tesselate() {
    const tesselations = this.faces().map(face => face.tesselate() )
    return {
      positions: tesselations.map(tess => tess.positions ).flat(),
      normals: tesselations.map(tess => tess.normals ).flat(),
    }
  }

  reference() {
    return new SolidReference(this)
  }
}


export class Compound extends Volumetric {
  constructor(componentId, geom) {
    super()
    this.componentId = componentId
    if(geom instanceof ShapexGeometry) {
      this.isShapex = true
      this.kernelGeometry = geom
      this.geom = () => geom.raw
    } else if(geom) {
      // // Wrap solid in compound
      // if(geom.ShapeType() == window.oc.oc.TopAbs_ShapeEnum.TopAbs_SOLID) {
      //   const compound = new window.oc.oc.TopoDS_Compound()
      //   const builder = new window.oc.oc.BRep_Builder()
      //   builder.MakeCompound(compound)
      //   builder.Add(compound, geom)
      //   geom = compound
      // }
      this.geom = () => geom
    } else if(globalThis.window?.kernel?.modeling?.id == 'shapex') {
      this.isShapex = true
    }
  }

  solids() {
    if(!this.geom) return []
    if(this.cachedSolids) return this.cachedSolids
    if(this.isShapex) {
      this.cachedSolids = window.kernel.modeling.compoundSolids(this.geom())
        .map(solid => new Solid(this, solid))
      this.cachedSolids.forEach((solid, i) => solid.id = this.componentId + '/solid/' + i)
      return this.cachedSolids
    }
    this.cachedSolids = this.collectShapes('solid')
    this.cachedSolids.forEach((solid, i) => solid.id = this.componentId + '/solid/' + i )
    return this.cachedSolids
  }

  clone(geom) {
    if(this.isShapex) {
      const raw = geom instanceof ShapexGeometry
        ? geom.raw
        : geom || (this.geom && this.geom())
      if(!raw) return new Compound(this.componentId)
      return new Compound(this.componentId, new ShapexGeometry(raw))
    }
    return new Compound(this.componentId, geom || (this.geom && this.geom()))
  }

  cloneCached(geom, cachedSolids) {
    const clone = this.clone(geom)
    cachedSolids ||= this.solids()
    clone.cachedSolids = cachedSolids.map(solid => solid.cloneCached(clone) )
    return clone
  }

  cloneForComponent(componentId) {
    const clone = this.cloneCached()
    clone.componentId = componentId
    clone.cachedSolids.forEach((solid, index) => {
      solid.id = componentId + '/solid/' + index
    })
    return clone
  }

  nameShapexExtrusion(profile, featureId) {
    if(!this.isShapex) return this
    const faces = this.faces()
    faces.forEach(face => {
      const role = window.kernel.modeling.creationRole(this.geom(), face.geom())
      if(role?.kind == 'extrusion-start') {
        face.id = `/extrude-${featureId}/bottom`
      } else if(role?.kind == 'extrusion-end') {
        face.id = `/extrude-${featureId}/top`
      } else if(role?.kind == 'extrusion-side') {
        const ringIndex = role.boundary == 'outer' ? 0 : role.holeIndex + 1
        const segmentIndex = role.segment
        const segment = profile.rings[ringIndex]?.segments[segmentIndex]
        const suffix = role.part ? `/part/${role.part}` : ''
        const sourceId = segment ? profile.getBaseId(segment) : `${role.boundary}/${segmentIndex}`
        face.id = `/extrude-${featureId}/swept/${sourceId}${suffix}`
      }
    })
    this.nameShapexEdges()
    return this
  }

  inheritShapexTopology(parents, operation, featureId) {
    if(!this.isShapex) return this
    const outputFaces = this.faces()
    const assigned = new Set()
    parents.flatMap(parent => parent.faces()).forEach(oldFace => {
      const resolution = window.kernel.modeling.resolveFace(this.geom(), oldFace.geom())
      for(let index = 0; index < resolution.entityIds.length; index++) {
        const resolvedId = resolution.entityIds[index]
        const face = outputFaces.find(candidate => candidate.kernelId == resolvedId)
        if(!face || assigned.has(face)) continue
        face.id = resolution.kind == 'split' ? `${oldFace.id}/split/${index}` : oldFace.id
        assigned.add(face)
      }
    })
    outputFaces.filter(face => !assigned.has(face)).forEach((face, index) => {
      face.id = `/${featureId || 'kernel'}/${operation}/generated/${index}`
    })
    this.nameShapexEdges()
    return this
  }

  nameShapexEdges() {
    const namesUsed = {}
    this.edges().forEach(edge => {
      const faces = edge.connectedFaces()
      const base = `(${faces.map(face => face.id).join('|')})`
      namesUsed[base] = (namesUsed[base] || 0) + 1
      edge.id = `${base}/${namesUsed[base]}`
    })
  }

  faces() {
    return this.solids().flatMap(solid => solid.faces())
  }

  edges() {
    return this.solids().flatMap(solid => solid.edges())
  }

  track(algorithm, algoName, other, featureId) {
    const out = this.clone(algorithm.Shape ?
      algorithm.Shape() : algorithm.Apply(this.geom())
    )
    const history = algorithm.Modified ?
      algorithm
      :
      algorithm.History ?
        algorithm.History().get()
        :
        algorithm.History_1 ?
          algorithm.History_1().get()
          :
          algorithm.Context().get().History().get()

    const solids = out.solids()
    const oldSolids = this.solids().concat(other ? other.solids() : [])

    const newFaces = solids.flatMap(solid => solid.faces() )
    const newEdges = solids.flatMap(solid => solid.edges() )

    const oldFaces = oldSolids.flatMap(solid => solid.faces() )
    const oldEdges = oldSolids.flatMap(solid => solid.edges() )

    // Find unchanged faces in new solids & use original IDs
    const unchangedFaces = newFaces.map(face => {
      const original = oldFaces.find(f => f.geom().IsSame(face.geom()) ) //XXX IsSame -> surface & location same, IsEqual -> orientation also same
      if(!original) return
      face.id = original.id
      face.tessId = original.tessId
      face.cachedTesselation = original.cachedTesselation
      // return [face, original]
    })//.filter(Boolean)

    // // Unchanged faces are geometrically identical if all of their neighboors are unchanged too
    // unchangedFaces.filter(([face, original]) => {
    //   const neighboors = face.connectedFaces()
    //   return neighboors.every(neighboor => unchangedFaces.find(([f, _]) => f == neighboor ) )
    // }).forEach(([face, original]) => {
    //   // Inherit tessId if faces are visually identical
    //   face.tessId = original.tessId
    //   face.cachedTesselation = original.cachedTesselation
    // })

    // Find unchanged edges & inherit tesselation
    newEdges.forEach(edge => {
      const original = oldEdges.find(e => e.geom().IsSame(edge.geom()) )
      if(!original) return
      edge.tessId = original.tessId
      edge.cachedTesselation = original.cachedTesselation
    })

    // Find modified faces in new solids & use original IDs
    oldFaces.forEach(oldFace => {
      const modified = arrayFromOcList(history.Modified(oldFace.geom())).map(m => new window.oc.oc.TopoDS.Face_1(m) )
      const faces = modified.map(modFace => newFaces.find(f => f.geom().IsSame(modFace) ) )
      faces.forEach(f => f.id = oldFace.id )
    });

    // Find new faces generated from the original edges & faces and name them accordingly
    [...oldEdges, ...oldFaces].forEach(old => {
      const shapes = arrayFromOcList(history.Generated(old.geom()))
      const generated = shapes.map(shape => {
        try {
          return window.oc.oc.TopoDS.Face_1(shape)
        } catch(err) {}
      }).filter(Boolean)
      const faces = generated.map(genFace => newFaces.find(face => face.geom().IsSame(genFace) ) )
      faces.forEach((face, i) => face.id = old.id + '/' + algoName + '/' + i )
    })

    // Find faces that have been generated out of thin air
    newFaces
      .filter(face => !face.id )
      .forEach((face, i) => {
        face.id = '/' + featureId + '/' + algoName + '/' + i
      })

    // Name all edges in new compound according to their connected faces
    // Enumerate too, since circular faces may connect to the same neighboor twice
    const namesUsed = {}
    newEdges.forEach(edge => {
      const faces = edge.connectedFaces()
      edge.id = '(' + faces[0].id + '|' + faces[1].id + ')'
      namesUsed[edge.id] = (namesUsed[edge.id] || 0) + 1
      edge.id += '/' + namesUsed[edge.id]
    })

    return out
  }

  transform(plane) {
    const input = this.isShapex ? this.kernelGeometry : this.geom()
    const algo = window.kernel.modeling.transform(input, plane)
    if(algo instanceof ShapexGeometry) {
      return new Compound(this.componentId, algo).inheritShapexTopology([this], 'transform')
    }
    return this.track(algo, 'transform')
  }

  merge(others) {
    if(this.isShapex || others.some(other => other.isShapex)) {
      if(!this.isShapex || others.some(other => !other.isShapex)) {
        throw { type: 'error', msg: 'Cannot mix Shapex and OpenCascade bodies' }
      }
      const geometry = window.kernel.modeling.combine(
        this.kernelGeometry,
        others.map(other => other.kernelGeometry),
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this, ...others], 'combine')
    }
    const compound = new window.oc.oc.TopoDS_Compound()
    const builder = new window.oc.oc.BRep_Builder()
    builder.MakeCompound(compound);
    [this, ...others].forEach(c => builder.Add(compound, c.geom()) )

    const clone = this.clone(compound)
    const solids = [...this.solids(), ...others.flatMap(other => other.solids() )]
    clone.solids().forEach((cloneSolid, i) => {
      const originSolid = solids[i]
      const originFaces = originSolid.faces()
      const originEdges = originSolid.edges()
      cloneSolid.faces().forEach((cloneFace, j) => {
        cloneFace.id = originFaces[j].id
      })
      cloneSolid.edges().forEach((cloneEdge, j) => {
        cloneEdge.id = originEdges[j].id
      })
    })
    return clone
  }

  repair() {
    if(!this.geom) return this
    if(this.isShapex) return this

    const originallyValid = this.validate()
    let out
    try {
      out = this.unifyFaces().fixShape()
    } catch(_) {
      // Some valid periodic solids cannot be tracked through face unification.
      // Keep those solids as-is, but do not mask failures to repair invalid input.
      if(originallyValid) return this
      throw { type: 'error', msg: "Geometry could not be repaired" }
    }
    if(!out.validate()) {
      if(originallyValid) return this
      throw { type: 'error', msg: "Operation produced invalid geometry" }
    }
    return out
  }

  unifyFaces() {
    const unify = new window.oc.oc.ShapeUpgrade_UnifySameDomain_2(this.geom(), true, true, false)
    unify.Build()
    return this.track(unify, 'unify')
  }

  fixShape() {
    const fix = new window.oc.oc.ShapeFix_Shape_2(this.geom())
    fix.Perform(new window.oc.oc.Message_ProgressRange_1())
    return this.track(fix, 'fix')
  }

  orientFaces() {
    exploreShape(this.geom(), 'shell', shell => {
      window.oc.oc.BOPTools_AlgoTools.OrientFacesOnShell(shell)
    })
  }

  alignFaceOrientation() {
    const reshaper = new window.oc.oc.BRepTools_ReShape()
    reshaper.ModeConsiderOrientation().set(true)

    exploreShape(this.geom(), 'face', face => {
      const orientation = face.Orientation()
      if(orientation == window.oc.oc.TopAbs_Orientation.TopAbs_REVERSED) {
        const reversed = face.Complemented() // .Reversed()
        reshaper.Replace(face, reversed, true)
      }
    })

    this.track(reshaper, 'reorient')
  }

  boolean(other, op, featureId) {
    if(!other.geom) throw { type: 'error', msg: "Tool body has no volume" }

    if(op == 'create') {
      if(!this.geom) return this.cloneCached(other.geom(), other.solids())
      return this.merge([other])
    }

    if(!this.geom) {
      if(op == 'cut' || op == 'intersect') return this
      return this.cloneCached(other.geom(), other.solids())
    }

    if(this.isShapex || other.isShapex) {
      if(!this.isShapex || !other.isShapex) {
        throw { type: 'error', msg: 'Cannot mix Shapex and OpenCascade bodies' }
      }
      return new Compound(
        this.componentId,
        window.kernel.modeling.boolean(this.kernelGeometry, other.kernelGeometry, op),
      ).inheritShapexTopology([this, other], op, featureId)
    }
    const algo = window.kernel.modeling.boolean(this.geom(), other.geom(), op)
    return this.track(algo, op, other)
  }

  fillet(edges, radius, followTangentEdges=true) {
    if(this.isShapex) {
      const geometry = window.kernel.modeling.fillet(
        this.kernelGeometry,
        edges.map(edge => edge.geom()),
        radius,
        followTangentEdges,
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'fillet')
    }
    const fillet = new window.oc.oc.BRepFilletAPI_MakeFillet(this.geom(), window.oc.oc.ChFi3d_FilletShape.ChFi3d_Rational)
    edges.forEach(edge => {
      fillet.Add_2(radius, edge.geom())
    })
    try {
      fillet.Build(new window.oc.oc.Message_ProgressRange_1())
      if(!fillet.IsDone()) throw null
      return this.track(fillet, 'fillet')

    } catch(_) {
      throw { type: 'error', msg: "Fillet could not be built" }
    }
  }

  chamfer(edges, distance, followTangentEdges=true) {
    if(this.isShapex) {
      const geometry = window.kernel.modeling.chamfer(
        this.kernelGeometry, edges.map(edge => edge.geom()), distance, followTangentEdges,
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'chamfer')
    }
    const chamfer = new window.oc.oc.BRepFilletAPI_MakeChamfer(this.geom())
    edges.forEach(edge => {
      chamfer.Add_2(distance, edge.geom())
    })
    try {
      chamfer.Build(new window.oc.oc.Message_ProgressRange_1())
      if(!chamfer.IsDone()) throw null
      return this.track(chamfer, 'chamfer')

    } catch(_) {
      throw { type: 'error', msg: "Chamfer could not be built" }
    }
  }

  offset(openFaces, distance) {
    openFaces ||= []
    if(this.isShapex) {
      const geometry = window.kernel.modeling.shell(
        this.kernelGeometry,
        openFaces.map(face => face.geom()),
        distance,
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'shell')
    }
    // Offset each affected solid individually
    try {
      const thickened = this.solids().map(solid => {
        const solidFaces = openFaces.filter(f => solid.faces().includes(f) )
        const faces = ocListOfShapeFromArray(solidFaces.map(f => f.geom() ))
        const thicken = new window.oc.oc.BRepOffsetAPI_MakeThickSolid()
        thicken.MakeThickSolidByJoin(
          solid.geom(),
          faces,
          distance,
          1.0e-6, // window.oc.oc.Precision.Confusion()
          window.oc.oc.BRepOffset_Mode.BRepOffset_Skin,
          // window.oc.oc.BRepOffset_Mode.BRepOffset_Pipe,
          // window.oc.oc.BRepOffset_Mode.BRepOffset_RectoVerso,
          false,
          false,
          window.oc.oc.GeomAbs_JoinType.GeomAbs_Arc,
          false,
          new window.oc.oc.Message_ProgressRange_1()
        )
        if(!thicken.IsDone()) throw null
        return this.track(thicken, 'thicken')
      })
      // Merge resulting solids
      return thickened[0].merge(thickened.slice(1))

    } catch(_) {
      throw { type: 'error', msg: "Offset could not be built" }
    }
  }

  draft(faces, plane, angle) {
    if(this.isShapex) {
      const geometry = window.kernel.modeling.draft(
        this.kernelGeometry, faces.map(face => face.geom()), plane, angle,
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'draft')
    }
    try {
      const draft = new window.oc.oc.BRepOffsetAPI_DraftAngle_1()
      draft.Init(this.geom())
      const direction = ocDirFromVec(normalFromMatrix(plane))
      const neutralPlane = ocPlnFromMatrix(plane)

      faces.forEach(face => {
        draft.Add(face.geom(), direction, angle, neutralPlane, true)
        if(!draft.AddDone()) throw null
      })

      draft.Build(new window.oc.oc.Message_ProgressRange_1())
      if(!draft.IsDone()) throw null
      return this.track(draft, 'draft')
    } catch(_) {
      throw { type: 'error', msg: "Draft could not be built" }
    }
  }

  dissolveFaces(faces) {
    try {
      const remove = new window.oc.oc.BOPAlgo_RemoveFeatures()
      remove.SetShape(this.geom())
      remove.AddFacesToRemove(ocListOfShapeFromArray(faces.map(face => face.geom())))
      remove.Perform(new window.oc.oc.Message_ProgressRange_1())
      if(remove.HasErrors()) throw null
      return this.track(remove, 'remove-faces')
    } catch(_) {
      throw { type: 'error', msg: "Faces could not be deleted" }
    }
  }

  reshape(algoName, cb) {
    if(this.isShapex) window.kernel.modeling.unsupported(algoName)
    const reshaper = new window.oc.oc.ShapeBuild_ReShape()
    cb(reshaper)
    reshaper.Shape = () => reshaper.Apply(this.geom(), window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHAPE)
    return this.track(reshaper, algoName)
  }

  removeShapes(shapes) {
    if(this.isShapex) {
      if(shapes.some(shape => !(shape instanceof Solid))) {
        window.kernel.modeling.unsupported('removing non-solid shapes')
      }
      const geometry = window.kernel.modeling.removeSolids(
        this.kernelGeometry,
        shapes.map(shape => shape.geom()),
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'remove-solids')
    }
    return this.reshape('remove-shapes', reshaper => {
      shapes.forEach(shape => {
        // reshaper.Remove(solid instanceof Solid ? solid.geom() : solid)
        reshaper.Remove(shape.geom())
      })
    })
  }

  replaceShapes(shapes, replacements, algoName) {
    if(this.isShapex) {
      if(shapes.length != 1 || replacements.length != 1
        || !(shapes[0] instanceof Face) || !(replacements[0] instanceof Face)) {
        window.kernel.modeling.unsupported('this face replacement')
      }
      const geometry = window.kernel.modeling.replaceFace(
        this.kernelGeometry,
        shapes[0].geom(),
        replacements[0].geom(),
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], algoName || 'replace-face')
    }
    return this.reshape(algoName || 'replace-shapes', reshaper => {
      shapes.forEach((shape, i) => {
        reshaper.Replace(shape.geom(), replacements[i].geom())
      })
    })
  }

  moveFaces(faces, transform) {
    if(this.isShapex) {
      const geometry = window.kernel.modeling.moveFaces(
        this.kernelGeometry,
        faces.map(face => face.geom()),
        transform,
      )
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'move-faces')
    }
    return this.replaceShapes(
      faces,
      faces.map(face => transformGeometry(face.geom(), transform).Shape() ),
      'move-faces',
    )
  }

  split(plane, keep, featureId) {
    if(this.isShapex) {
      const geometry = window.kernel.modeling.split(this.kernelGeometry, plane, keep)
      return new Compound(this.componentId, geometry)
        .inheritShapexTopology([this], 'split', featureId)
    }
    const left = this.halfCut(plane, true, featureId)
    const right = this.halfCut(plane, false, featureId)
    if(keep == 'both') return left.merge([right])
    if(keep == 'left') return left
    return right
  }

  halfCut(plane, side, featureId) {
    if(!this.geom) return this
    if(this.isShapex) {
      try {
        const geometry = window.kernel.modeling.halfCut(this.kernelGeometry, plane, side)
        return new Compound(this.componentId, geometry)
          .inheritShapexTopology([this], side ? 'split-left' : 'split-right', featureId)
      } catch(error) {
        throw new Error(`Shapex section view failed: ${error?.message || error}`)
      }
    }
    let box = new window.oc.oc.BRepPrimAPI_MakeBox_2(1000, 1000, side ? 1000 : -1000).Shape() //XXX determine bounding box automatically
    box = transformGeometry(box, new THREE.Matrix4().makeTranslation(-500, -500, 0).premultiply(plane)).Shape()
    const algo = new window.oc.oc.BRepAlgoAPI_Cut_3(this.geom(), box, new window.oc.oc.Message_ProgressRange_1())
    return this.track(algo, 'split-' + side ? 'left' : 'right', null, featureId)
  }

  // split(plane, side, featureId) {
  //   if(!this.geom) return this
  //   const pln = ocPlnFromMatrix(plane)
  //   const face = new window.oc.oc.BRepBuilderAPI_MakeFace_9(pln, -1000, 1000, -1000, 1000).Face()
  //   // const algo = new window.oc.oc.BOPAlgo_MakerVolume_1()
  //   const algo = new window.oc.oc.BOPAlgo_Splitter_1()
  //   // algo.SetAvoidInternalShapes(true)
  //   algo.AddArgument(this.geom())
  //   // algo.AddArgument(face)
  //   algo.AddTool(face)
  //   algo.Perform(new window.oc.oc.Message_ProgressRange_1())
  //   return this.track(algo, 'split', null, featureId)
  // }

  // split(plane, side, featureId) {
  //   const pln = ocPlnFromMatrix(plane)
  //   const face = new window.oc.oc.BRepBuilderAPI_MakeFace_9(pln, -1000, 1000, -1000, 1000).Face()
  //   const algo = new window.oc.oc.BRepAlgoAPI_Splitter_1()
  //   algo.SetArguments(ocListOfShapeFromArray([this.geom()]))
  //   algo.SetTools(ocListOfShapeFromArray([face]))
  //   return this.track(algo, 'split', null, featureId)
  // }

  // split(plane, side) {
  //   if(!this.geom) return this
  //   const pln = ocPlnFromMatrix(plane)
  //   const section = new window.oc.oc.BRepAlgoAPI_Section_5(this.geom(), pln, false) //XXX Use BRepAlgoAPI_Section_6 to split by surface

  //   section.ComputePCurveOn1(true)
  //   section.Approximation(true)
  //   section.Build(new window.oc.oc.Message_ProgressRange_1())
  //   if(!section.IsDone()) throw 'Cannot split'

  //   const split = new window.oc.oc.BRepFeat_SplitShape_2(this.geom())
  //   exploreShape(section.Shape(), 'edge', edge => {
  //     exploreShape(this.geom(), 'face', face => {
  //       if(section.HasAncestorFaceOn1(edge, face)) {
  //         split.Add_3(edge, face)
  //       }
  //     })
  //   })
  //   split.Build(new window.oc.oc.Message_ProgressRange_1())

  //   const comp = new window.oc.oc.TopoDS_Compound()
  //   const builder = new window.oc.oc.BRep_Builder()
  //   builder.MakeCompound(comp)

  //   arrayFromOcList(split.DirectLeft()).forEach(face => {
  //     builder.Add(comp, new window.oc.oc.TopoDS.Face_1(face))
  //   })

  //   return this.clone(comp)
  // }

  tesselate() {
    if(!this.geom) return { positions: [], normals: [] }
    if(this.isShapex) return window.kernel.modeling.mesh(this.geom())
    const tesselations = this.solids().map(solid => solid.tesselate() )
    return {
      positions: tesselations.map(tess => tess.positions ).flat(),
      normals: tesselations.map(tess => tess.normals ).flat(),
    }
  }
}

function meshAreaAndCenter(mesh) {
  let area = 0
  const center = new THREE.Vector3()
  for(let index = 0; index < mesh.positions.length; index += 9) {
    const a = new THREE.Vector3(...mesh.positions.slice(index, index + 3))
    const b = new THREE.Vector3(...mesh.positions.slice(index + 3, index + 6))
    const c = new THREE.Vector3(...mesh.positions.slice(index + 6, index + 9))
    const triangleArea = b.clone().sub(a).cross(c.clone().sub(a)).length() * 0.5
    area += triangleArea
    center.add(a.add(b).add(c).multiplyScalar(triangleArea / 3.0))
  }
  if(area) center.divideScalar(area)
  return { area, center }
}


function tesselateCurve(geom) {
  const deflection = new window.oc.oc.GCPnts_TangentialDeflection_2(geom,
    getViewportAngularTessellationTolerance() * EDGE_ANGULAR_TESSELLATION_RATIO,
    getViewportTessellationTolerance() * EDGE_LINEAR_TESSELLATION_RATIO,
    2, // min points
    1.0e-9, 1.0e-7
  )
  return arrayRange(1, deflection.NbPoints()).map(i => arrayFromOcVec(deflection.Value(i)) )
}
