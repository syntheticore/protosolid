const fs = require('fs').promises
const path = require('path')
const {
  app,
  ipcMain,
  BrowserWindow,
  Menu,
  screen,
  nativeTheme,
  dialog,
} = require('electron')

const isMac = process.platform === 'darwin'

const template = [
  // { role: 'appMenu' }
  ...(isMac ? [{
    label: app.name,
    submenu: [
      { role: 'about' },
      { type: 'separator' },
      { role: 'services' },
      { type: 'separator' },
      { role: 'hide' },
      { role: 'hideothers' },
      { role: 'unhide' },
      { type: 'separator' },
      { role: 'quit' }
    ]
  }] : []),
  // { role: 'fileMenu' }
  {
    label: 'File',
    submenu: [
      isMac ? { role: 'close' } : { role: 'quit' }
    ]
  },
  // { role: 'editMenu' }
  {
    label: 'Edit',
    submenu: [
      { role: 'undo' },
      { role: 'redo' },
      { type: 'separator' },
      { role: 'cut' },
      { role: 'copy' },
      { role: 'paste' },
      ...(isMac ? [
        { role: 'pasteAndMatchStyle' },
        { role: 'delete' },
        { role: 'selectAll' },
        { type: 'separator' },
        {
          label: 'Speech',
          submenu: [
            { role: 'startspeaking' },
            { role: 'stopspeaking' }
          ]
        }
      ] : [
        { role: 'delete' },
        { type: 'separator' },
        { role: 'selectAll' }
      ])
    ]
  },
  // { role: 'viewMenu' }
  {
    label: 'View',
    submenu: [
      { role: 'reload' },
      { role: 'forcereload' },
      { role: 'toggledevtools' },
      { type: 'separator' },
      { role: 'resetzoom' },
      { role: 'zoomin' },
      { role: 'zoomout' },
      { type: 'separator' },
      { role: 'togglefullscreen' }
    ]
  },
  // { role: 'windowMenu' }
  {
    label: 'Window',
    submenu: [
      { role: 'minimize' },
      { role: 'zoom' },
      ...(isMac ? [
        { type: 'separator' },
        { role: 'front' },
        { type: 'separator' },
        { role: 'window' }
      ] : [
        { role: 'close' }
      ])
    ]
  },
  {
    role: 'help',
    submenu: [
      {
        label: 'Learn More',
        click: async () => {
          const { shell } = require('electron')
          await shell.openExternal('https://alchemy.org')
        }
      }
    ]
  }
]

const menu = Menu.buildFromTemplate(template)
Menu.setApplicationMenu(menu)

// Keep a global reference of the window object, if you don't, the window will
// be closed automatically when the JavaScript object is garbage collected.
let mainWindow

function createWindow() {
  // Create the browser window.
  mainWindow = new BrowserWindow({
    width: 1250,
    height: 850,
    minWidth: 1000,
    minHeight: 450,
    titleBarStyle: 'hiddenInset',
    devTools: false,
    // backgroundColor: isMac ? false : '#000',
    backgroundColor: '#000',
    show: false,
    frame: isMac,
    // vibrancy: isMac ? 'dark' : undefined,
    // transparent: !isMac,
    // opacity: 0.9,
    // fullscreenWindowTitle: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      scrollBounce: true,
    }
  })

  // and load the index.html of the app.
  mainWindow.loadFile('index.html')
  // mainWindow.webContents.openDevTools()

  mainWindow.on('closed', function() {
    // Dereference the window object
    mainWindow = null
  })

  mainWindow.on('enter-full-screen', function() {
    mainWindow.webContents.send('fullscreen-changed', true)
  })

  mainWindow.on('enter-html-full-screen', function() {
    mainWindow.webContents.send('fullscreen-changed', true)
  })

  mainWindow.on('leave-full-screen', function() {
    mainWindow.webContents.send('fullscreen-changed', false)
  })

  mainWindow.on('leave-html-full-screen', function() {
    mainWindow.webContents.send('fullscreen-changed', false)
  })

  mainWindow.on('maximize', function() {
    mainWindow.webContents.send('maximize-changed', true)
  })

  mainWindow.on('unmaximize', function() {
    mainWindow.webContents.send('maximize-changed', false)
  })

