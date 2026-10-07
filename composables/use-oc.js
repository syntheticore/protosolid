import opencascade from '~/assets/oc/oc.js'
import opencascadeWasm from '~/assets/oc/oc.wasm?url'

const ocp = opencascade({
  locateFile: () => opencascadeWasm,
})

import { init_planegcs_module, GcsWrapper } from '@salusoft89/planegcs'
import wasm_url from '@salusoft89/planegcs/dist/planegcs_dist/planegcs.wasm?url'

const gcsp = init_planegcs_module({ locateFile: () => wasm_url })

export async function useOC() {
  const oc = await ocp
  const gcs = await gcsp
  return {
    oc,
    gcs,

    // Takes a TDocStd_Document, creates a GLB file from it and returns a ObjectURL
    tesselateDoc(doc) {
      // Export a GLB file (this will also perform the meshing)
      const cafWriter = new oc.RWGltf_CafWriter(new oc.TCollection_AsciiString_2("./file.glb"), true)
      cafWriter.Perform_2(new oc.Handle_TDocStd_Document_2(doc), new oc.TColStd_IndexedDataMapOfStringString_1(), new oc.Message_ProgressRange_1())

      // Read the GLB file from the virtual file system
      const glbFile = oc.FS.readFile("./file.glb", { encoding: "binary" })
      return URL.createObjectURL(new Blob([glbFile.buffer], { type: "model/gltf-binary" }))
    },

    // Takes TopoDS_Shape, add to document, create GLB file from it and returns a ObjectURL
    tesselateShapes(shapes_, maxOffset=0.1, maxAngle=0.1) {
      const shapes = Array.isArray(shapes_) ? shapes_ : [shapes_]

      // Create a document add our shapes
      const doc = new oc.TDocStd_Document(new oc.TCollection_ExtendedString_1())
      const shapeTool = oc.XCAFDoc_DocumentTool.ShapeTool(doc.Main()).get()
      for (const s of shapes) {
        shapeTool.SetShape(shapeTool.NewShape(), s)
        // Tell OpenCascade that we want our shape to get meshed
        new oc.BRepMesh_IncrementalMesh_2(s, maxOffset, false, maxAngle, false)
      }

      // Return our visualized document
      return this.tesselateDoc(doc)
    },

    solveSystem(primitives, parameterGroups=[]) {
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

      const solver = makeSolver()
      const status = solver.solve()
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
        const status = probe.solve()
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
    },
  }
}
