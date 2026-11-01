import * as THREE from 'three'

import Serialize from './serialize.js'
import { Reference, CurveReference } from './references.js'
import { SketchPoint, Line, Circle, Arc } from './geom2d.js'
import { Vertex, Edge, Face, Solid } from './geom3d.js'
import { ElemRef, SketchOrigin, ProjectedPoint } from './sketch.js'
import { shapex as sx, threeVector } from './shapex.js'

// Measures are picked geometry embedded in expressions as `{id}` tokens. Each
// derives a single number from its geometry, in millimeters, square or cubic
// millimeters, or degrees, measured in the space of the owning component.

const measureToken = /\{(\d+)\}/g

export function measureIds(expression) {
  return [...String(expression).matchAll(measureToken)].map(match => match[1])
}

// A handle of a sketch element, such as the end of a line or the center of a circle
export class HandleReference extends Reference {
  constructor(item, curveReference, index) {
    super(item)
    this.curveReference = curveReference
    this.componentId = curveReference.componentId
    this.index = index
  }

  static fromElemRef(ref) {
    const reference = new HandleReference(null, ref.curveRef.clone(), ref.index)
    reference.locate()
    return reference
  }

  locate() {
    const elem = this.curveReference.getItem()
    const point = elem?.handles()[this.index]
    this.item = point && new THREE.Matrix4().setPosition(point.clone().applyMatrix4(elem.sketch.workplane))
  }

  update(tree) {
    const error = this.curveReference.update(tree)
    this.locate()
    if(error) return error
    if(!this.item) return { type: 'error', msg: 'Sketch point reference was lost' }
  }

  referencedItems() { return this.curveReference.referencedItems() }

  clone() { return new HandleReference(this.item, this.curveReference.clone(), this.index) }

  dump() {
    return {
      curveReference: this.curveReference,
      index: this.index,
    }
  }

  static undump(dump) {
    return new HandleReference(null, dump.curveReference, dump.index)
  }
}
Serialize.register(HandleReference, 'HandleReference')


export function acceptsMeasure(item) {
  return item instanceof ElemRef ? item.index !== undefined :
    item instanceof Vertex || item instanceof Edge || item instanceof Face || item instanceof Solid ||
    (!!item?.sketch && !(item instanceof SketchOrigin))
}

export function measureReference(item) {
  if(item instanceof ElemRef) return HandleReference.fromElemRef(item)
  if(item instanceof Vertex) return item.vertexReference()
  if(item instanceof Edge) return item.edgeReference()
  if(item instanceof Face) return item.faceReference()
  if(item instanceof Solid) return item.reference()
  return new CurveReference(item)
}

// Available measures of an item as [{ key, title, group }], the first one is the default
export function measureOptions(item) {
  return Object.entries(measurements(item)).map(([key, { title, group }]) => ({ key, title, group }))
}

export function measureValue(item, key) {
  const measurement = measurements(item)[key]
  if(!measurement) return
  return measurement.value()
}

function position(point, prefix = '') {
  const group = prefix ? 'Center' : undefined
  return {
    [prefix + 'x']: { title: 'X', group, value: () => point().x },
    [prefix + 'y']: { title: 'Y', group, value: () => point().y },
    [prefix + 'z']: { title: 'Z', group, value: () => point().z },
  }
}

function round(radius) {
  return {
    radius: { title: 'Radius', value: radius },
    diameter: { title: 'Diameter', value: () => radius() * 2 },
  }
}

const degrees = THREE.MathUtils.radToDeg

function measurements(item) {
  if(item instanceof THREE.Matrix4) {
    return position(() => new THREE.Vector3().setFromMatrixPosition(item))
  }

  if(item instanceof Vertex) return position(() => item.point)

  if(item instanceof Edge) {
    const curve = item.geom().curve()
    const circular = curve instanceof sx.Circle3
    const span = () => Math.abs(item.geom().trimmedCurve().range().span())
    return {
      length: { title: 'Length', value: () => item.length() },
      ...(circular && {
        ...round(() => curve.radius()),
        ...(span() < 2 * Math.PI - 1e-6 && { angle: { title: 'Angle', value: () => degrees(span()) } }),
        ...position(() => threeVector(curve.center()), 'center-'),
      }),
    }
  }

  if(item instanceof Face) {
    const surface = item.geom().surface()
    const firstPoint = () => new THREE.Vector3().fromArray(item.tesselate().positions)
    return {
      area: { title: 'Area', value: () => item.area() },
      perimeter: {
        title: 'Perimeter',
        // Seams of closed surfaces only border the face itself
        value: () => item.boundaryEdges()
          .filter(edge => edge.connectedFaces().some(face => face.kernelId != item.kernelId))
          .reduce((sum, edge) => sum + edge.length(), 0),
      },
      ...(surface.kind == 'cylinder' && round(() => {
        const origin = threeVector(surface.axis.origin())
        const direction = threeVector(surface.axis.direction()).normalize()
        const offset = firstPoint().sub(origin)
        return offset.addScaledVector(direction, -offset.dot(direction)).length()
      })),
      ...(surface.kind == 'sphere' && round(() => firstPoint().distanceTo(threeVector(surface.center)))),
      ...position(() => item.center(), 'center-'),
    }
  }

  if(item instanceof Solid) return {
    volume: { title: 'Volume', value: () => item.volume() },
    area: { title: 'Surface Area', value: () => item.area() },
    ...position(() => item.center(), 'center-'),
  }

  // Sketch elements, in the space of their component
  const workplane = item.sketch.workplane
  const placed = point => () => point.clone().applyMatrix4(workplane)

  if(item instanceof SketchPoint || item instanceof ProjectedPoint) return position(placed(item.point))

  if(item instanceof Line) return {
    length: { title: 'Length', value: () => item.length() },
    angle: {
      title: 'Angle',
      value: () => {
        const direction = item.direction()
        return degrees(Math.atan2(direction.y, direction.x))
      },
    },
  }

  if(item instanceof Circle) return {
    ...round(() => item.radius),
    circumference: { title: 'Circumference', value: () => item.length() },
    area: { title: 'Area', value: () => item.area() },
    ...position(placed(item.center()), 'center-'),
  }

  if(item instanceof Arc) return {
    ...round(() => item.radius),
    length: { title: 'Arc Length', value: () => item.length() },
    angle: { title: 'Angle', value: () => degrees(Math.abs(item.sweep)) },
    ...position(placed(item.center()), 'center-'),
  }

  return {
    length: { title: 'Length', value: () => item.length() },
  }
}
