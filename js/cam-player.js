import { markRaw } from 'vue'

import { CamSimulation } from './core/cam.js'

// Simulation speeds, as multiples of the real machine's
export const playbackSpeeds = [1, 10, 30, 100, 300, 1000]

// Plays and scrubs the machining simulation of a setup's toolpaths, in
// machining time. Until it starts, the toolpaths are shown instead.
export default class CamPlayer {
  constructor(onFrame) {
    this.onFrame = onFrame
    this.result = null
    this.simulation = null
    this.time = 0
    this.playing = false
    // 100×
    this.speed = 4
    this.frame = null
    this.previousFrameTime = null
  }

  get duration() {
    return this.result?.minutes || 0
  }

  get canPlay() {
    return !!this.result?.moves.length
  }

  load(result) {
    this.stop()
    this.result = result
  }

  play() {
    if(!this.canPlay) return
    if(this.time >= this.duration) this.seek(0)
    this.playing = true
    this.previousFrameTime = null
    this.frame = requestAnimationFrame(time => this.tick(time))
  }

  tick(frameTime) {
    // Long pauses, like a hidden tab, don't skip ahead
    const elapsed = this.previousFrameTime === null ? 0 : Math.min(frameTime - this.previousFrameTime, 200)
    this.previousFrameTime = frameTime
    this.seek(this.time + elapsed / 60000 * playbackSpeeds[this.speed - 1])
    if(this.time >= this.duration) return this.pause()
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

  seek(time) {
    if(!this.canPlay) return
    this.simulation ||= markRaw(new CamSimulation(this.result))
    this.simulation.seek(time)
    this.time = this.simulation.time
    this.onFrame(this.simulation)
  }

  // Back to the toolpaths
  stop() {
    this.pause()
    this.simulation = null
    this.time = 0
    this.onFrame(null)
  }

  // To the start of the next operation, or back to the start of the current
  // or previous one
  step(direction) {
    const starts = this.result.operations.map(operation => operation.start)
    const time = direction > 0 ?
      starts.find(start => start > this.time + 1e-6) ?? this.duration :
      starts.findLast(start => start < this.time - this.duration * 0.002) ?? 0
    this.pause()
    this.seek(time)
  }

  get operationIndex() {
    const operations = this.result?.operations || []
    const index = operations.findLastIndex(operation => operation.start <= this.time)
    return Math.max(0, index)
  }
}
