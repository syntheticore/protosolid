# ProtoSolid

Cross-platform parametric CAD. Built with Rust & Tauri + Nuxt


## Installation

  Install Rust, Node.js and the [Tauri prerequisites](https://tauri.app/start/prerequisites/)

  ```sh
  npm install
  ```

## Usage

  ### Debug in a browser
  ```sh
  npm run dev
  ```

  ### Debug in the desktop app
  ```sh
  npm run dev-tauri
  ```

  ### Build as a standalone application
  ```sh
  npm run pack
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

  Shapex is a separate Rust library in `rust/lib/shapex`. Run its native tests
  from the repository root with:

  ```sh
  cargo test --manifest-path rust/lib/shapex/Cargo.toml --release
  ```

## License

  MIT
