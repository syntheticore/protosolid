// All of the Node.js APIs are available in the preload process.
// It has the same sandbox as a Chrome extension.

const electron = require('electron')

window.electron = {
  fs: require('fs'),
  ipc: electron.ipcRenderer,
  platform: require('os').platform(),
  platformVersion: require('os').release(),
}
