// Gives the renderer the window.desktop interface it uses to talk to the desktop
// app. Channels are Tauri commands of the same name in snake case, events
// from the app arrive as an array of arguments, after an event object.
(() => {
  // Positional arguments become the named arguments of the commands
  const parameters = {
    'set-save-enabled': ['enabled'],
    'add-recent-file': ['path'],
    'remove-recent-file': ['path'],
    'open-preferences': ['section'],
    'preferences-updated': ['preferences'],
    'mcp-configure': ['config'],
    'mcp-response': ['id', 'response'],
    'set-theme': ['theme'],
    'get-save-path': ['format', 'title'],
    'get-load-path': ['format', 'multiple'],
    'load-file': ['path', 'encoding'],
  }

  // File contents travel as raw bytes, not as JSON
  function bytes(data) {
    if(typeof data == 'string') return new TextEncoder().encode(data)
    if(ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
    return new Uint8Array(data)
  }

  async function call(channel, args) {
    const { invoke } = window.__TAURI__.core
    if(channel == 'save-file') {
      const [path, data] = args
      return invoke('save_file', bytes(data), { headers: { path: encodeURIComponent(path) } })
    }
    const names = parameters[channel] || []
    const result = await invoke(channel.replaceAll('-', '_'), Object.fromEntries(names.map((name, index) => [name, args[index]])))
    return result instanceof ArrayBuffer ? new Uint8Array(result) : result
  }

  window.desktop = {
    on(channel, listener) {
      window.__TAURI__.webviewWindow.getCurrentWebviewWindow()
        .listen(channel, event => listener(event, ...event.payload))
    },
    send(channel, ...args) {
      call(channel, args).catch(error => console.error(`${channel} failed`, error))
    },
    invoke(channel, ...args) {
      return call(channel, args)
    },
  }
})()
