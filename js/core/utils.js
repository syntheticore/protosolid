import * as THREE from 'three'


export function arrayFromOcVec(ocVec) {
  return [ocVec.X(), ocVec.Y(), (ocVec.Z && ocVec.Z()) || 0.0]
}

export function vecFromOc(ocVec) {
  return new THREE.Vector3().fromArray(arrayFromOcVec(ocVec))
}

export function arrayFromOcList(list) {
  const out = []
  while(list.Size()) {
    out.push(list.First_1())
    list.RemoveFirst()
  }
  return out
}

export function ocListOfShapeFromArray(shapes) {
  const list = new window.oc.oc.TopTools_ListOfShape_1()
  shapes.forEach(shape => list.Append_1(shape) )
  return list
}

export function ocPnt2dFromVec(vec) {
  return new window.oc.oc.gp_Pnt2d_3(vec.x, vec.y)
}

export function ocPntFromVec(vec) {
  return new window.oc.oc.gp_Pnt_3(vec.x, vec.y, vec.z)
}

export function ocDirFromVec(vec) {
  return new window.oc.oc.gp_Dir_4(vec.x, vec.y, vec.z)
}

export function ocDir2dFromVec(vec) {
  return new window.oc.oc.gp_Dir2d_4(vec.x, vec.y)
}

export function ocVecFromVec(vec) {
  return new window.oc.oc.gp_Vec_4(vec.x, vec.y, vec.z)
}

export function ocCirc2dFromVec(center, radius) {
  center = ocPnt2dFromVec(center)
  const v = new window.oc.oc.gp_Dir2d_4(1.0, 0.0)
  const axis = new window.oc.oc.gp_Ax2d_2(center, v)
  return new window.oc.oc.gp_Circ2d_2(axis, radius, false)
}

export function ocAx1FromMatrix(m) {
  const pos = new THREE.Vector3().setFromMatrixPosition(m)
  const axDir = normalFromMatrix(m)
  return new window.oc.oc.gp_Ax1_2(ocPntFromVec(pos), ocDirFromVec(axDir))
}

export function ocAx3FromMatrix(m) {
  const pos = new THREE.Vector3().setFromMatrixPosition(m)
  const rot = new THREE.Quaternion().setFromRotationMatrix(m)
  const axDir = new THREE.Vector3(0,0,1).applyQuaternion(rot)
  const xDir = new THREE.Vector3(1,0,0).applyQuaternion(rot)
  return new window.oc.oc.gp_Ax3_3(ocPntFromVec(pos), ocDirFromVec(axDir), ocDirFromVec(xDir))
}

export function matrixFromOcPln(pln) {
  const x = vecFromOc(pln.XAxis().Direction())
  const y = vecFromOc(pln.YAxis().Direction())
  const z = vecFromOc(pln.Axis().Direction())
  return new THREE.Matrix4()
    .makeBasis(x, y, z)
    .setPosition(vecFromOc(pln.Axis().Location()))
}

export function ocPlnFromMatrix(m) {
  return new window.oc.oc.gp_Pln_2(ocAx3FromMatrix(m))
}

export function transformGeometry(geom, plane, inPlace) {
  const pos = new THREE.Vector3().setFromMatrixPosition(plane)
  const rot = new THREE.Quaternion().setFromRotationMatrix(plane)

  // gce_MakeTranslation
  const trans = new window.oc.oc.gp_Trsf_1()
  const quat = new window.oc.oc.gp_Quaternion_2(rot.x, rot.y, rot.z, rot.w)
  trans.SetRotationPart(quat)
  trans.SetTranslationPart(ocVecFromVec(pos))

  return new window.oc.oc.BRepBuilderAPI_Transform_2(geom, trans, !inPlace)
}

function shapeEnum(type) {
  return {
    vertex: window.oc.oc.TopAbs_ShapeEnum.TopAbs_VERTEX,
    edge: window.oc.oc.TopAbs_ShapeEnum.TopAbs_EDGE,
    face: window.oc.oc.TopAbs_ShapeEnum.TopAbs_FACE,
    solid: window.oc.oc.TopAbs_ShapeEnum.TopAbs_SOLID,
    shell: window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHELL,
  }[type]
}

function wrapShape(shape, type) {
  const converters = {
    vertex: window.oc.oc.TopoDS.Vertex_1,
    edge: window.oc.oc.TopoDS.Edge_1,
    face: window.oc.oc.TopoDS.Face_1,
    solid: window.oc.oc.TopoDS.Solid_1,
    shell: window.oc.oc.TopoDS.Shell_1,
  }
  return new converters[type](shape)
}

export function exploreShape(shape, type, cb) {
  const items = new window.oc.oc.TopExp_Explorer_2(shape, shapeEnum(type), window.oc.oc.TopAbs_ShapeEnum.TopAbs_SHAPE)
  while(items.More()) {
    cb(wrapShape(items.Current(), type))
    items.Next()
  }
}

export function collectShapes(geom, type) {
  const map = new window.oc.oc.TopTools_IndexedMapOfShape_1()
  window.oc.oc.TopExp.MapShapes_1(geom, shapeEnum(type), map)

  return arrayRange(1, map.Extent())
    .map(i => map.FindKey(i) )
    .map(shape => wrapShape(shape, type) )
}

