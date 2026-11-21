import Serialize from '../core/serialize.js'
import {
  CreateSketchFeature,
  PlaneFeature,
  AxisFeature,
  PointFeature,
  ExtrudeFeature,
  RevolveFeature,
  LoftFeature,
  SweepFeature,
  CoilFeature,
  ThreadFeature,
  WebFeature,
  PatternFeature,
  MirrorFeature,
  ShellFeature,
  OffsetFeature,
  BooleanFeature,
  MoveFeature,
  AlignFeature,
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
  axis: AxisFeature,
  point: PointFeature,
  extrude: ExtrudeFeature,
  revolve: RevolveFeature,
  loft: LoftFeature,
  sweep: SweepFeature,
  coil: CoilFeature,
  thread: ThreadFeature,
  web: WebFeature,
  pattern: PatternFeature,
  mirror: MirrorFeature,
  shell: ShellFeature,
  offset: OffsetFeature,
  boolean: BooleanFeature,
  move: MoveFeature,
  align: AlignFeature,
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
  axis: {
    facePoint: 'Where the axis leaves the face. Defaults to the face center.',
  },
  point: {
    axis: 'Any straight edge, sketch line, circle, cylindrical face or axis. Skew axes give the point halfway between them.',
    path: 'Straight curves extend infinitely to cross the plane',
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
  joint: {
    restAngle: 'Turns component 2 about the joint Z axis, away from where the joint aligned it. ' +
      'Rest pose and limits are measured from that aligned placement.',
    restTiltX: 'Turns component 2 about the joint X axis, before restTiltY and restAngle',
    restTiltY: 'Turns component 2 about the joint Y axis, after restTiltX',
    restSlide: 'Moves component 2 along the axis from where the joint aligned it',
    restX: 'Moves component 2 along the joint plane X axis from where the joint aligned it',
    restY: 'Moves component 2 along the joint plane Y axis from where the joint aligned it',
    minAngle: 'Ranges of a full turn or more do not limit rotation',
    maxTilt: 'Largest angle the ball joint Z axis of component 2 may swing away from its aligned direction',
  },
  align: {
    flip: 'Planes face each other by default, flipped they face the same way. Axes point the same way by default.',
    offset: 'Moves the bodies along the normal of the target plane or axis',
    angle: 'Turns the bodies about the normal of the target plane or axis',
  },
  web: {
    curves: 'Open sketch curves of one sketch. Connected curves form mitered chains.',
    extend: 'Runs free curve ends on until they meet walls of the body. Ribs start at the sketch plane, which must cut into the body.',
    side: 'Forward follows the normal of the sketch plane',
  },
  thread: {
    faces: 'Cylindrical faces. A shaft keeps its diameter as the major diameter, a bore as the minor diameter.',
    series: 'Standard pitches follow from each face diameter',
    offset: 'Distance of the thread from the face end chosen by side',
  },
}

// Features created otherwise keep the name they are saved under
export function featureName(feature) {
  const entry = Object.entries(featureTypes).find(([, klass]) => feature.constructor === klass)
  if(entry) return entry[0]
  return Object.keys(Serialize.classes).find(name => Serialize.classes[name] === feature.constructor)
}
