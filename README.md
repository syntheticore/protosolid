# ProtoSolid

Cross-platform parametric CAD. Built with Rust & Electron + Nuxt


## Installation

  Install Rust & Node.js

  ```sh
  npm install
  ```

## Usage

  ### Debug in a browser
  ```sh
  npm run dev
  ```

  ### Build as a standalone application
  ```sh
  npm run pack-electron
  ```

## Tests

  ```sh
  npm test
  npm run test:exhaustive
  ```

  The default suite checks every supported Document 9 fillet edge at a 1 mm
  radius, plus all five radii on both revolution rims and a bounded spline edge
  (39 combinations). The exhaustive suite restores all 135 edge/radius
  combinations. Both commands run the fillet and revolution regressions in
  separate Node worker processes and tessellate every resulting body.

  Tests use the existing Shapex WASM package. After changing the Rust kernel,
  rebuild it with `npm run build-shapex` before running the JavaScript tests.

## License

  MIT
