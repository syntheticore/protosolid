import { ShapexError } from '../shapexJS/index.js'
import preferences from '../preferences.js'

// An expected modeling failure whose message is meant for the user.
export class ModelingError extends Error {
  constructor(msg) {
    super(msg)
    this.name = 'ModelingError'
  }
}

export function fail(msg) {
  throw new ModelingError(msg)
}

// Shapex reports invalid geometry as ShapexError, and its argument checks
// (degenerate radii, parallel axes, ...) as RangeError.
export function isModelingFailure(error) {
  return error instanceof ModelingError || error instanceof ShapexError || error instanceof RangeError
}

// Converts a modeling failure into the feature status shown in the UI.
// Kernel messages are internal and replaced by the short message given,
// unless debug mode is enabled.
// Anything else is a defect and keeps propagating with its stack.
export function featureError(error, msg) {
  if(!isModelingFailure(error)) throw error
  const userFacing = error instanceof ModelingError || preferences.debugMode
  return { type: 'error', msg: userFacing ? error.message : msg }
}
