import Serialize from '../core/serialize.js'
import {
  CreateSketchFeature,
  PlaneFeature,
  ExtrudeFeature,
  RevolveFeature,
  LoftFeature,
  SweepFeature,
  CoilFeature,
  PatternFeature,
  MirrorFeature,
  ShellFeature,
  OffsetFeature,
  BooleanFeature,
  MoveFeature,
  FilletFeature,
  ChamferFeature,
  ReplaceFeature,
  DissolveFeature,
  DraftFeature,
  SplitFeature,
  JointFeature,
  GroupFeature,
  MotionLinkFeature,
  RemoveSolidsFeature,
} from '../core/features.js'
import {
  SheetFaceFeature,
  ContourFlangeFeature,
  FlangeFeature,
  HemFeature,
  BendFeature,
  UnfoldFeature,
  RefoldFeature,
} from '../core/sheet-metal.js'

// Feature types agents create, by the names they use
export const featureTypes = {
  sketch: CreateSketchFeature,
  plane: PlaneFeature,
  extrude: ExtrudeFeature,
  revolve: RevolveFeature,
  loft: LoftFeature,
  sweep: SweepFeature,
  coil: CoilFeature,
  pattern: PatternFeature,
  mirror: MirrorFeature,
  shell: ShellFeature,
  offset: OffsetFeature,
  boolean: BooleanFeature,
  move: MoveFeature,
  fillet: FilletFeature,
  chamfer: ChamferFeature,
  replace: ReplaceFeature,
  dissolve: DissolveFeature,
  draft: DraftFeature,
  split: SplitFeature,
  remove_bodies: RemoveSolidsFeature,
  sheet_face: SheetFaceFeature,
  contour_flange: ContourFlangeFeature,
  flange: FlangeFeature,
  hem: HemFeature,
  bend: BendFeature,
  unfold: UnfoldFeature,
  refold: RefoldFeature,
  joint: JointFeature,
  group: GroupFeature,
  motion_link: MotionLinkFeature,
}

// What settings do, where their titles leave it open
export const settingNotes = {
  plane: {
    offset: 'Distance along the normal of the base plane',
    rotationX: 'Turns the offset plane about its own X axis, before rotationY turns it about its own Y axis. ' +
      'Based on XY, the normal becomes Rx(rotationX)·Ry(rotationY)·Z. Later changes turn the plane further from its current orientation.',
    rotationY: 'Turns the offset plane about its own Y axis, after rotationX',
    position: 'Position along the curve in percent of its length',
  },
  extrude: {
    side: 'Forward follows the normal of the sketch plane',
  },
  pattern: {
    inputs: 'Bodies and components are copied, features are replayed',
    uCount: 'Grid instances along the component X axis, including the original',
    vCount: 'Grid instances along the component Y axis, including the original',
    uOffset: 'Grid step along the component X axis',
    vOffset: 'Grid step along the component Y axis',
    axis: 'Axis of radial patterns',
  },
  mirror: {
    inputs: 'Bodies are copied, features are replayed. Components cannot be mirrored.',
  },
}

// Features created otherwise keep the name they are saved under
export function featureName(feature) {
  const entry = Object.entries(featureTypes).find(([, klass]) => feature.constructor === klass)
  if(entry) return entry[0]
  return Object.keys(Serialize.classes).find(name => Serialize.classes[name] === feature.constructor)
}
