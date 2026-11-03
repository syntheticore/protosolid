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

// const template = [
//   // { role: 'appMenu' }
//   ...(isMac ? [{
//     label: app.name,
//     submenu: [
//       { role: 'about' },
//       { type: 'separator' },
//       { role: 'services' },
//       { type: 'separator' },
//       { role: 'hide' },
//       { role: 'hideothers' },
//       { role: 'unhide' },
//       { type: 'separator' },
//       { role: 'quit' }
//     ]
//   }] : []),
//   // { role: 'fileMenu' }
//   {
//     label: 'File',
//     submenu: [
//       isMac ? { role: 'close' } : { role: 'quit' }
//     ]
//   },
//   // { role: 'editMenu' }
//   {
//     label: 'Edit',
//     submenu: [
//       { role: 'undo' },
//       { role: 'redo' },
//       { type: 'separator' },
//       { role: 'cut' },
//       { role: 'copy' },
//       { role: 'paste' },
//       ...(isMac ? [
//         { role: 'pasteAndMatchStyle' },
//         { role: 'delete' },
//         { role: 'selectAll' },
//         { type: 'separator' },
//         {
//           label: 'Speech',
//           submenu: [
//             { role: 'startspeaking' },
//             { role: 'stopspeaking' }
//           ]
//         }
//       ] : [
//         { role: 'delete' },
//         { type: 'separator' },
//         { role: 'selectAll' }
//       ])
//     ]
//   },
//   // { role: 'viewMenu' }
//   {
//     label: 'View',
//     submenu: [
//       { role: 'reload' },
//       { role: 'forcereload' },
//       { role: 'toggledevtools' },
//       { type: 'separator' },
//       { role: 'resetzoom' },
//       { role: 'zoomin' },
//       { role: 'zoomout' },
//       { type: 'separator' },
//       { role: 'togglefullscreen' }
//     ]
//   },
//   // { role: 'windowMenu' }
//   {
//     label: 'Window',
//     submenu: [
//       { role: 'minimize' },
//       { role: 'zoom' },
//       ...(isMac ? [
//         { type: 'separator' },
//         { role: 'front' },
//         { type: 'separator' },
//         { role: 'window' }
//       ] : [
//         { role: 'close' }
//       ])
//     ]
//   },
//   {
//     role: 'help',
//     submenu: [
//       {
//         label: 'Learn More',
//         click: async () => {
//           const { shell } = require('electron')
//           await shell.openExternal('https://protosolid.org')
//         }
//       }
//     ]
//   }
// ]

const maxRecentFiles = 10
const recentFilesPath = path.join(app.getPath('userData'), 'recent-files.json')
let recentFiles = []

const recentMenu = {
  label: "Open Recent",
  submenu: [],
}

