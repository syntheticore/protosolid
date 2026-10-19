import * as THREE from 'three'


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
