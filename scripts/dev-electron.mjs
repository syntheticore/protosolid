import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import electron from 'electron'

// Runs the Nuxt dev server and points Electron at it for hot reloading
const root = fileURLToPath(new URL('..', import.meta.url))
const port = 3000
const url = `http://localhost:${port}/`

const nuxt = spawn(process.execPath, [path.join(root, 'node_modules', 'nuxt', 'bin', 'nuxt.mjs'), 'dev', '--port', String(port)], {
  cwd: root,
  stdio: 'inherit',
})

// Nuxt answers with a loading page until the first build is done
async function waitForServer() {
  while(true) {
    try {
      const response = await fetch(url)
      if(response.ok) return
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 500))
  }
}

// The dev server runs in a child process of its own, which needs to go too
function stopNuxt() {
  if(process.platform == 'win32') {
    spawn('taskkill', ['/pid', String(nuxt.pid), '/T', '/F'])
  } else {
    nuxt.kill()
  }
}

await waitForServer()

const app = spawn(electron, [path.join(root, 'electron')], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, DEV_SERVER_URL: url },
})

app.on('exit', stopNuxt)
nuxt.on('exit', code => {
  app.kill()
  process.exit(code)
})
