export class ShapexError extends Error {}

export class Point2 {
  constructor(x?: number, y?: number)
  static new(x: number, y: number): Point2
  readonly x: number
  readonly y: number
  toArray(): [number, number]
}

export class Vec2 {
  constructor(x?: number, y?: number)
  static new(x: number, y: number): Vec2
  readonly x: number
  readonly y: number
  magnitude(): number
  toArray(): [number, number]
}

export class Point3 {
  constructor(x?: number, y?: number, z?: number)
  static new(x: number, y: number, z: number): Point3
  readonly x: number
  readonly y: number
  readonly z: number
  toArray(): [number, number, number]
}

export class Vec3 {
  constructor(x?: number, y?: number, z?: number)
  static new(x: number, y: number, z: number): Vec3
  readonly x: number
  readonly y: number
  readonly z: number
  magnitude(): number
  dot(other: Vec3): number
  cross(other: Vec3): Vec3
  normalized(): Vec3
  scaled(scale: number): Vec3
  toArray(): [number, number, number]
}

export { Vec3 as Vector3 }

export class Matrix3 {
  constructor(x: Vec3, y: Vec3, z: Vec3)
  static fromCols(x: Vec3, y: Vec3, z: Vec3): Matrix3
  static identity(): Matrix3
  readonly x: Vec3
  readonly y: Vec3
  readonly z: Vec3
}

export class Quaternion {
  constructor(s: number, x: number, y: number, z: number)
  static new(s: number, x: number, y: number, z: number): Quaternion
  readonly s: number
  readonly v: Vec3
}

export class ModelTolerance {
  constructor(linear?: number, relative?: number, angular?: number, parameter?: number)
  static new(linear: number, relative: number, angular: number, parameter: number): ModelTolerance
  static default(): ModelTolerance
  readonly linear: number
  readonly relative: number
  readonly angular: number
  readonly parameter: number
  linearAtScale(scale: number): number
  toArray(): [number, number, number, number]
}

export class ParameterRange {
  constructor(start: number, end: number)
  static new(start: number, end: number): ParameterRange
  readonly start: number
  readonly end: number
  span(): number
  reversed(): ParameterRange
  contains(parameter: number, tolerance: ModelTolerance): boolean
}

export class Axis3 {
  constructor(origin: Point3, direction: Vec3)
  static new(origin: Point3, direction: Vec3): Axis3
  static through(start: Point3, end: Point3): Axis3
  origin(): Point3
  direction(): Vec3
  pointAt(distance: number): Point3
}

export class Plane3 {
  constructor(origin: Point3, u: Vec3, v: Vec3, tolerance?: ModelTolerance)
  static new(origin: Point3, u: Vec3, v: Vec3, tolerance: ModelTolerance): Plane3
  static xy(): Plane3
  static xz(): Plane3
  static yz(): Plane3
  static fromOriginNormal(origin: Point3, normal: Vec3): Plane3
  static fromFrame(frame: Frame3): Plane3
  origin(): Point3
  u(): Vec3
  v(): Vec3
  normal(): Vec3
}

export class Frame3 {
  constructor(origin: Point3, x: Vec3, y: Vec3, tolerance?: ModelTolerance)
  static new(origin: Point3, x: Vec3, y: Vec3, tolerance: ModelTolerance): Frame3
  static fromZ(origin: Point3, z: Vec3): Frame3
  origin(): Point3
  x(): Vec3
  y(): Vec3
  z(): Vec3
  axis(): Axis3
}

export class RigidTransform3 {
  constructor(rotation: Matrix3, translation: Vec3, tolerance?: ModelTolerance)
  static new(rotation: Matrix3, translation: Vec3, tolerance: ModelTolerance): RigidTransform3
  static identity(): RigidTransform3
  static fromQuaternion(rotation: Quaternion, translation: Vec3, tolerance?: ModelTolerance): RigidTransform3
  rotation(): Matrix3
  translation(): Vec3
  transformPoint(point: Point3): Point3
  transformVector(vector: Vec3): Vec3
  inverse(): RigidTransform3
  then(next: RigidTransform3): RigidTransform3
}

export class Line2 {
  constructor(origin: Point2, direction: Vec2)
  static new(origin: Point2, direction: Vec2): Line2
  origin(): Point2
  direction(): Vec2
}

export class Circle2 {
  constructor(center: Point2, radius: number)
  static new(center: Point2, radius: number): Circle2
  center(): Point2
  radius(): number
}

export class Line3 {
  constructor(origin: Point3, direction: Vec3)
  static new(origin: Point3, direction: Vec3): Line3
  static through(start: Point3, end: Point3): Line3
  origin(): Point3
  direction(): Vec3
}

