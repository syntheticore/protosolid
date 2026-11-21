import * as THREE from 'three'

import { makeID } from './id.js'
import Serialize from './serialize.js'


// Keyframes store a component's transform relative to its parent, so tracks
// of nested components follow their animated parents.
function keyframeLocal(keyframe) {
  return new THREE.Matrix4().fromArray(keyframe.local)
}

// Holds the first and last keyframes outside of their range. In between,
// positions move linearly and rotations turn along the shortest arc.
export function sampleTrack(track, time) {
  const keyframes = track.keyframes
  const next = keyframes.findIndex(keyframe => keyframe.time > time)
  if(next == 0) return keyframeLocal(keyframes[0])
  if(next < 0) return keyframeLocal(keyframes.at(-1))
  const from = keyframes[next - 1], to = keyframes[next]
  const fraction = (time - from.time) / (to.time - from.time)
  const decompose = keyframe => {
    const position = new THREE.Vector3(), rotation = new THREE.Quaternion(), scale = new THREE.Vector3()
    keyframeLocal(keyframe).decompose(position, rotation, scale)
    return { position, rotation, scale }
  }
  const a = decompose(from), b = decompose(to)
  return new THREE.Matrix4().compose(
    a.position.lerp(b.position, fraction),
    a.rotation.slerp(b.rotation, fraction),
    a.scale.lerp(b.scale, fraction),
  )
}

// Replaces a keyframe at the same time
export function setKeyframe(track, time, local) {
  track.keyframes = track.keyframes.filter(keyframe => Math.abs(keyframe.time - time) > 1e-3)
  track.keyframes.push({ time, local: local.toArray() })
  track.keyframes.sort((a, b) => a.time - b.time)
}


// Keyframed motion of child components. Components moved in the viewport
// while its treelet is open get keyframed at the current time. Tracks run
// concurrently, joints are followed as when dragging.
export class Animation {
  constructor(id) {
    this.id = id || makeID()
    this.title = 'Animation'
    // Seconds
    this.duration = 4
    this.loop = false
    // [{ id, componentId, keyframes: [{ time, local }] }]
    this.tracks = []
  }

  static undump(dump) {
    return Object.assign(new Animation(dump.id), dump)
  }
}
Serialize.register(Animation, 'Animation')
