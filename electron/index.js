// Modules to control application life and create native browser window
const {app, ipcMain, BrowserWindow, Menu} = require('electron')
const path = require('path')

// Keep a global reference of the window object, if you don't, the window will
// be closed automatically when the JavaScript object is garbage collected.
let mainWindow

function createWindow () {
  // Create the browser window.
  mainWindow = new BrowserWindow({
    width: 900,
    minWidth: 900,
    height: 600,
    minHeight: 400,
    titleBarStyle: 'hiddenInset',
    scrollBounce: true,
    devTools: true,
    backgroundColor: '#000',
    show: false,
    // frame: false,
    // vibrancy: 'ultra-dark',
    // transparent: true,
    // opacity: 0.9,
    // fullscreenWindowTitle: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
    }
  })

  // and load the index.html of the app.
  mainWindow.loadFile('index.html')

  // Open the DevTools.
  // mainWindow.webContents.openDevTools()

  // Emitted when the window is closed.
  mainWindow.on('closed', function () {
    // Dereference the window object, usually you would store windows
    // in an array if your app supports multi windows, this is the time
    // when you should delete the corresponding element.
    mainWindow = null
  })

  mainWindow.on('enter-full-screen', function () {
    mainWindow.webContents.send('fullscreen-changed', true)
  })
  mainWindow.on('enter-html-full-screen', function () {
    mainWindow.webContents.send('fullscreen-changed', true)
  })
  mainWindow.on('leave-full-screen', function () {
    mainWindow.webContents.send('fullscreen-changed', false)
  })
  mainWindow.on('leave-html-full-screen', function () {
    mainWindow.webContents.send('fullscreen-changed', false)
  })
}

// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.on('ready', createWindow)

// Quit when all windows are closed.
app.on('window-all-closed', function () {
  // On macOS it is common for applications and their menu bar
  // to stay active until the user quits explicitly with Cmd + Q
  if (process.platform !== 'darwin') app.quit()
})

app.on('activate', function () {
  // On macOS it's common to re-create a window in the app when the
  // dock icon is clicked and there are no other windows open.
  if (mainWindow === null) createWindow()
})

ipcMain.on('ping', function() {
  mainWindow.webContents.send('pong')
});

ipcMain.on('vue-ready', function() {
  setTimeout(() => {
    mainWindow.show()
  }, 500)
});

// In this file you can include the rest of your app's specific main process
// code. You can also put them in separate files and require them here.
