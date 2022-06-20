import * as THREE from 'three'

import {
  LineTool,
  SplineTool,
} from './tools.js'
import { vec2three } from './utils.js'

const snapDistance = 14 // px
const maxSnapReferences = 5

export default class Snapper {
  constructor(viewport, updateView) {
    this.viewport = viewport
    this.updateView = updateView
    this.guides = []
    this.lastSnaps = []
    this.snapAnchor = null
    this.planeTransform = new THREE.Matrix4()
  }

  reset() {
    this.guides = []
    this.snapAnchor = null
    this.lastSnaps = []
    this.updateView([], null)
  }

  snap(vec, coords) {
    this.guides = []
    this.catchSnapPoints(coords)
    vec = this.snapToGuides(vec) || vec
    this.updateView(this.guides, this.snapAnchor)
    return vec
  }

  getSnapPoints() {
    const sketchElements = this.viewport.activeComponent.real.get_sketch().get_sketch_elements()
    // Filter out sketch element actively being drawn
    const tool = this.viewport.activeTool
    if((tool.constructor === LineTool && tool.line) || (tool.constructor === SplineTool && tool.spline)) sketchElements.pop()
    return sketchElements.flatMap(elem => {
      let points = elem.get_snap_points().map(p => vec2three(p))
      // Filter out handle actively being dragged
      if(this.viewport.activeHandle && elem.id() == this.viewport.activeHandle.elem.id()) {
        const handlePoint = vec2three(this.viewport.activeHandle.elem.get_handles()[this.viewport.activeHandle.index])
        points = points.filter(p => !p.equals(handlePoint))
      }
      return points
    })
  }

  catchSnapPoints(coords) {
    const snapPoints = this.getSnapPoints()
    let closestDist = 99999
    let target
    snapPoints.forEach(p => {
      const dist = this.viewport.renderer.toScreen(p).distanceTo(coords)
      if(dist < snapDistance && dist < closestDist) {
        closestDist = dist
        target = p
      }
    })
    if(!target) return
    if(!(this.lastSnaps[0] && this.lastSnaps[0].equals(target))) {
      this.lastSnaps.unshift(target)
      if(this.lastSnaps.length >= maxSnapReferences) this.lastSnaps.pop()
    }
  }

  snapToGuides(vec) {
    if(!vec) return
    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)
    const screenVec = this.viewport.renderer.toScreen(vec)
    let snapX = this.lastSnaps.find(snap => {
      // Compare plane space X axis..
      const testSnap = snap.clone().applyMatrix4(localTransform)
      testSnap.setY(localVec.y)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      // .. in screen space
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    let snapY = this.lastSnaps.find(snap => {
      const testSnap = snap.clone().applyMatrix4(localTransform)
      testSnap.setX(localVec.x)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    const snapVec = new THREE.Vector3(
      snapX ? snapX.clone().applyMatrix4(localTransform).x : localVec.x,
      snapY ? snapY.clone().applyMatrix4(localTransform).y : localVec.y,
      localVec.z
    ) //XXX z-imprecision
    snapVec.applyMatrix4(this.planeTransform)
    const screenSnapVec = this.viewport.renderer.toScreen(snapVec)
    if(snapX) {
      const start = this.viewport.renderer.toScreen(snapX)
      this.guides.push({
        id: 'v' + start.x + start.y,
        start,
        end: screenSnapVec,
      })
    }
    if(snapY) {
      const start = this.viewport.renderer.toScreen(snapY)
      this.guides.push({
        id: 'h' + start.x + start.y,
        start,
        end: screenSnapVec,
      })
    }
    if(snapX && snapY) {
      this.snapAnchor = this.snapAnchor || {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(snapVec),
        vec: snapVec,
        id: '' + snapVec.x + snapVec.y + snapVec.z,
      }
      if(snapX === snapY) return snapX
    } else {
      this.snapAnchor = null
    }
    if(snapX || snapY) return snapVec
  }
}
