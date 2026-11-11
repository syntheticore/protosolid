// https://nuxt.com/docs/api/configuration/nuxt-config

// import wasm from 'vite-plugin-wasm';

import pkg from './package.json'

export default defineNuxtConfig({
  compatibilityDate: '2026-08-30',

  runtimeConfig: {
    public: {
      version: pkg.version,
    },
  },

  ignore: ['dist', 'dist/**', 'rust/**', 'src-tauri/**', 'js/shapexJS/rust/**'],

  experimental: {
    watcher: 'parcel',
  },

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
  ],

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
    // Keeps the Tauri CLI output visible
    clearScreen: false,
    server: {
      watch: {
        ignored: ['**/rust/**', '**/src-tauri/**'],
      },
    },
    css: {
      preprocessorOptions: {
        stylus: {
          additionalData: '@import "../styles/variables.styl"',
        },
      },
    },
  },
})
