import { GcsWrapper } from '@salusoft89/planegcs'

let gcs

export function initializeSketchSolver(module) {
  gcs = module
}

export function solveSystem(primitives, parameterGroups = []) {
  // PlaneGCS needs the module constructors when it builds B-spline
  // geometry. Free parameters let point-on-spline constraints slide
  // along the curve instead of pinning the point to the initial snap.
  const freeParams = primitives.filter(primitive => primitive.type == 'free_param')
  primitives = primitives.filter(primitive => primitive.type != 'free_param')

  const makeSolver = (systemPrimitives=primitives, systemFreeParams=freeParams) => {
    const system = new gcs.GcsSystem()
    const solver = new GcsWrapper(system, gcs)
    solver.set_max_iterations(200)
    solver.set_convergence_threshold(1e-10)
    systemFreeParams.forEach(param => solver.push_sketch_param(param.name, param.value, false))
    solver.push_primitives_and_params(systemPrimitives)
    return solver
  }

  const solveWithObjectives = solver => {
    let status = solver.solve(0)
    // PlaneGCS's redundant-system pass can discard temporary objectives.
    // Remove only fully redundant equations from this solver instance so
    // dragging and movement probes can use the remaining freedom.
    while(!solver.has_gcs_conflicting_constraints() && solver.has_gcs_redundant_constraints()) {
      const redundant = solver.gcs.get_redundant()
      for(let index = 0; index < redundant.size(); index++) {
        solver.gcs.clear_by_id(redundant.get(index))
      }
      redundant.delete()
      status = solver.solve(0)
    }
    return status
  }

  const solver = makeSolver()
  // BFGS is cheaper for unreachable pointer objectives. Structural
  // solves keep the default DogLeg algorithm.
  const status = primitives.some(primitive => primitive.temporary) ?
    solveWithObjectives(solver) : solver.solve()
  const conflicting = solver.has_gcs_conflicting_constraints()
  const failed = status >= 2
  // Redundant equations can have a valid solution. They are not a
  // reason to reject the sketch or discard a user's constraint.
  if(conflicting || failed) {
    solver.destroy_gcs_module()
    return { results: [], conflicting, failed, fullyConstrained: [] }
  }
  solver.apply_solution()

  const results = solver.sketch_index.get_primitives()
  const solvedFreeParams = freeParams.map(param => ({
    ...param, value: solver.get_sketch_param_value(param.name),
  }))
  let dof = solver.gcs.dof()
  if(parameterGroups.length) {
    // Diagnosis uses the initial geometry. Recompute at the solution
    // before skipping movement probes for a fully constrained system.
    const diagnosis = makeSolver(results, solvedFreeParams)
    diagnosis.solve()
    dof = diagnosis.gcs.dof()
    diagnosis.destroy_gcs_module()
  }

  // Fixing parameters changes PlaneGCS's redundancy reduction, so DOF
  // differences can report freedom on immovable geometry. Instead, use
  // temporary objectives, as dragging does, to measure whether each
  // parameter can move while preserving the structural constraints.
  const parameterCounts = { point: 2, circle: 1, arc: 3 }
  const primitiveById = new Map(primitives.map(primitive => [primitive.id, primitive]))
  const parameterIndices = primitiveIds => primitiveIds.flatMap(id => {
    const primitive = primitiveById.get(id)
    const start = solver.p_param_index.get(id)
    const count = parameterCounts[primitive?.type] || 0
    return start === undefined ? [] : Array.from({ length: count }, (_, offset) => start + offset)
  }).filter((index, position, indices) => indices.indexOf(index) == position)

  const constrainedParameters = new Map()
  const probeParameter = index => {
    if(solver.gcs.get_is_fixed(index)) return true
    if(constrainedParameters.has(index)) return constrainedParameters.get(index)

    const probe = makeSolver(results, solvedFreeParams)
    const value = probe.gcs.get_p_param(index)
    const parameterScale = Math.max(1, Math.abs(value))
    const probeStep = parameterScale * 1e-3
    const movementTolerance = parameterScale * 1e-7
    const target = probe.gcs.push_p_param(value + probeStep, true)
    // A negative tag makes this an objective on the remaining freedom,
    // rather than an additional equation in the structural diagnosis.
    probe.gcs.add_constraint_equal(index, target, -1, true, 0, 1)
    const status = solveWithObjectives(probe)
    probe.gcs.apply_solution()
    const constrained = status < 2 &&
      Math.abs(probe.gcs.get_p_param(index) - value) <= movementTolerance
    probe.destroy_gcs_module()

    constrainedParameters.set(index, constrained)
    return constrained
  }
  const fullyConstrained = dof == 0 ?
    parameterGroups.map(() => true) : parameterGroups.map(primitiveIds =>
      parameterIndices(primitiveIds).every(probeParameter)
    )

  solver.destroy_gcs_module()

  return { results, conflicting, failed, dof, fullyConstrained }
}