export class Circle3 {
  constructor(center: Point3, xRadius: Vec3, yRadius: Vec3, tolerance?: ModelTolerance)
  static new(center: Point3, xRadius: Vec3, yRadius: Vec3, tolerance: ModelTolerance): Circle3
  static fromAxis(axis: Axis3, radius: number, tolerance?: ModelTolerance): Circle3
  center(): Point3
  radius(): number
  xRadius(): Vec3
  yRadius(): Vec3
  normal(): Vec3
}

export class NurbsCurve2 {
  constructor(degree: number, controlPoints: Iterable<Point2>, weights: Iterable<number>, knots: Iterable<number>)
  static new(degree: number, controlPoints: Iterable<Point2>, weights: Iterable<number>, knots: Iterable<number>): NurbsCurve2
  degree(): number
  controlPoints(): readonly Point2[]
  weights(): readonly number[]
  knots(): readonly number[]
}

export class TrimmedCurve2 {
  constructor(curve: Line2 | Circle2 | NurbsCurve2, range: ParameterRange)
  static new(curve: Line2 | Circle2 | NurbsCurve2, range: ParameterRange): TrimmedCurve2
  static line(start: Point2, end: Point2): TrimmedCurve2
  static circle(center: Point2, radius: number): TrimmedCurve2
  static arcThrough(start: Point2, interior: Point2, end: Point2): TrimmedCurve2
  curve(): Line2 | Circle2 | NurbsCurve2
  range(): ParameterRange
  reversed(): TrimmedCurve2
}

export class Wire2 {
  constructor(segments: Iterable<TrimmedCurve2>, tolerance?: ModelTolerance)
  static new(segments: Iterable<TrimmedCurve2>, tolerance: ModelTolerance): Wire2
  static polygon(vertices: Iterable<Point2>, tolerance?: ModelTolerance): Wire2
  segments(): readonly TrimmedCurve2[]
  signedArea(): number
  area(): number
  isCounterClockwise(): boolean
  reversed(): Wire2
}

export class TessellationOptions {
  constructor(chordTolerance?: number, angularTolerance?: number, maximumDepth?: number)
  static new(chordTolerance: number, angularTolerance: number, maximumDepth: number): TessellationOptions
  static default(): TessellationOptions
  readonly chordTolerance: number
  readonly angularTolerance: number
  readonly maximumDepth: number
}

export class FilletOptions {
  constructor(radius: number, followTangentEdges?: boolean)
  static new(radius: number): FilletOptions
  readonly radius: number
  readonly followTangentEdges: boolean
  withTangentPropagation(enabled: boolean): FilletOptions
}

export const BooleanKind: Readonly<{
  Union: 'union'
  Difference: 'difference'
  Intersection: 'intersection'
}>
export type BooleanKindValue = typeof BooleanKind[keyof typeof BooleanKind]

export class TriangleMesh {
  readonly positions: readonly Point3[]
  readonly normals: readonly Vec3[]
  readonly triangles: readonly (readonly [number, number, number])[]
  isEmpty(): boolean
}

export class Polyline {
  readonly parameters: readonly number[]
  readonly points: readonly Point3[]
}

export interface NativeValue {
  readonly raw: unknown
  free(): void
}

export interface Face extends NativeValue {
  id(): string
  surface(): { kind: string, value?: Plane3, axis?: Axis3, center?: Point3 }
  boundaryEdges(): Edge[]
  tessellate(options?: TessellationOptions): TriangleMesh
}

export interface Edge extends NativeValue {
  id(): string
  endpoints(): Point3[]
  curve(): Line3 | Circle3 | { kind: string }
  connectedFaces(): Face[]
  tessellate(options?: TessellationOptions): Polyline
}

export interface Vertex extends NativeValue {
  id(): string
  point(): Point3
}

export interface Solid extends NativeValue {
  volume(): number
  surfaceArea(): number
  centroid(): Point3
  faces(): Face[]
  edges(): Edge[]
  vertices(): Vertex[]
  tessellate(options?: TessellationOptions): TriangleMesh
  toCompound(): Compound
  union(other: Solid): Compound
  difference(other: Solid): Compound
  intersection(other: Solid): Compound
}

export interface Resolution<T extends Face | Edge | Vertex = Face | Edge | Vertex> {
  readonly kind: 'preserved' | 'modified' | 'split' | 'merged' | 'deleted' | 'unrelated'
  readonly entities: readonly T[]
}

export interface CreationRole {
  readonly kind: string
  readonly boundary?: string
  readonly holeIndex?: number
  readonly segment?: number
  readonly part?: number
}

