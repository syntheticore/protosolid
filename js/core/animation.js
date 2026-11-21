import * as THREE from 'three'

import { makeID } from './id.js'
import Serialize from './serialize.js'


// Keyframes store a component's transform relative to its parent, so tracks
// of nested components follow their animated parents.
function keyframeLocal(keyframe) {
  return new THREE.Matrix4().fromArray(keyframe.local)
}

// Keyframes without an easing move linearly. Smooth keyframes are passed
// with a tangent from their neighbors, or start and stop smoothly at the ends
// of a track. Step keyframes hold until the next keyframe.
export const easings = ['linear', 'smooth', 'step']

function decompose(keyframe) {
  const position = new THREE.Vector3(), rotation = new THREE.Quaternion(), scale = new THREE.Vector3()
  keyframeLocal(keyframe).decompose(position, rotation, scale)
  return { position, rotation, scale }
}

// Cubic Hermite basis at a fraction of a segment
function hermite(t) {
  const t2 = t * t, t3 = t2 * t
  return [2 * t3 - 3 * t2 + 1, t3 - 2 * t2 + t, -2 * t3 + 3 * t2, t3 - t2]
}

// Rotation vector, axis times angle, of the shortest turn from rotation a to
// rotation b, in a's frame
function turn(a, b) {
  const delta = a.clone().invert().multiply(b)
  if(delta.w < 0) delta.set(-delta.x, -delta.y, -delta.z, -delta.w)
  const vector = new THREE.Vector3(delta.x, delta.y, delta.z)
  const sine = vector.length()
  return vector.multiplyScalar(sine < 1e-9 ? 2 : 2 * Math.atan2(sine, delta.w) / sine)
}

// Rotation q turned by a rotation vector in its own frame
function rotate(q, vector) {
  const angle = vector.length()
  if(angle < 1e-9) return q.clone()
  return q.clone().multiply(new THREE.Quaternion().setFromAxisAngle(vector.clone().divideScalar(angle), angle))
}

// Holds the first and last keyframes outside of their range. In between,
// positions and rotation vectors follow cubic curves through the keyframes.
// Linear segments turn along the shortest arc at constant speed.
export function sampleTrack(track, time) {
  const keyframes = track.keyframes
  const next = keyframes.findIndex(keyframe => keyframe.time > time)
  if(next == 0) return keyframeLocal(keyframes[0])
  if(next < 0) return keyframeLocal(keyframes.at(-1))
  const index = next - 1
  if(keyframes[index].easing == 'step') return keyframeLocal(keyframes[index])

  const poses = keyframes.map(decompose)
  const times = keyframes.map(keyframe => keyframe.time)
  const duration = times[next] - times[index]
  const t = (time - times[index]) / duration
  const [h00, h10, h01, h11] = hermite(t)
  const smooth = key => keyframes[key].easing == 'smooth'
  const interior = key => key > 0 && key < keyframes.length - 1
  const span = key => times[key + 1] - times[key - 1]

  // Velocities at the ends of this segment, per second
  const travel = (a, b, seconds) => poses[b].position.clone().sub(poses[a].position).divideScalar(seconds)
  const velocity = key => {
    if(!smooth(key)) return travel(index, next, duration)
    if(!interior(key)) return new THREE.Vector3()
    return travel(key - 1, key + 1, span(key))
  }
  const position = poses[index].position.clone().multiplyScalar(h00)
    .addScaledVector(velocity(index), h10 * duration)
    .addScaledVector(poses[next].position, h01)
    .addScaledVector(velocity(next), h11 * duration)

  const rotations = poses.map(pose => pose.rotation)
  const start = rotations[index]
  const change = turn(start, rotations[next])
  // Angular velocities at the ends of this segment, per second, in the
  // segment start's frame. Smooth keyframes take theirs from the turn
  // between their neighbors, measured in the previous neighbor's frame.
  const angularVelocity = key => {
    if(!smooth(key)) return change.clone().divideScalar(duration)
    if(!interior(key)) return new THREE.Vector3()
    return turn(rotations[key - 1], rotations[key + 1]).divideScalar(span(key))
      .applyQuaternion(start.clone().invert().multiply(rotations[key - 1]))
  }
  const rotation = rotate(start, change.clone().multiplyScalar(h01)
    .addScaledVector(angularVelocity(index), h10 * duration)
    .addScaledVector(angularVelocity(next), h11 * duration))

  return new THREE.Matrix4().compose(
    position,
    rotation,
    poses[index].scale.clone().lerp(poses[next].scale, t),
  )
}

// Replaces a keyframe at the same time, keeping its easing
export function setKeyframe(track, time, local) {
  const replaced = track.keyframes.find(keyframe => Math.abs(keyframe.time - time) <= 1e-3)
  track.keyframes = track.keyframes.filter(keyframe => keyframe !== replaced)
  track.keyframes.push({ time, local: local.toArray(), ...replaced?.easing && { easing: replaced.easing } })
  track.keyframes.sort((a, b) => a.time - b.time)
}


// Keyframed motion of child components. Components moved in the viewport
// while its treelet is open get keyframed at the current time. Tracks run
// concurrently, joints are followed as when dragging. Traced points leave
// trails of where they went.
export class Animation {
  constructor(id) {
    this.id = id || makeID()
    this.title = 'Animation'
    // Seconds
    this.duration = 4
    this.loop = false
    // [{ id, componentId, keyframes: [{ time, local, easing? }] }]
    this.tracks = []
    // Points whose paths are drawn while the animation runs, in the
    // coordinates of their components
    // [{ id, componentId, point: [x, y, z] }]
    this.tracers = []
  }

  static undump(dump) {
    return Object.assign(new Animation(dump.id), dump)
  }
}
Serialize.register(Animation, 'Animation')