export function ocCatch(cb) {
  try {
    return cb()
  } catch(e) {
    if(typeof e === "number") {
      const exceptionData = window.oc.oc.OCJS.getStandard_FailureData(e)
      console.error(exceptionData.GetMessageString())
    }
    throw e
  }
}

// export function cloneShape(shape) {
//   return new window.oc.oc.BRepBuilderAPI_Copy_2(shape, true, false).Shape() // copyGeom, copyMesh
// }

export function normalFromMatrix(m) {
  const rot = new THREE.Quaternion().setFromRotationMatrix(m)
  return new THREE.Vector3(0,0,1).applyQuaternion(rot)
}

export function rotationFromNormal(normal) {
  let up = THREE.Object3D.DEFAULT_UP
  let xAxis
  if(Math.abs(normal.dot(up)) > 0.9999) {
    xAxis = new THREE.Vector3(1, 0, 0)
  } else {
    xAxis = new THREE.Vector3().crossVectors(up, normal).normalize()
  }
  const yAxis = new THREE.Vector3().crossVectors(normal, xAxis)
  const rot = new THREE.Matrix4().makeBasis(xAxis, yAxis, normal)
  return rot
  // return new THREE.Quaternion().setFromRotationMatrix(rot)
  // let radians = Math.acos(normal.dot(up))
  // return new THREE.Quaternion().setFromAxisAngle(xAxis, radians)
}

export function cross2d(a, b) {
  return a.x * b.y - a.y * b.x
}

// Check if two line segments turn clockwise
// Returns values < 0 when clockwise, > 0 when anti-clockwise and 0 when segments are colinear
export function clockwise(p1, p2, p3) {
  let v1 = p2.clone().sub(p1).normalize()
  let v2 = p3.clone().sub(p2).normalize()
  let v3 = p3.clone().sub(p1).normalize()
  // Cross product changes sign with clockwiseness,
  // but doesn't show if angle is steeper or shallower than 90 degrees
  // (symmetric between front and back)
  let cross = cross2d(v1, v3)
  // Dot product is "left/right" symmetric,
  // but negative for steep angles and positive for shallow angles
  let dot = Math.abs(v1.dot(v2) - 1.0) / 2.0 // Range shallow to steep => 0 -> 1
  return dot * Math.sign(cross)
}

export function isClockwise(closedPolyline) {
  return signedPolygonArea(closedPolyline) < 0.0
}

export function polygon_area(closedPolyline) {
  return signedPolygonArea(closedPolyline).abs()
}

export function signedPolygonArea(closedPolyline) {
  let signedArea = 0.0
  let len = closedPolyline.length
  arrayRange(0, len - 1).forEach(i => {
    let j = (i + 1) % len
    let p = closedPolyline[i]
    let nextP = closedPolyline[j]
    signedArea += p.x * nextP.y - nextP.x * p.y
  })
  return signedArea / 2.0
}

export const EPSILON = 0.000001

Number.prototype.almost = function(other) {
  return Math.abs(this - other) < EPSILON
}

THREE.Vector3.prototype.almost = function(other) {
  return Math.abs(this.x - other.x) < EPSILON &&
         Math.abs(this.y - other.y) < EPSILON &&
         Math.abs(this.z - other.z) < EPSILON
}

export function arrayRange(start, end, step=1) {
  return Array.from(
    { length: (end - start) / step + 1 },
    (_, i) => start + i * step
  )
}

export function makeColor(allColors) {
  const existingColors = allColors.map(color => parseHsl(color) )
  const testColors = [...Array(100)].map(() => {
    const color = {
      h: Math.random() * 360,
      s: 45 + Math.random() * 20,
      l: 55 + Math.random() * 10,
    }
    const diffs = existingColors.map(c => colorDiff(c, color) )
    const worstDiff = Math.min(...diffs)
    return { color, diff: worstDiff }
  })
  testColors.sort((a, b) => Math.sign(b.diff - a.diff) )
  const color = testColors[0].color
  return `hsl(${color.h}, ${color.s}%, ${color.l}%)`
}

export function colorDiff(c1, c2) {
  let hue = Math.abs(c1.h - c2.h)
  hue = hue > 180 ? 360 - hue : hue
  return hue + Math.abs(c1.s - c2.s) + Math.abs(c1.l - c2.l)
}

export function parseHsl(str) {
  console.log(str)
  const match = /hsl\((.+),\s*(.+)%,\s*(.+)%\)/g.exec(str).slice(1,4).map(v => Number(v) )
  return { h: match[0], s: match[1], l: match[2] }
}

export function rad(degrees) {
  return degrees * Math.PI / 180.0
}

export function deg(radians) {
  return radians / Math.PI * 180.0
}

Array.prototype.minMaxBy = function(comparator, lambda) {
  var lambdaFn;
  if (typeof(lambda) === "function") {
      lambdaFn = lambda;
  } else {
      lambdaFn = function(x){
          return x[lambda];
      }
  }
  var mapped = this.map(lambdaFn);
  var minValue = comparator.apply(Math, mapped);
  return this[mapped.indexOf(minValue)];
}