const template = [
  ...(isMac ? [{
    label: app.name,
    submenu: [
      { role: 'about' },
      { type: 'separator' },
      {
        label: "Settings...",
        accelerator: "CmdOrCtrl+,",
        click: () => openPreferencesWindow(),
      },
      { type: 'separator' },
      // { role: 'services' },
      // { type: 'separator' },
      { role: 'hide' },
      { role: 'hideothers' },
      { role: 'unhide' },
      { type: 'separator' },
      { role: 'quit' }
    ]
  }] : []),
  {
    label: 'File',
    submenu: [
      {
        label: "New Document",
        accelerator: "CmdOrCtrl+N",
        click: async () => {
          mainWindow.webContents.send('createFile')
        },
      },
      {
        label: "Open...",
        accelerator: "CmdOrCtrl+O",
        click: async () => {
          mainWindow.webContents.send('openFile')
        },
      },
      recentMenu,
      {
        label: "Connect...",
        click: async () => {
          mainWindow.webContents.send('connectRepo')
        },
      },
      {
        label: "Import...",
        accelerator: "CmdOrCtrl+I",
        click: async () => {
          mainWindow.webContents.send('importFile')
        },
      },
      {
        label: "Export...",
        accelerator: "CmdOrCtrl+E",
        click: async () => {
          mainWindow.webContents.send('exportFile')
        },
      },
      { type: 'separator' },
      {
        id: 'save',
        label: "Save",
        accelerator: "CmdOrCtrl+S",
        click: async () => {
          mainWindow.webContents.send('saveFile')
        },
      },
      {
        label: "Save As...",
        accelerator: "CmdOrCtrl+SHIFT+S",
        click: async () => {
          mainWindow.webContents.send('saveFileAs')
        },
      },
      {
        label: "Save All",
        accelerator: "CmdOrCtrl+ALT+S",
        click: async () => {
          mainWindow.webContents.send('saveAll')
        },
      },
      {
        label: "Publish...",
        click: async () => {
          mainWindow.webContents.send('publishRepo')
        },
      },
      { type: 'separator' },
      {
        label: "Close File",
        accelerator: "CmdOrCtrl+W",
        click: async () => {
          mainWindow.webContents.send('closeFile')
        },
      },
      {
        label: "Close All Files",
        click: async () => {
          mainWindow.webContents.send('closeFiles')
        },
      },
      ...!isMac ? [{ role: 'quit' }] : [],
    ]
  },
  {
    label: 'Edit',
    submenu: [
      { role: 'undo' },
      { role: 'redo' },
      { type: 'separator' },
      { role: 'cut' },
      { role: 'copy' },
      { role: 'paste' },
      { role: 'delete' },
      // { type: 'separator' },
      // { role: 'selectAll' },
    ]
  },
  {
    label: 'View',
    submenu: [
      { role: 'toggledevtools' },
      { role: 'togglefullscreen' },
    ]
  },
  { role: 'windowMenu' },
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

let menu

// Menus can't be changed after building, so recent files rebuild the whole menu
function buildMenu() {
  recentMenu.submenu = recentFiles.length ? [
    ...recentFiles.map(filePath => ({
      label: filePath,
      click: () => mainWindow.webContents.send('openFile', filePath),
    })),
    { type: 'separator' },
    {
      label: "Clear Recent",
      click: () => setRecentFiles([]),
    },
  ] : [{ label: "No Recent Files", enabled: false }]
  const saveEnabled = menu ? menu.getMenuItemById('save').enabled : true
  menu = Menu.buildFromTemplate(template)
  menu.getMenuItemById('save').enabled = saveEnabled
  Menu.setApplicationMenu(menu)
}

function setRecentFiles(files) {
  recentFiles = files.slice(0, maxRecentFiles)
  buildMenu()
  return fs.writeFile(recentFilesPath, JSON.stringify(recentFiles))
}

buildMenu()

// The list doesn't exist before the first document is opened or saved
fs.readFile(recentFilesPath, 'utf-8').then(data => {
  recentFiles = JSON.parse(data)
  buildMenu()
}).catch(() => {})

// Keep a global reference of the window object, if you don't, the window will
// be closed automatically when the JavaScript object is garbage collected.
let mainWindow
let preferencesWindow

// Set by scripts/dev-electron.mjs to load the Nuxt dev server instead of the generated app
const devServerUrl = process.env.DEV_SERVER_URL

function loadApp(window, query) {
  if(!devServerUrl) return window.loadFile('app/index.html', { query })
  const url = new URL(devServerUrl)
  for(const key in query) url.searchParams.set(key, query[key])
  return window.loadURL(url.href)
}

function openPreferencesWindow(section) {
  if(preferencesWindow && !preferencesWindow.isDestroyed()) {
    if(section) preferencesWindow.webContents.send('show-preferences-section', section)
    if(preferencesWindow.isMinimized()) preferencesWindow.restore()
    preferencesWindow.show()
    preferencesWindow.focus()
    return
  }

  preferencesWindow = new BrowserWindow({
    parent: mainWindow,
    width: 760,
    height: 500,
    minWidth: 680,
    minHeight: 480,
    resizable: false,
    title: 'Settings',
    backgroundColor: '#1c2127',
    show: false,
    frame: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      scrollBounce: true,
    },
  })

  preferencesWindow.setMenu(null)

  loadApp(preferencesWindow, { window: 'preferences', ...(section && { section }) })

  preferencesWindow.once('ready-to-show', () => preferencesWindow.show())
  preferencesWindow.webContents.on('did-finish-load', () => {
    preferencesWindow.webContents.send('dark-mode', nativeTheme.shouldUseDarkColors)
  })
  preferencesWindow.on('closed', () => {
    preferencesWindow = null
  })
}

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
    backgroundColor: '#1c2127',
    show: false,
    frame: isMac,
    // vibrancy: isMac ? 'dark' : undefined,
    // Windows needs an opaque window to retain native minimize/maximize animations.
    // transparent: !isMac,
    // opacity: 0.9,
    // fullscreenWindowTitle: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      // contextIsolation: false,
      scrollBounce: true,
      // webSecurity: false,
      // nodeIntegration: true,
    }
  })

  // and load the index.html of the app.
  loadApp(mainWindow)
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
  BrowserWindow.getAllWindows().forEach(window => {
    if(!window.isDestroyed()) {
      window.webContents.send('dark-mode', nativeTheme.shouldUseDarkColors)
    }
  })
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