export interface Compound extends NativeValue {
  volume(): number
  surfaceArea(): number
  centroid(): Point3
  solids(): Solid[]
  faces(): Face[]
  edges(): Edge[]
  vertices(): Vertex[]
  tessellate(options?: TessellationOptions): TriangleMesh
  boolean(other: Compound, kind: BooleanKindValue): Compound
  union(other: Compound): Compound
  difference(other: Compound): Compound
  intersection(other: Compound): Compound
  combined(others: Iterable<Compound>): Compound
  offsetFaces(faces: Iterable<Face>, distance: number): Compound
  shell(openings: Iterable<Face>, distance: number): Compound
  fillet(edges: Iterable<Edge>, radius: number): Compound
  filletWithOptions(edges: Iterable<Edge>, options: FilletOptions): Compound
  chamfer(edges: Iterable<Edge>, distance: number, followTangentEdges?: boolean): Compound
  draft(faces: Iterable<Face>, plane: Plane3, angle: number): Compound
  split(plane: Plane3, keep: string): Compound
  withoutSolids(solids: Iterable<Solid>): Compound
  moveFaces(faces: Iterable<Face>, transform: RigidTransform3): Compound
  replaceFace(target: Face, replacement: Face): Compound
  transformed(transform: RigidTransform3): Compound
  resolve<T extends Face | Edge | Vertex>(entity: T): Resolution<T>
  isSemanticEdge(edge: Edge): boolean
  creationRole(entity: Face | Edge): CreationRole | undefined
  facesGeneratedFrom(entity: Face | Edge): Face[]
}

export interface Profile {
  outer(): Wire2
  holes(): readonly Wire2[]
  area(): number
  tolerance(): ModelTolerance
  extrude(plane: Plane3, vector: Vec3): Compound
  revolve(plane: Plane3, axis: Axis3, angle: number): Compound
  extrudeUpToFace(plane: Plane3, target: Compound, limit: Face, offset?: number): Compound
  revolveUpToFace(plane: Plane3, axis: Axis3, target: Compound, limit: Face, forward?: boolean): Compound
  revolveBetweenFaces(plane: Plane3, axis: Axis3, fromBody: Compound, from: Face, untilBody: Compound, until: Face, forward?: boolean): Compound
  sweep(plane: Plane3, railPlane: Plane3, rail: TrimmedCurve2): Compound
  loft(plane: Plane3, sections: Iterable<{ profile: Profile, plane: Plane3 }>, alignSections?: boolean): Compound
  loftWithAlignment(plane: Plane3, sections: Iterable<{ profile: Profile, plane: Plane3 }>, alignSections: boolean): Compound
}

export interface ProfileConstructor {
  new(outer: Wire2, holes?: Iterable<Wire2>, tolerance?: ModelTolerance): Profile
  new: (outer: Wire2, holes?: Iterable<Wire2>, tolerance?: ModelTolerance) => Profile
}

export interface Shapex {
  Point2: typeof Point2
  Vec2: typeof Vec2
  Point3: typeof Point3
  Vec3: typeof Vec3
  Vector3: typeof Vec3
  Matrix3: typeof Matrix3
  Quaternion: typeof Quaternion
  ModelTolerance: typeof ModelTolerance
  ParameterRange: typeof ParameterRange
  Axis3: typeof Axis3
  Frame3: typeof Frame3
  Plane3: typeof Plane3
  RigidTransform3: typeof RigidTransform3
  Line2: typeof Line2
  Circle2: typeof Circle2
  NurbsCurve2: typeof NurbsCurve2
  Line3: typeof Line3
  Circle3: typeof Circle3
  TrimmedCurve2: typeof TrimmedCurve2
  Wire2: typeof Wire2
  Profile: ProfileConstructor
  TessellationOptions: typeof TessellationOptions
  FilletOptions: typeof FilletOptions
  BooleanKind: typeof BooleanKind
  TriangleMesh: typeof TriangleMesh
  Polyline: typeof Polyline
  Face: new(raw: unknown) => Face
  Edge: new(raw: unknown) => Edge
  Vertex: new(raw: unknown) => Vertex
  Solid: new(raw: unknown) => Solid
  Compound: new(raw: unknown) => Compound
}

export interface InitOptions {
  bindings?: Record<string, unknown>
  moduleUrl?: string | URL
  wasmUrl?: string | URL
  wasm?: WebAssembly.Module | BufferSource | Response
}

export function createShapex(bindings: Record<string, unknown>): Shapex
export function initShapex(options?: InitOptions): Promise<Shapex>
export default initShapex