//   // Fix for missing window events with transparent: true
//   if(mainWindow.webContents.browserWindowOptions.transparent) {
//     // rewrite getNormalBounds, maximize, unmaximize and isMaximized API for the transparent window
//     let resizable = mainWindow.isResizable()
//     let normalBounds = mainWindow.getNormalBounds ? mainWindow.getNormalBounds() : mainWindow.getBounds()

//     mainWindow.getNormalBounds = function() {
//       if (!this.isMaximized()) {
//         if (BrowserWindow.prototype.getNormalBounds) {
//           normalBounds = BrowserWindow.prototype.getNormalBounds.call(this)
//         } else {
//           normalBounds = BrowserWindow.prototype.getBounds.call(this)
//         }
//       }
//       return normalBounds
//     }.bind(mainWindow)

//     mainWindow.maximize = function() {
//       normalBounds = this.getNormalBounds() // store the bounds of normal window
//       resizable = this.isResizable() // store resizable value
//       BrowserWindow.prototype.maximize.call(this)
//       if (!BrowserWindow.prototype.isMaximized.call(this)) {
//         // while isMaximized() was returning false, it will not emit 'maximize' event
//         this.emit('maximize', { sender: this, preventDefault: () => {} })
//       }
//       this.setResizable(false) // disable resize when the window is maximized
//     }.bind(mainWindow)

//     mainWindow.unmaximize = function() {
//       const fromMaximized = BrowserWindow.prototype.isMaximized.call(this)
//       BrowserWindow.prototype.unmaximize.call(this)
//       if (!fromMaximized) {
//         // isMaximized() returned false before unmaximize was called, it will not emit 'unmaximize' event
//         this.emit('unmaximize', { sender: this, preventDefault: () => {} })
//       }
//       this.setResizable(resizable) // restore resizable
//     }.bind(mainWindow)

//     mainWindow.isMaximized = function() {
//       const nativeIsMaximized = BrowserWindow.prototype.isMaximized.call(this)
//       if (!nativeIsMaximized) {
//         // determine whether the window is full of the screen work area
//         const bounds = this.getBounds()
//         const workArea = screen.getDisplayMatching(bounds).workArea
//         if (bounds.x <= workArea.x && bounds.y <= workArea.y && bounds.width >= workArea.width && bounds.height >= workArea.height) {
//           return true
//         }
//       }
//       return nativeIsMaximized
//     }.bind(mainWindow)
//   }

  // mainWindow.removeMenu()
}

let splash;

function showSplash() {
  splash = new BrowserWindow({
    width: 300,
    height: 200,
    transparent: true,
    frame: false,
    // vibrancy: 'ultra-dark',
    alwaysOnTop: true,
    webPreferences: {
      nodeIntegration: false,
    }
  })
  splash.loadFile('splash.html')
  setTimeout(createWindow, 250)
}

function setDarkMode() {
  mainWindow.webContents.send('dark-mode', nativeTheme.shouldUseDarkColors)
}
nativeTheme.on('updated', setDarkMode)

app.on('ready', showSplash)

app.on('window-all-closed', function() {
  if(!isMac) app.quit()
})

// macOS only
app.on('activate', function() {
  if (mainWindow === null) createWindow()
})

ipcMain.on('maximize', function() {
  mainWindow.maximize()
});

ipcMain.on('unmaximize', function() {
  mainWindow.unmaximize()
});

ipcMain.on('minimize', function() {
  mainWindow.minimize()
});

ipcMain.on('close', function() {
  mainWindow.close()
});

ipcMain.on('restart', function() {
  app.relaunch()
  app.quit()
});

ipcMain.on('vue-ready', function() {
  setDarkMode()
  setTimeout(() => {
    splash.destroy()
    mainWindow.show()
  }, 1000)
});

ipcMain.handle('get-save-path', (e, format) => {
  return dialog.showSaveDialog({
    properties: ['createDirectory', 'showOverwriteConfirmation'],
    filters: [
      { name: format + ' Files', extensions: [format.toLowerCase()] },
      { name: 'All Files', extensions: ['*'] },
    ]
  }).then(e => e.filePath )
})

ipcMain.handle('save-file', (e, path, data, encoding) => {
  return fs.writeFile(path, data, encoding || 'utf-8')
})

