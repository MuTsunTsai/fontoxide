# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). fontoxide
is 0.x: each update to a new fontTools release bumps the minor version.

## Unreleased

### Changed

- **Breaking:** `to_xml` and `from_xml` moved from the `Table` trait to a new trait,
  `TableXml`, and `fmt::Debug` is no longer a supertrait of `Table`. Programs that never
  read or write TTX no longer link the XML code of every table, which the vtable of
  `dyn Table` kept; a WebAssembly build that reads, instantiates, subsets and saves fonts
  shrank by about 38%. To migrate:
  - import `fontoxide_ttlib::TableXml` to call `to_xml` or `from_xml` on a table type;
  - on a `dyn Table`, call `fontoxide_ttlib::table::table_to_xml` or `table_from_xml`,
    which dispatch on the table's tag;
  - the `Debug` of `dyn Table` shows only its tag; downcast to see the contents.
- The tables of the multi-byte codecs (`shift_jis`, `gb2312`, `big5`, `euc_kr`, `johab`)
  are stored in a compact binary format, 114 KB instead of 734 KB, exported by
  `tools/reference/codecs_export.py`. Decoding and encoding are unchanged.

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
