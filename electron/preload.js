const electron = require('electron')

electron.contextBridge.exposeInMainWorld('platform', process.platform)
electron.contextBridge.exposeInMainWorld(
  'platformVersion',
  process.getSystemVersion?.() || '',
)

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

electron.contextBridge.exposeInMainWorld(
  'appPath',
  electron.ipcRenderer.sendSync('get-app-path'),
)

// electron.webFrame.setVisualZoomLevelLimits(1, 1)
// electron.webFrame.setLayoutZoomLevelLimits(0, 0)
