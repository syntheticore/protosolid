import * as THREE from 'three'
import earcut from 'earcut'

import { ProfileReference, EdgeReference, FaceReference, PlanarReference, AxialReference } from './references.js'
import { makeID } from './id.js'
import { Line } from './geom2d.js'
import {
  arrayRange,
  rotationFromNormal,
  isClockwise,
  arrayFromOcList,
  arrayFromOcVec,
  ocVecFromVec,
  ocAx1FromMatrix,
  matrixFromOcPln,
  normalFromMatrix,
  vecFromOc,
} from './utils.js'


export class Wire {
  constructor(region) {
    region = region.map(seg => seg.clone() )
    if(region.length == 0) throw "Wires may not be empty"
    else if(region.length >= 2) {
      // Find starting point from element order
      let bounds = region[0].endpoints()
      let next_bounds = region[1].endpoints()
      let point = (bounds[0].almost(next_bounds[0]) || bounds[0].almost(next_bounds[1])) ? bounds[1] : bounds[0]

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
    return other.segments.every(elem => this.containsPoint(elem.endpoints()[0]) )
  }

  tesselate() {
    let polyline = this.segments.flatMap(curve => {
      let poly = curve.tesselate()
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
    const shape = transformGeometry(this.geometry(), workplane)
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
    const flatVertices = polyRings[0].map(p => [p[0], p[1]] ).flat()
    var vertices = earcut(flatVertices, [], 2).map(v => polyRings[0][v] )
    return {
      positions: vertices.flat(),
      normals: vertices.flatMap(v => [0.0, 0.0, 1.0] ),
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

  extrude(componentId, height, featureId) {
    if(!height) throw { type: 'error', msg: 'Extrusion has no volume' }
    const face = this.makeFace()
    const rot = new THREE.Quaternion().setFromRotationMatrix(this.sketch.workplane)
    const dir = ocVecFromVec(new THREE.Vector3(0,0,height).applyQuaternion(rot))
    let prism = new window.oc.oc.BRepPrimAPI_MakePrism_1(face, dir, true, true) //XXX use BRepFeat_MakePrism to allow extrusion up to limit face
    // const args = new window.oc.oc.TopTools_ListOfShape_1()
    // args.Append_1(face)
    // const history = new window.oc.oc.BRepTools_History_2(args, prism)
    return this.makeCompound(componentId, prism.Shape(), 'extrude-' + featureId)
  }

  revolve(componentId, axis, angle, featureId) {
    if(!angle) throw { type: 'error', msg: 'Revolution has no volume' }
    const face = this.makeFace()
    const ax = ocAx1FromMatrix(axis)
    let revolution = new window.oc.oc.BRepPrimAPI_MakeRevol_1(face, ax, angle, true)
    return this.makeCompound(componentId, revolution.Shape(), 'revolve-' + featureId)
  }

  makeFace() {
    const wire = this.rings[0].transformed(this.sketch.workplane)
    return new window.oc.oc.BRepBuilderAPI_MakeFace_15(wire, true).Face()
  }

  makeCompound(componentId, shape, featureId) {
    const compound = new Compound(componentId, window.oc.oc.TopoDS.Solid_1(shape))

    const originals = [...this.rings[0].segments]
    const solid = compound.solids()[0]

    // Name faces
    const faces = solid.faces()
    originals.forEach((seg, i) => {
      faces[i].id = '/' + featureId + '/swept/' + this.getBaseId(seg)
    })
    faces[faces.length - 2].id = '/' + featureId + '/bottom'
    faces[faces.length - 1].id = '/' + featureId + '/top'

    // Name all edges in new solid according to their connected faces
    solid.edges().forEach(edge => {
      const faces = edge.connectedFaces()
      edge.id = '(' + faces[0].id + '|' + faces[1].id + ')'
    })

    return compound
  }

  getBaseId(curve) {
    return curve.id.split('/')[0]
  }

  update() {
    const cutElements = this.sketch.elements.flatMap(elem => elem.split(this.sketch.elements) )
    const newWires = this.sketch.getWires(cutElements, false)
    let wasRepairNeeded = false
    let error
    this.rings = this.rings.map(wire => {
      const wireIds = new Set(wire.segments.map(seg => this.getBaseId(seg) ))
      const replacementWire = newWires.map(newWire => {
        const newWireIds = new Set(newWire.segments.map(tcurve => this.getBaseId(tcurve) ))
        const matched = wireIds.intersection(newWireIds).size
        if(matched > 0) return [matched, newWireIds.size, newWire]
      }).filter(Boolean).minMaxBy(Math.max, pair => pair[0] )
      if(!replacementWire) {
        error = { type: 'error', msg: "Profile was lost" }
        return wire
      }
      const [matched, newSize, newWire] = replacementWire
      if(matched != wireIds.size || matched != newSize) wasRepairNeeded = true
      return newWire
    })
    if(error) return error
    if(wasRepairNeeded) return { type: 'warning', msg: "Profile has been repaired" }
  }

  clone() {
    const clone = new Profile(this.sketch, [...this.rings])
    clone.id = this.id
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
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return gprops.Mass() // Mass really corresponds to surface area here
  }

  center() {
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.SurfaceProperties_1(this.geom(), gprops, false, false)
    return vecFromOc(gprops.CentreOfMass())
  }

  // transform(workplane) {
  //   //XXX Track transformation
  //   const transformed = transformGeometry(this.geom(), workplane)
  //   this.geom = () => transformed
  // }

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
    const analyzer = new window.oc.oc.BRepCheck_Analyzer(this.geom(), true, false)
    return analyzer.IsValid_2()
  }

  volume() {
    if(!this.geom) return 0
    const gprops = new window.oc.oc.GProp_GProps_1() //XXX use GProp_GProps_2 to supply point close to center for better accuracy
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return gprops.Mass()
  }

  center() {
    if(!this.geom) return
    const gprops = new window.oc.oc.GProp_GProps_1()
    window.oc.oc.BRepGProp.VolumeProperties_1(this.geom(), gprops, false, false, false)
    return gprops.CentreOfMass()
  }
}


export class Face extends Shape {
  constructor(solid, geom, id) {
    super()
    this.solid = solid
    this.geom = () => geom
    this.id = id
  }

  planarReference() {
    if(!this.getPlane()) throw "Cannot provide plane from non-planar face"
    return new PlanarReference(new FaceReference(this))
  }

  faceReference() {
    return new FaceReference(this)
  }

  getPlane() {
    const surface = window.oc.oc.BRep_Tool.Surface_2(this.geom())
    const isPlanar = new window.oc.oc.GeomLib_IsPlanarSurface(surface, 1.0e-7) //XXX Surface->IsKind(STANDARD_TYPE(Geom_Plane)) could be faster
    if(!isPlanar.IsPlanar()) return
    const plane = isPlanar.Plan()
    return matrixFromOcPln(plane)
  }

  normal(u, v, useCurvature) { //XXX use BOPTools_AlgoTools3D::GetNormalToSurface or BOPTools_AlgoTools3D::GetNormalToFaceOnEdge
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

  tesselate() {
    if(this.cachedTesselation) return this.cachedTesselation
    const location = new window.oc.oc.TopLoc_Location_1()
    const triangulation = window.oc.oc.BRep_Tool.Triangulation(this.geom(), location, 0).get()
    triangulation.ComputeNormals()
    let positions = []
    let normals = []
    arrayRange(1, triangulation.NbTriangles()).forEach(i => {
      const triangle = triangulation.Triangle(i)
      const pos = arrayRange(1, 3).flatMap(j => arrayFromOcVec(triangulation.Node(triangle.Value(j))) )
      const norm = arrayRange(1, 3).flatMap(j => arrayFromOcVec(triangulation.Normal_1(triangle.Value(j))) )
      positions = positions.concat(pos)
      normals = normals.concat(norm)
    })
    this.cachedTesselation = {
      positions,
      normals,
    }
    return this.cachedTesselation
  }
}


export class Edge extends Shape {
  constructor(solid, geom, id) {
    super()
    this.solid = solid
    this.geom = () => geom
    this.id = id
  }

  axialReference() {
    if(!this.getAxis()) throw "Cannot provide axis from non-linear edge"
    return new AxialReference(this.edgeReference())
  }

  edgeReference() {
    return new EdgeReference(this)
  }

  getAxis() {
    // BRep_Tool.Curve()
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    if(curve.GetType() != window.oc.oc.GeomAbs_CurveType.GeomAbs_Line) return
    const line = curve.Line()
    const dir = vecFromOc(line.Direction()).normalize()
    const loc = vecFromOc(line.Location())
    return rotationFromNormal(dir).setPosition(loc)
  }

  connectedFaces() {
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
    const curve = new window.oc.oc.BRepAdaptor_Curve_2(this.geom())
    const middle = (curve.FirstParameter() + curve.LastParameter()) / 2.0
    return vecFromOc(curve.Value(middle))
  }

  vertices() {
    return collectShapes(this.geom(), 'vertex').map(vertex => vecFromOc(window.oc.oc.BRep_Tool.Pnt(vertex)) )
  }

  tesselate() {
    if(this.cachedTesselation) return this.cachedTesselation
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
    this.cachedFaces = this.collectShapes('face')
    this.cachedFaces.forEach((face, i) => face.id = this.id + '/face/' + i )
    return this.cachedFaces
  }

  edges() {
    if(this.cachedEdges) return this.cachedEdges
    this.cachedEdges = this.collectShapes('edge')
    this.cachedEdges.forEach((edge, i) => edge.id = this.id + '/edge/' + i )
    return this.cachedEdges
  }

  cloneCached(newCompound) {
    const clone = new Solid(newCompound, this.geom())
    clone.id = this.id
    clone.cachedFaces = this.faces().map(face => new Face(clone, face.geom(), face.id) )
    clone.cachedEdges = this.edges().map(edge => new Edge(clone, edge.geom(), edge.id) )
    return clone
  }

  tesselate() {
    const tesselations = this.faces().map(face => face.tesselate() )
    return {
      positions: tesselations.map(tess => tess.positions ).flat(),
      normals: tesselations.map(tess => tess.normals ).flat(),
    }
  }
}


export class Compound extends Volumetric {
  constructor(componentId, geom) {
    super()
    this.componentId = componentId
    if(geom) this.geom = () => geom
  }

  solids() {
    if(!this.geom) return []
    if(this.cachedSolids) return this.cachedSolids
    this.cachedSolids = this.collectShapes('solid')
    this.cachedSolids.forEach((solid, i) => solid.id = this.componentId + '/solid/' + i )
    return this.cachedSolids
  }

  clone(geom) {
    return new Compound(this.componentId, geom || (this.geom && this.geom()))
  }

  cloneCached(geom, cachedSolids) {
    const clone = this.clone(geom)
    cachedSolids ||= this.solids()
    clone.cachedSolids = cachedSolids.map(solid => solid.cloneCached(clone) )
    return clone
  }

  track(algorithm, algoName, other) {
    const out = this.clone(algorithm.Shape())

    const history = algorithm.Modified ? algorithm : (algorithm.History_1 ? algorithm.History_1().get() : algorithm.Context().get().History().get())

    const solids = out.solids()
    const oldSolids = this.solids().concat(other ? other.solids() : [])

    const newFaces = solids.flatMap(solid => solid.faces() )
    const newEdges = solids.flatMap(solid => solid.edges() )

    const oldFaces = oldSolids.flatMap(solid => solid.faces() )
    const oldEdges = oldSolids.flatMap(solid => solid.edges() )

    // Find unchanged faces in new solids & use original IDs
    newFaces.forEach(face => {
      const original = oldFaces.find(f => f.geom().IsSame(face.geom()) )
      // if(original) console.log('original', original)
      if(original) face.id = original.id
    })

    // Find modified faces in new solids & use original IDs
    oldFaces.forEach(oldFace => {
      const modified = arrayFromOcList(history.Modified(oldFace.geom())).map(m => new window.oc.oc.TopoDS.Face_1(m) )
      const faces = modified.map(modFace => newFaces.find(f => f.geom().IsSame(modFace) ) )
      // if(faces.length) console.log('modified', faces)
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
      // if(faces.length) console.log('generated', faces)
      faces.forEach((face, i) => face.id = old.id + '/' + algoName + '/' + i )
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

  repair() {
    if(!this.geom) return this
    const out = this.unifyFaces().fixShape()
    if(!out.validate()) throw { type: 'error', msg: "Operation produced invalid geometry" }
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

  boolean(other, op) {
    if(!other.geom) throw { type: 'error', msg: "Tool body has no volume" }
    if(!this.geom) return this.cloneCached(other.geom(), other.solids())

    const ops = {
      join: window.oc.oc.BRepAlgoAPI_Fuse_3,
      cut: window.oc.oc.BRepAlgoAPI_Cut_3,
    }
    const algo = new ops[op](this.geom(), other.geom(), new window.oc.oc.Message_ProgressRange_1())
    return this.track(algo, op, other)
  }

  fillet(edges, radius) {
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

  offset(openFaces, distance) {
    // Build face list
    const faces = new window.oc.oc.TopTools_ListOfShape_1()
    openFaces.forEach(face => faces.Append_1(face.geom()) )
    // Offset each affected solid individually
    try {
      const thickened = this.solids().map(solid => {
        const thicken = new window.oc.oc.BRepOffsetAPI_MakeThickSolid()
        thicken.MakeThickSolidByJoin(
          solid.geom(),
          faces,
          distance,
          1.0e-6, // window.oc.oc.Precision.Confusion()
          window.oc.oc.BRepOffset_Mode.BRepOffset_Skin,
          false,
          false,
          window.oc.oc.GeomAbs_JoinType.GeomAbs_Arc,
          false,
          new window.oc.oc.Message_ProgressRange_1()
        )
        if(!thicken.IsDone()) throw null
        return this.track(thicken, 'thicken')
      })
      // Merge resulting solids, in case overlaps occured
      return thickened.reduce((acc, next) => acc ? acc.boolean(next, 'join') : next )

    } catch(_) {
      throw { type: 'error', msg: "Offset could not be built" }
    }
  }

  tesselate() {
    const tesselations = this.solids().map(solid => solid.tesselate() )
    return {
      positions: tesselations.map(tess => tess.positions ).flat(),
      normals: tesselations.map(tess => tess.normals ).flat(),
    }
  }
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

function transformGeometry(geom, workplane) {
  const pos = new THREE.Vector3().setFromMatrixPosition(workplane)
  const rot = new THREE.Quaternion().setFromRotationMatrix(workplane)

  const trans = new window.oc.oc.gp_Trsf_1()
  const quat = new window.oc.oc.gp_Quaternion_2(rot.x, rot.y, rot.z, rot.w)
  trans.SetRotationPart(quat)
  trans.SetTranslationPart(ocVecFromVec(pos))

  return new window.oc.oc.BRepBuilderAPI_Transform_2(geom, trans, true).Shape()
}

function collectShapes(geom, type) {
  const enums = {
    vertex: window.oc.oc.TopAbs_ShapeEnum.TopAbs_VERTEX,
    edge: window.oc.oc.TopAbs_ShapeEnum.TopAbs_EDGE,
    face: window.oc.oc.TopAbs_ShapeEnum.TopAbs_FACE,
    solid: window.oc.oc.TopAbs_ShapeEnum.TopAbs_SOLID,
  }
  const converters = {
    vertex: window.oc.oc.TopoDS.Vertex_1,
    edge: window.oc.oc.TopoDS.Edge_1,
    face: window.oc.oc.TopoDS.Face_1,
    solid: window.oc.oc.TopoDS.Solid_1,
  }

  const map = new window.oc.oc.TopTools_IndexedMapOfShape_1()
  window.oc.oc.TopExp.MapShapes_1(geom, enums[type], map)

  return arrayRange(1, map.Extent())
    .map(i => map.FindKey(i) )
    .map(shape => new converters[type](shape) )
}
