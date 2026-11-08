const http = require('http')
const crypto = require('crypto')

// Serves the Model Context Protocol over Streamable HTTP on the loopback
// interface. Messages are answered by the document window, which owns the
// documents and the editing UI. Each request waits for the window's reply,
// so responses are plain JSON and no event streams are offered.

const localHosts = ['127.0.0.1', 'localhost', '[::1]']
const maxBodySize = 16 * 1024 * 1024

function isLocal(url) {
  try {
    return localHosts.includes(new URL(url).hostname)
  } catch(error) {
    return false
  }
}

function sendJSON(res, status, body, headers = {}) {
  res.writeHead(status, { 'Content-Type': 'application/json', ...headers })
  res.end(JSON.stringify(body))
}

function rpcError(id, code, message) {
  return { jsonrpc: '2.0', id, error: { code, message } }
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = []
    let size = 0
    req.on('data', chunk => {
      size += chunk.length
      if(size > maxBodySize) {
        reject(new Error('Request body is too large'))
        req.destroy()
        return
      }
      chunks.push(chunk)
    })
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf-8')))
    req.on('error', reject)
  })
}

function createMcpServer({ getWebContents, onStatus }) {
  let server = null
  let config = { enabled: false, port: null }
  let status = { running: false, url: null, error: null }
  const pending = new Map()
  let nextRequestId = 1

  function setStatus(next) {
    status = next
    onStatus(status)
  }

  function forward(message) {
    const contents = getWebContents()
    if(!contents) return Promise.reject(new Error('ProtoSolid has no open document window'))
    const id = nextRequestId++
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject })
      contents.send('mcp-request', id, message)
    })
  }

  // The window answers every forwarded request, unless it reloads or closes
  function receive(id, response) {
    const request = pending.get(id)
    if(!request) return
    pending.delete(id)
    request.resolve(response)
  }

  function rejectPending(reason) {
    pending.forEach(request => request.reject(new Error(reason)))
    pending.clear()
  }

  async function handle(req, res) {
    // Browsers attach an origin. Refusing foreign ones and foreign host names
    // keeps web pages from reaching the server, including by DNS rebinding.
    if(req.headers.origin && !isLocal(req.headers.origin)) {
      return sendJSON(res, 403, rpcError(null, -32000, 'Forbidden origin'))
    }
    if(!isLocal(`http://${req.headers.host}`)) {
      return sendJSON(res, 403, rpcError(null, -32000, 'Forbidden host'))
    }
    if(new URL(req.url, 'http://localhost').pathname != '/mcp') {
      return sendJSON(res, 404, rpcError(null, -32000, 'Not found, use the /mcp endpoint'))
    }
    if(req.method != 'POST') {
      res.writeHead(405, { Allow: 'POST' })
      return res.end()
    }

    let message
    try {
      message = JSON.parse(await readBody(req))
    } catch(error) {
      return sendJSON(res, 400, rpcError(null, -32700, `Parse error: ${error.message}`))
    }
    if(Array.isArray(message) || typeof message != 'object' || !message) {
      return sendJSON(res, 400, rpcError(null, -32600, 'Expected a single JSON-RPC message'))
    }

    // Notifications and responses need no answer
    if(message.id === undefined || message.method === undefined) {
      res.writeHead(202)
      return res.end()
    }

    let response
    try {
      response = await forward(message)
    } catch(error) {
      return sendJSON(res, 503, rpcError(message.id, -32000, error.message))
    }
    const headers = message.method == 'initialize' ? { 'Mcp-Session-Id': crypto.randomUUID() } : {}
    sendJSON(res, 200, response, headers)
  }

  function start(port) {
    const instance = http.createServer((req, res) => {
      handle(req, res).catch(error => {
        if(!res.headersSent) sendJSON(res, 500, rpcError(null, -32603, error.message))
        else res.end()
      })
    })
    server = instance
    instance.on('error', error => {
      if(server !== instance) return
      server = null
      setStatus({
        running: false,
        url: null,
        error: error.code == 'EADDRINUSE' ? `Port ${port} is already in use` : error.message,
      })
    })
    instance.listen(port, '127.0.0.1', () => {
      if(server === instance) setStatus({ running: true, url: `http://127.0.0.1:${port}/mcp`, error: null })
    })
  }

  function stop() {
    if(!server) return
    server.close()
    server.closeAllConnections()
    server = null
    rejectPending('The MCP server was stopped')
  }

  function configure({ enabled, port }) {
    // A server that failed to start, like on a busy port, retries on reconfiguration
    if(enabled == config.enabled && port == config.port && (server || !enabled)) return
    config = { enabled, port }
    stop()
    if(enabled) start(port)
    else setStatus({ running: false, url: null, error: null })
  }

  return {
    configure,
    receive,
    rejectPending,
    status: () => status,
  }
}

module.exports = { createMcpServer }
