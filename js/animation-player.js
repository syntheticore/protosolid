import { markRaw } from 'vue'
import * as THREE from 'three'

import {
  captureMotionLinkStates, localTransform, restoreMotionLinkStates, solveAssembly, worldTransform,
} from './core/assembly.js'
import { sampleTrack, setKeyframe } from './core/animation.js'
import { makeID } from './core/id.js'

export function formatDuration(seconds) {
  return `${seconds.toFixed(1)} s`
}

function depth(component) {
  return component.parent ? depth(component.parent) + 1 : 0
}

// Plays and scrubs an animation by posing its keyframed components, in
// seconds. Components moved in the viewport stay where they were put until
// they are keyframed or the time changes. Closing returns to the pose the
// assembly had when the animation opened.
export default class AnimationPlayer {
  constructor(document, animation) {
    this.document = document
    this.animation = animation
    this.time = 0
    this.playing = false
    this.frame = null
    this.previousFrameTime = null
    this.pose = null
    // Ids of components moved in the viewport since the last keyframe
    this.posed = []
  }

  get duration() {
    return this.animation.duration
  }

  get keyTimes() {
    const times = this.animation.tracks.flatMap(track => track.keyframes.map(keyframe => keyframe.time))
    return [...new Set(times)].sort((a, b) => a - b)
  }

  // Moved components without a track yet
  get offered() {
    return this.posed.filter(id => !this.track(id))
  }

  track(componentId) {
    return this.animation.tracks.find(track => track.componentId == componentId)
  }

  // Remember the pose to start from
  capture() {
    this.pause()
    const tree = this.document.top()
    this.pose = markRaw({
      transforms: new Map(tree.getChildren().map(component => [component.id, component.transform?.clone() || null])),
      links: captureMotionLinkStates(tree),
    })
    this.posed = []
    this.time = 0
    this.stepTo(0, 0)
  }

  restore() {
    const tree = this.document.top()
    tree.getChildren().forEach(component => {
      if(this.pose.transforms.has(component.id)) component.transform = this.pose.transforms.get(component.id)?.clone() || null
    })
    restoreMotionLinkStates(tree, this.pose.links)
  }

  close() {
    this.pause()
    this.restore()
  }

  play() {
    if(this.time >= this.duration) this.seek(0)
    this.playing = true
    this.previousFrameTime = null
    this.frame = requestAnimationFrame(time => this.tick(time))
  }

  tick(frameTime) {
    // Long pauses, like a hidden tab, don't skip ahead
    const elapsed = this.previousFrameTime === null ? 0 : Math.min(frameTime - this.previousFrameTime, 200)
    this.previousFrameTime = frameTime
    this.seek(this.time + elapsed / 1000)
    if(this.time >= this.duration) {
      if(!this.animation.loop) return this.pause()
      this.seek(0)
    }
    this.frame = requestAnimationFrame(time => this.tick(time))
  }

  pause() {
    cancelAnimationFrame(this.frame)
    this.frame = null
    this.playing = false
  }

  toggle() {
    this.playing ? this.pause() : this.play()
  }

  // Unkeyed moves are dropped when the time changes
  seek(time) {
    time = THREE.MathUtils.clamp(time, 0, this.duration)
    if(this.posed.length) this.refresh(time)
    else this.stepTo(this.time, time)
  }

  // Replays the animation from the start, after keyframes changed
  refresh(time = this.time) {
    this.restore()
    this.posed = []
    this.stepTo(0, Math.min(time, this.duration))
  }

  // Large jumps go in steps, so linkages follow continuously as in a drag
  stepTo(from, to) {
    const steps = Math.max(1, Math.ceil(Math.abs(to - from) / (this.duration / 60)))
    for(let i = 1; i <= steps; i++) this.apply(from + (to - from) * i / steps)
    this.time = to
  }

  // Parents go first, so their children's keyframes apply relative to them
  apply(time) {
    const tree = this.document.top()
    this.animation.tracks
      .map(track => ({ track, component: tree.findChild(track.componentId) }))
      .filter(({ track, component }) => component && track.keyframes.length)
      .sort((a, b) => depth(a.component) - depth(b.component))
      .forEach(({ track, component }) => {
        solveAssembly(tree, component, worldTransform(component.parent).multiply(sampleTrack(track, time)))
      })
  }

  // Components were moved in the viewport
  moved(componentIds) {
    this.pause()
    componentIds.forEach(id => this.posed.includes(id) || this.posed.push(id))
  }

  // Puts a moved component back where the animation has it, keeping the
  // other moves
  discard(componentId) {
    const tree = this.document.top()
    const kept = this.posed.filter(id => id != componentId)
      .map(id => [id, worldTransform(tree.findChild(id))])
    this.refresh()
    kept.forEach(([id, world]) => solveAssembly(tree, tree.findChild(id), world))
    this.posed = kept.map(([id]) => id)
  }

  // Keyframes the moved components at the current time. Without moves, all
  // tracks are keyframed to hold their current poses.
  keyframe() {
    const tree = this.document.top()
    const ids = this.posed.length ? this.posed : this.animation.tracks.map(track => track.componentId)
    for(const id of ids) {
      const component = tree.findChild(id)
      const existing = this.track(id)
      const track = existing || { id: makeID(), componentId: id, keyframes: [] }
      // New tracks start from the pose the component had before
      if(!existing && this.time > 0) {
        const initial = this.pose.transforms.get(id)
        setKeyframe(track, 0, (component.designTransform || new THREE.Matrix4()).clone().multiply(initial || new THREE.Matrix4()))
      }
      setKeyframe(track, this.time, localTransform(component))
      if(!existing) this.animation.tracks.push(track)
    }
    this.posed = []
  }

  removeKeyframe(track, keyframe) {
    track.keyframes = track.keyframes.filter(other => other !== keyframe)
    if(!track.keyframes.length) this.removeTrack(track)
  }

  removeTrack(track) {
    this.animation.tracks = this.animation.tracks.filter(other => other !== track)
  }

  moveKeyframe(track, keyframe, time) {
    keyframe.time = THREE.MathUtils.clamp(time, 0, this.duration)
    track.keyframes.sort((a, b) => a.time - b.time)
  }

  // To the next keyframe, or back to the previous one
  step(direction) {
    const times = [0, ...this.keyTimes, this.duration]
    const time = direction > 0 ?
      times.find(time => time > this.time + 1e-6) ?? this.duration :
      times.findLast(time => time < this.time - 1e-6) ?? 0
    this.pause()
    this.seek(time)
  }
}
