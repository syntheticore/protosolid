// import initOpenCascade from "opencascade.js"
// import opencascade from 'opencascade.js/dist/opencascade.full.js'
// import opencascadeWasm from "opencascade.js/dist/opencascade.full.wasm?url"

// const oc = await initOpenCascade({
//   mainJs: opencascade,
//   mainWasm: opencascadeWasm,
// })

import opencascade from '~/assets/oc/oc.js'
import opencascadeWasm from '~/assets/oc/oc.wasm?url'

// import opencascade from '~/assets/oc/opencascade.full.js'
// import opencascadeWasm from '~/assets/oc/opencascade.full.wasm?url'

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

    solveSystem(primitives) {
      const system = new gcs.GcsSystem()
      const solver = new GcsWrapper(system)

      solver.set_max_iterations(200) // 100
      solver.set_convergence_threshold(1e-10)

      solver.push_primitives_and_params(primitives)
      solver.solve()
      solver.apply_solution()

      const results = solver.sketch_index.get_primitives()

      solver.destroy_gcs_module()

      return results
    },
  }
}
