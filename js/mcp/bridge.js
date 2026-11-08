import preferences, { emitter as prefEmitter } from '../preferences.js'
import { tools as initialTools } from './tools.js'
import { ToolError as InitialToolError } from './describe.js'

// Tool modules are swapped on hot reload, so agents test changes without restarting the app
let tools = initialTools
let ToolError = InitialToolError
if(import.meta.hot) {
  import.meta.hot.accept(['./tools.js', './describe.js'], ([toolsModule, describeModule]) => {
    if(toolsModule) tools = toolsModule.tools
    if(describeModule) ToolError = describeModule.ToolError
  })
}

// Answers MCP messages the Electron main process receives over HTTP. Tool
// calls run one after another on the active document.

const protocolVersions = ['2025-11-25', '2025-06-18', '2025-03-26']

const instructions = `ProtoSolid is a parametric CAD application. You edit the user's active document like the user does: \
features are added to a timeline at its marker, and the model is regenerated from the timeline.
Start with get_document. Use get_body to find faces and edges, get_sketch for sketch geometry and profiles, \
and get_feature for the settings of feature types and existing features.
To model, add a sketch on an origin plane or planar face with create_feature (type sketch), draw and dimension it, \
then create solid features such as extrude or revolve from its profiles. Refine with fillet, chamfer, shell and others. \
Change existing work with edit_feature and edit_sketch, which regenerate everything after it.
Lengths are in mm and angles in degrees. Settings and dimensions also accept expressions with units and parameters, like "40mm" or "width / 2". \
Y points up, so the ground plane is XZ. The standard views look from +Y (top), +Z (front) and +X (side), home looks from (1, 1, 1). \
New components become active. Use activate_component on the top component to work in and see the whole model again. \
Prefer parameters and dimensions over hard coded geometry, so the user can adapt the design.
Item ids are only valid for the model state they were read from, query again after changes.
Use capture_view to check the result visually.`

function textResult(value) {
  return { content: [{ type: 'text', text: JSON.stringify(value) }] }
}

async function callTool({ name, arguments: args }, app) {
  const tool = tools.find(tool => tool.name == name)
  if(!tool) throw Object.assign(new Error(`Unknown tool ${name}`), { code: -32602 })
  const doc = app.activeDocument()
  await doc.regenerationQueue.catch(() => {})
  try {
    const result = await tool.run(args || {}, { app, doc })
    // Each change is its own undo step, however quickly calls follow
    if(!tool.readOnly) {
      await doc.regenerationQueue.catch(() => {})
      doc.recordHistory()
    }
    if(result?.image) return { content: [{ type: 'image', data: result.image, mimeType: result.mimeType }] }
    return textResult(result)
  } catch(error) {
    if(error instanceof ToolError) return { ...textResult(error.message), isError: true }
    // File dialogs reject with this string when the user closes them
    if(error == 'canceled') return { ...textResult('The user canceled the file dialog, nothing was written or loaded'), isError: true }
    console.error(error)
    const message = tool.readOnly ? `Internal error: ${error.message}` :
      `Internal error: ${error.message}. The document may have been changed partly, use get_document to check its state before retrying.`
    return { ...textResult(message), isError: true }
  }
}

async function dispatch(message, app) {
  switch(message.method) {
    case 'initialize': {
      const requested = message.params?.protocolVersion
      return {
        protocolVersion: protocolVersions.includes(requested) ? requested : protocolVersions[0],
        capabilities: { tools: {} },
        serverInfo: { name: 'protosolid', title: 'ProtoSolid', version: '0.1.0' },
        instructions,
      }
    }
    case 'ping':
      return {}
    case 'tools/list':
      return {
        tools: tools.map(({ name, title, description, inputSchema, readOnly }) => ({
          name,
          title,
          description,
          inputSchema,
          annotations: { readOnlyHint: !!readOnly },
        })),
      }
    case 'tools/call':
      return callTool(message.params, app)
  }
  throw Object.assign(new Error(`Method not found: ${message.method}`), { code: -32601 })
}

// App gives access to the documents: { bus, activeDocument() }
export function startMcpBridge(app) {
  const configure = () => window.ipc.send('mcp-configure', {
    enabled: preferences.mcpEnabled,
    port: preferences.mcpPort,
  })
  configure()
  prefEmitter.on('updated', configure)

  let queue = Promise.resolve()
  window.ipc.on('mcp-request', (e, id, message) => {
    queue = queue.then(async () => {
      let response
      try {
        response = { jsonrpc: '2.0', id: message.id, result: await dispatch(message, app) }
      } catch(error) {
        response = { jsonrpc: '2.0', id: message.id, error: { code: error.code || -32603, message: error.message } }
      }
      window.ipc.send('mcp-response', id, response)
    })
  })
}