ipcMain.on('get-app-path', function(event) {
  event.returnValue = app.getAppPath()
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

ipcMain.on('set-save-enabled', function(event, enabled) {
  menu.getMenuItemById('save').enabled = enabled
})

ipcMain.handle('get-recent-files', () => recentFiles)

ipcMain.on('add-recent-file', (event, filePath) => {
  setRecentFiles([filePath, ...recentFiles.filter(file => file != filePath)])
})

ipcMain.on('remove-recent-file', (event, filePath) => {
  setRecentFiles(recentFiles.filter(file => file != filePath))
})

ipcMain.on('open-preferences',(event, section) => openPreferencesWindow(section))

ipcMain.on('close-preferences', function(event) {
  if(preferencesWindow && !preferencesWindow.isDestroyed()
    && preferencesWindow.webContents.id === event.sender.id) {
    preferencesWindow.close()
  }
})

ipcMain.on('preferences-updated', function(event, preferences) {
  BrowserWindow.getAllWindows().forEach(window => {
    if(!window.isDestroyed() && window.webContents.id !== event.sender.id) {
      window.webContents.send('preferences-updated', preferences)
    }
  })
})

ipcMain.on('set-theme', function(event, theme) {
  if(['system', 'light', 'dark'].includes(theme)) {
    nativeTheme.themeSource = theme
  }
})

ipcMain.on('vue-ready', function() {
  setDarkMode()
  setTimeout(() => {
    if(splash && !splash.isDestroyed()) splash.destroy()
    if(mainWindow && !mainWindow.isDestroyed()) mainWindow.show()
  }, 1000)
});

// Formats are comma-separated extensions, with or without leading dots.
const formatExtensions = format => format.toLowerCase().split(',').map(extension => extension.trim().replace(/^\./, ''))
const formatName = extension => extension == 'cad' ? 'ProtoSolid Documents' : `${extension.toUpperCase()} Files`

// Each extension is offered as its own file type.
ipcMain.handle('get-save-path', (e, format, title) => {
  const extensions = formatExtensions(format)
  return dialog.showSaveDialog({
    ...(title && { defaultPath: `${title}.${extensions[0]}` }),
    properties: ['createDirectory', 'showOverwriteConfirmation'],
    filters: [
      ...extensions.map(extension => ({ name: formatName(extension), extensions: [extension] })),
      { name: 'All Files', extensions: ['*'] },
    ]
  }).then(e => e.filePath )
})

// All extensions open as one file type, named after the first.
// With multiple, all chosen paths are returned as an array.
ipcMain.handle('get-load-path', (e, format, multiple) => {
  const isImage = format.toLowerCase() == 'image/*'
  const name = isImage ? 'Image Files' : formatName(formatExtensions(format)[0])
  const extensions = isImage
    ? ['avif', 'bmp', 'gif', 'ico', 'jpeg', 'jpg', 'png', 'svg', 'webp']
    : formatExtensions(format)
  return dialog.showOpenDialog({
    properties: multiple ? ['openFile', 'multiSelections'] : ['openFile'],
    filters: [
      { name, extensions },
      { name: 'All Files', extensions: ['*'] },
    ]
  }).then(e => multiple ? e.filePaths : e.filePaths[0] )
})

ipcMain.handle('save-file', (e, path, data, encoding) => {
  return fs.writeFile(path, data, encoding || 'utf-8')
})

// Without an encoding, the file is read as raw bytes
ipcMain.handle('load-file', (e, path, encoding) => {
  return fs.readFile(path, encoding)
})
