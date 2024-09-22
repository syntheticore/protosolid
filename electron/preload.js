const electron = require('electron')

electron.contextBridge.exposeInMainWorld('platform', require('os').platform())
electron.contextBridge.exposeInMainWorld('platformVersion', require('os').release())

electron.contextBridge.exposeInMainWorld('ipc', {
  on(...args) {
    const [channel, listener] = args
    return electron.ipcRenderer.on(channel, (event, ...args) => listener(event, ...args))
  },
  off(...args) {
    const [channel, ...omit] = args
    return electron.ipcRenderer.off(channel, ...omit)
  },
  send(...args) {
    const [channel, ...omit] = args
    return electron.ipcRenderer.send(channel, ...omit)
  },
  invoke(...args) {
    const [channel, ...omit] = args
    return electron.ipcRenderer.invoke(channel, ...omit)
  },
})
