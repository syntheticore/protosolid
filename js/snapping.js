import * as THREE from 'three'

// import {
//   LineTool,
//   SplineTool,
// } from './tools.js'

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

  snap(vec, coords, snapToGuides, snapToPoints, localSpace) {
    this.guides = []
    if(localSpace) {
      if((snapToGuides || snapToPoints) && this.viewport.activeSketch) this.catchSnapPoints(coords)
      vec = this.snapToGuides(vec, snapToGuides, snapToPoints)
      if(vec) vec.z = 0.0 //XXX project vec to plane before snapping
    }
    this.updateView(this.guides, snapToPoints ? this.snapAnchor : null)
    return vec
  }

  getSnapPoints() {
    let sketchElements = [...this.viewport.activeSketch.elements]
    // Filter out sketch element actively being drawn
    const tool = this.viewport.activeTool
    if(tool.curve) sketchElements.pop()
    return sketchElements.flatMap(elem => {
      let points = elem.snapPoints()
      // Filter out handle actively being dragged & connected neighboors
      if(this.viewport.activeHandle) {
        const handlePoint = this.viewport.activeHandle.elem.handles()[this.viewport.activeHandle.index]
        const constrainedPoints = this.viewport.activeHandle.elem.constraints().flatMap(constraint => constraint.items().flatMap(item => item.curve.handles() ) )
        const omit = [handlePoint, ...constrainedPoints]
        points = points.filter(p => !omit.some(hp => p.almost(hp) ) )
      }
      return points
    })
  }

  catchSnapPoints(coords) {
    const snapPoints = this.getSnapPoints()
    let closestDist = 99999
    let target
    snapPoints.forEach(p => {
      const dist = this.viewport.renderer.toScreen(p.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
      if(dist < snapDistance && dist < closestDist) {
        closestDist = dist
        target = p
      }
    })
    if(!target) return
    if(!(this.lastSnaps[0] && this.lastSnaps[0].equals(target))) {
      this.lastSnaps.unshift(target)
      if(this.lastSnaps.length > maxSnapReferences) this.lastSnaps.pop()
    }
  }

  snapToGuides(vec, snapToGuides, snapToPoints) {
    if(!vec) return

    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)

    if(!(snapToGuides || snapToPoints)) return localVec

    const screenVec = this.viewport.renderer.toScreen(vec)
    let snapX = this.lastSnaps.find(snap => {
      // Compare plane space X axis..
      const testSnap = snap.clone()//.applyMatrix4(localTransform)
      testSnap.setY(localVec.y)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      // .. in screen space
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    let snapY = this.lastSnaps.find(snap => {
      const testSnap = snap.clone()//.applyMatrix4(localTransform)
      testSnap.setX(localVec.x)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    // const snapVec = new THREE.Vector3(
    //   snapX ? snapX.clone().applyMatrix4(localTransform).x : localVec.x,
    //   snapY ? snapY.clone().applyMatrix4(localTransform).y : localVec.y,
    //   localVec.z
    // ) //XXX z-imprecision
    const snapVec = new THREE.Vector3(
      snapX ? snapX.x : localVec.x,
      snapY ? snapY.y : localVec.y,
      localVec.z
    )
    const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
    const screenSnapVec = this.viewport.renderer.toScreen(worldSnapVec)
    if(snapToGuides) {
      if(snapX) {
        const start = this.viewport.renderer.toScreen(snapX.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'v' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
      if(snapY) {
        const start = this.viewport.renderer.toScreen(snapY.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'h' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
    }
    if(snapX && snapY) {
      this.snapAnchor = this.snapAnchor || {
        type: 'snap',
        pos: screenSnapVec,
        vec: worldSnapVec,
        id: '' + snapVec.x + snapVec.y + snapVec.z,
      }
      if(snapX === snapY && snapToPoints) return snapX//.clone().applyMatrix4(localTransform)
    } else {
      this.snapAnchor = null
    }

    if((snapX || snapY) && snapToGuides) return snapVec

    return localVec
  }
}
