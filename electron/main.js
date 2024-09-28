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
          await shell.openExternal('https://protosolid.org')
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
    // show: false,
    frame: isMac,
    // vibrancy: isMac ? 'dark' : undefined,
    // transparent: !isMac,
    // opacity: 0.9,
    // fullscreenWindowTitle: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      // contextIsolation: false,
      scrollBounce: true,
      // webSecurity: false,
      nodeIntegration: true,
    }
  })

  // and load the index.html of the app.
  mainWindow.loadFile('app/index.html')
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
}

let splash;

function showSplash() {
  splash = new BrowserWindow({
    width: 300,
    height: 200,
    transparent: true,
    frame: false,
    show: false,
    // vibrancy: 'ultra-dark',
    alwaysOnTop: true,
    webPreferences: {
      contextIsolation: false,
    }
  })
  splash.loadFile('app/splash.html')
  setTimeout(() => {
    splash.show()
    console.log('Creating main win')
    setTimeout(createWindow, 1500)
  }, 500)
}

function setDarkMode() {
  mainWindow.webContents.send('dark-mode', nativeTheme.shouldUseDarkColors)
}
nativeTheme.on('updated', setDarkMode)

// app.commandLine.appendSwitch('js-flags', '--expose_gc --max-old-space-size=128')
app.commandLine.appendSwitch('js-flags', '--expose_gc')

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
  const name = (format == 'alc' ? 'ProtoSolid Documents' : format + ' Files')
  return dialog.showSaveDialog({
    properties: ['createDirectory', 'showOverwriteConfirmation'],
    filters: [
      { name, extensions: [format.toLowerCase()] },
      { name: 'All Files', extensions: ['*'] },
    ]
  }).then(e => e.filePath )
})

ipcMain.handle('get-load-path', (e, format) => {
  const name = (format == 'alc' ? 'ProtoSolid Documents' : format + ' Files')
  return dialog.showOpenDialog({
    properties: [],
    filters: [
      { name, extensions: [format.toLowerCase()] },
      { name: 'All Files', extensions: ['*'] },
    ]
  }).then(e => e.filePaths[0] )
})

ipcMain.handle('save-file', (e, path, data, encoding) => {
  return fs.writeFile(path, data, encoding || 'utf-8')
})

ipcMain.handle('load-file', (e, path, encoding) => {
  return fs.readFile(path, encoding || 'utf-8')
})