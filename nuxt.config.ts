// https://nuxt.com/docs/api/configuration/nuxt-config

// import wasm from 'vite-plugin-wasm';

export default defineNuxtConfig({
  compatibilityDate: '2024-04-03',

  devtools: {
    enabled: false,

    // timeline: {
    //   enabled: true,
    // },
  },

  ssr: false,
  // target: 'static',
  // generate: { fallback: true },
  // css: ['styles/main.styl'],

  modules: [
    '@pinia/nuxt',
    (process.env.NODE_ENV === 'production') && 'nuxt-electron',
  ].filter(Boolean),

  electron: {
    build: [
      // {
      //   entry: 'electron/main.js',
      // },
      // {
      //   entry: 'electron/preload.js',
      //   onstart(args) {
      //     // Notify the Renderer-Process to reload the page when the Preload-Scripts build is complete,
      //     // instead of restarting the entire Electron App.
      //     args.reload()
      //   },
      // },
    ],
  },

  nitro: {
    // output: {
    //   publicDir: path.join(__dirname, 'your path here')
    // },
    // experimental: {
    //   wasm: true,
    // },
  },

  vite: {
    // plugins: [wasm()],
    css: {
      preprocessorOptions: {
        stylus: {
          additionalData: '@import "../styles/variables.styl"',
        },
      },
    },
  },
})
