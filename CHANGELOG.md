# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). fontoxide
is 0.x: each update to a new fontTools release bumps the minor version.

## 0.1.0 — 2026-11-11

Tracks fontTools 4.68.0.

### Added

- First release: a complete port of fontTools in 23 crates, producing byte-for-byte the
  same output as fontTools 4.68.0 with `fonttools[all]`, except for the differences
  listed in [COMPATIBILITY.md](COMPATIBILITY.md).
- Every fontTools subpackage except drawing and rendering: ttLib (with cffLib, TTX, WOFF
  and WOFF 2.0, all table types), subset, varLib (build, instancer, mutator, merger,
  interpolatable, avar, featureVars, …), merge, feaLib, otlLib, fontBuilder, colorLib,
  designspaceLib, ufoLib, cu2qu, qu2cu, pens, svgLib, t1Lib, voltLib, mtiLib, diff,
  unicodedata, encodings, agl, afmLib, tfmLib and misc, plus a port of the HarfBuzz
  repacker.
- The `fontoxide` command line (`fontoxide-cli`), which runs every fontTools command line
  (`fontoxide ttx`, `fontoxide subset`, `fontoxide varLib.instancer`, …); the
  `fonttools-names` feature also installs `fonttools`, `ttx`, `pyftsubset` and
  `pyftmerge`.
- Cargo features for each subpackage in the umbrella crate `fontoxide`, and for
  fontTools' extras (`woff`, `graphite`, `pathops`, `repacker`); all are on by default,
  like `pip install fonttools[all]`.
- Pure safe Rust (`#![forbid(unsafe_code)]`), building for `wasm32-unknown-unknown`.
