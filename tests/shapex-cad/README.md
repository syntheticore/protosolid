# Shapex saved-document regressions

```sh
node tests/shapex-cad/check.mjs
```

The checker rebuilds Shapex WASM from the current Rust source and loads each
`.cad` like the app does. A document passes when every feature builds without
an error or warning and its references resolve. A final fillet or chamfer must
also select real edges joining two faces. Cases have no checks of their own.
The checker accepts a subdirectory name to run a separate group of cases.

## Failing

Put new robustness cases for existing features in the `failing/` subdirectory.
Only the last feature should fail, so it appears in red when the file is
opened. Each case should cover one distinct failure. Gaps in the numbering
indicate removed duplicates.

Run these cases with `node tests/shapex-cad/check.mjs failing`. Once a case
passes, move its `.cad` file into this directory so the default checker
includes it.
