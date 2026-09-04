import * as THREE from 'three'
import earcut from 'earcut'

import { ProfileReference, EdgeReference, FaceReference, SolidReference, PlanarReference, AxialReference, PointReference } from './references.js'
import { makeID } from './id.js'
import { Line } from './geom2d.js'
import { ShapexGeometry } from '../kernels/shapex.js'
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
      const curve = window.oc.oc.GeomAPI.To3d(geom, plane)
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

  extrude(componentId, height, featureId) {
    if(!height) throw { type: 'error', msg: 'Extrusion has no volume' }
    const geometry = window.kernel.modeling.extrude(this, height)
    if(geometry instanceof ShapexGeometry) {
      const compound = new Compound(componentId, geometry)
      compound.nameShapexExtrusion(this, featureId)
      return compound
    }
    return this.makeCompound(componentId, geometry, 'extrude-' + featureId)
  }

  revolve(componentId, axis, angle, featureId) {
    if(!angle) throw { type: 'error', msg: 'Revolution has no volume' }
    if(window.kernel.modeling.id == 'shapex') {
      const geometry = window.kernel.modeling.revolve(this, axis, angle)
      return new Compound(componentId, geometry).inheritShapexTopology([], 'revolve', featureId)
    }
    const face = this.makeFace()
    const ax = ocAx1FromMatrix(axis)
    let revolution = new window.oc.oc.BRepPrimAPI_MakeRevol_1(face, ax, angle, true) //XXX BRepFeat_MakeRevol

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

  sweep(componentId, rail, bounds) {
    try {
      if(this.rings.length != 1) {
        throw { type: 'error', msg: 'Sweeps with holes are not supported' }
      }
      if(bounds[0] != 0.0 || bounds[1] != 1.0) {
        throw { type: 'error', msg: 'Partial rail sweeps are not supported' }
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
    if(this.solid?.compound?.isShapex || this.compound?.isShapex) {
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
      0.4, // linear deflection
      false,
      0.2, // angular deflection
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

  chamfer(edges, distance) {
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
    0.1, // curvature deflection
    0.1, // angular deflection
    2, // min points
    1.0e-9, 1.0e-7
  )
  return arrayRange(1, deflection.NbPoints()).map(i => arrayFromOcVec(deflection.Value(i)) )
}
