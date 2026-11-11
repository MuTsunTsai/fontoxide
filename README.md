# fontoxide

[![crates.io](https://img.shields.io/crates/v/fontoxide)](https://crates.io/crates/fontoxide)
[![docs.rs](https://img.shields.io/docsrs/fontoxide)](https://docs.rs/fontoxide)
[![CI](https://img.shields.io/github/actions/workflow/status/MuTsunTsai/fontoxide/ci.yml?branch=main&label=CI)](https://github.com/MuTsunTsai/fontoxide/actions/workflows/ci.yml)
[![fontTools](https://img.shields.io/badge/fontTools-4.68.0-blue)](https://github.com/fonttools/fonttools/releases/tag/4.68.0)
[![MSRV](https://img.shields.io/crates/msrv/fontoxide)](https://github.com/MuTsunTsai/fontoxide/blob/main/Cargo.toml)
[![license](https://img.shields.io/crates/l/fontoxide)](LICENSE)

**A Rust port of [fontTools](https://github.com/fonttools/fonttools).**

fontoxide is a complete, behavior-compatible port of fontTools: the same tables, the
same operations (subsetting, instancing, merging, feature compilation, variable font
building, …) and the same command lines, producing byte-for-byte the same fonts and TTX
as fontTools. It is pure safe Rust, has no C dependencies, and compiles to
`wasm32-unknown-unknown`.

## Status

- Version **0.1.0**, tracking **fontTools 4.68.0**.
- Every fontTools subpackage is ported, except drawing and rendering (plotting, cairo,
  FreeType, …); for those, fontoxide provides the data to draw with.
- All ported upstream tests pass: 3,608 of fontTools' 3,784 tests are ported, the other
  176 don't apply to Rust (see [COMPATIBILITY.md](COMPATIBILITY.md)), none are pending.

## Quick start

```toml
[dependencies]
fontoxide = "0.1"
```

```rust
use std::collections::BTreeMap;

use fontoxide::subset::{Options, Subsetter};
use fontoxide::ttlib::{Font, ReadOptions, TtxOptions, WriteOptions};
use fontoxide::varlib::instancer::{AxisTriple, InstancerOptions, instantiate_variable_font};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Keep only the glyphs needed for some text, like `pyftsubset --text=...`.
	let mut font = Font::read(std::fs::read("font.ttf")?, ReadOptions::default())?;
	let mut subsetter = Subsetter::new(Options::default());
	subsetter.populate(&[], &[], &[], "Hello, world!");
	subsetter.subset(&mut font)?;
	std::fs::write("font.subset.ttf", font.write(&WriteOptions::default())?)?;

	// Pin the weight of a variable font and limit its width,
	// like `fonttools varLib.instancer variable.ttf wght=700 wdth=75:100`.
	let mut font = Font::read(std::fs::read("variable.ttf")?, ReadOptions::default())?;
	let mut limits = BTreeMap::new();
	limits.insert("wght".to_owned(), Some(AxisTriple::pin(700.0)));
	limits.insert("wdth".to_owned(), Some(AxisTriple::range(75.0, 100.0)?));
	instantiate_variable_font(&mut font, &limits, &InstancerOptions::default())?;
	std::fs::write("variable.instance.ttf", font.write(&WriteOptions::default())?)?;

	// Dump to TTX, like `ttx variable.instance.ttf`.
	std::fs::write("variable.instance.ttx", font.to_ttx(&TtxOptions::default())?)?;
	Ok(())
}
```

Names follow Rust conventions (`TTFont` → `Font`, `instantiateVariableFont` →
`instantiate_variable_font`), and the documentation of each item names its fontTools
counterpart. fontTools refers to glyphs by name; fontoxide's tables use glyph IDs.

## Command line

```sh
cargo install fontoxide-cli
```

This installs `fontoxide`, which runs fontTools' command lines: `fontoxide <module>` is
`fonttools <module>`.

```sh
fontoxide ttx font.ttf
fontoxide subset font.ttf --text="Hello, world!" --flavor=woff2
fontoxide varLib.instancer variable.ttf wght=700 wdth=75:100
fontoxide merge a.ttf b.ttf
fontoxide help
```

To also install fontTools' own command names (`fonttools`, `ttx`, `pyftsubset` and
`pyftmerge`, with fontTools' usage and help texts), enable the `fonttools-names` feature:

```sh
cargo install fontoxide-cli --features fonttools-names
```

## Crates

Each crate corresponds to a fontTools subpackage. The umbrella crate `fontoxide`
re-exports all of them, each behind a feature of the same name:

| Crate | `fontoxide::` | fontTools |
|---|---|---|
| `fontoxide-ttlib` | `ttlib` | `fontTools.ttLib` (including `cffLib`, `ttGlyphPen`, `t2CharStringPen`) |
| `fontoxide-misc` | `misc`, `agl`, `afmlib`, `tfmlib` | `fontTools.misc`, `agl`, `afmLib`, `tfmLib` |
| `fontoxide-pens` | `pens` | `fontTools.pens` |
| `fontoxide-unicodedata` | `unicodedata` | `fontTools.unicodedata` |
| `fontoxide-encodings` | (used by `ttlib`) | `fontTools.encodings` |
| `fontoxide-subset` | `subset` | `fontTools.subset` |
| `fontoxide-varlib` | `varlib` | `fontTools.varLib` |
| `fontoxide-merge` | `merge` | `fontTools.merge` |
| `fontoxide-fealib` | `fealib` | `fontTools.feaLib` |
| `fontoxide-otllib` | `otllib` | `fontTools.otlLib` |
| `fontoxide-colorlib` | `colorlib` | `fontTools.colorLib` |
| `fontoxide-designspacelib` | `designspacelib` | `fontTools.designspaceLib` |
| `fontoxide-ufolib` | `ufolib` | `fontTools.ufoLib` |
| `fontoxide-cu2qu` | `cu2qu` | `fontTools.cu2qu` |
| `fontoxide-qu2cu` | `qu2cu` | `fontTools.qu2cu` |
| `fontoxide-svglib` | `svglib` | `fontTools.svgLib` |
| `fontoxide-t1lib` | `t1lib` | `fontTools.t1Lib` |
| `fontoxide-voltlib` | `voltlib` | `fontTools.voltLib` |
| `fontoxide-mtilib` | `mtilib` | `fontTools.mtiLib` |
| `fontoxide-diff` | `diff` | `fontTools.diff` |
| `fontoxide-repack` | `repack` | the HarfBuzz repacker (uharfbuzz's `serialize_with_tag`) |
| `fontoxide` | `font_builder` | `fontTools.fontBuilder` |
| `fontoxide-cli` | | `fontTools.ttx`, `fontTools.__main__` (the `fontoxide` executable) |

## Features

By default, the `fontoxide` crate has everything, like `pip install fonttools[all]`.
With `default-features = false`, it has only `ttlib`, `misc`, `pens` and `unicodedata`;
each other subpackage is a feature of the same name (`subset`, `varlib`, `fealib`, …).
fontTools' optional dependencies are features named after its extras:

| Feature | fontTools extra | Without it |
|---|---|---|
| `woff` | `woff` (brotli, zopfli) | WOFF 2.0, and WOFF 1.0 with `--with-zopfli`, fail with an `ImportError` |
| `graphite` | `graphite` (lz4) | compressed `Silf` and `Glat` tables are kept compressed, with a warning |
| `pathops` | `pathops` (skia-pathops) | removing overlaps fails with an `ImportError` |
| `repacker` | `repacker` (uharfbuzz) | `GSUB` and `GPOS` are packed with fontTools' own serializer |

Without an extra, fontoxide behaves as fontTools does without that Python package. The
individual crates don't turn the extras on; enable them with the features of
`fontoxide-ttlib`.

## Compatibility

For the same input and options, fontoxide produces the same output as fontTools 4.68.0
with `fonttools[all]`, byte for byte. This is checked by the ported upstream tests and by
differential testing against fontTools on a font corpus (`cargo xtask parity`).

The exceptions are documented in [COMPATIBILITY.md](COMPATIBILITY.md). In short:
compressed data (WOFF 2.0, zopfli, LZ4, `.ufoz`) matches after decompression only;
overlap removal uses [linesweeper](https://crates.io/crates/linesweeper) instead of Skia
and is geometrically equivalent; invalid input can fail earlier, or be handled where
fontTools crashes; output that depends on Python's hash order is made deterministic.
fontTools bugs that affect output are reproduced on purpose, and listed there too.

## Versioning

fontoxide is 0.x. Each update to a new fontTools release bumps the minor version, since
the output may change with it. The fontTools version is in the badge above and in
`fontoxide::FONTTOOLS_VERSION`; `fontoxide --version` prints both.

Only the latest stable Rust is supported while fontoxide is pre-1.0.

## Performance

Some measurements against Python fontTools (fontoxide in a release build). They are
single measurements on one machine, so take them as an indication only:

- **merge**: 4–13× faster; for example, merging Segoe UI, Trebuchet and Corbel Bold takes
  2.2 s with fontTools and 0.17 s with fontoxide.
- **varLib.interpolatable**: 25–36× faster; Segoe UI Variable takes 50 s with fontTools
  and 1.5 s with fontoxide.
- **varLib.avar.plan**: 20–30× faster.
- **feaLib**: 7× faster; a feature file for 6,000 glyphs compiles in 420 ms with
  fontTools and 60 ms with fontoxide.
- **Building CFF2 variable fonts**: about 25× faster.

## Development

Requirements: the latest stable Rust, [uv](https://docs.astral.sh/uv/) (for the
fontTools reference environment), and git submodules:

```sh
git clone --recurse-submodules https://github.com/MuTsunTsai/fontoxide
```

`upstream/fonttools` is the fontTools release being tracked. `tools/reference` runs that
exact checkout to produce reference data.

| Command | Purpose |
|---|---|
| `cargo test --workspace` | Run the tests |
| `cargo clippy --workspace --all-targets` | Lint (no warnings allowed) |
| `cargo fmt --all --check` | Check formatting |
| `cargo xtask reference` | Show the fontTools reference environment |
| `cargo xtask parity [PATHS…]` | Compare against fontTools on a font corpus (see [corpus.md](corpus.md)) |
| `cargo xtask test-manifest [--check]` | Sync `upstream/tests.toml` with the upstream test suite |
| `cargo xtask codegen` | Export table definitions from fontTools and regenerate code |
| `cargo xtask coverage` | Measure code coverage (cargo-llvm-cov) |
| `cargo xtask upstream-report FROM TO` | List upstream changes between two tags, grouped by crate |
| `cargo xtask sweep` | Remove stale test binaries and caches from `target/` |

Fuzz targets for [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) are in `fuzz/`.
See [CONTRIBUTING.md](CONTRIBUTING.md) for the conventions.

### Updating to a new fontTools release

1. Check out the new tag in `upstream/fonttools`.
2. `cargo xtask upstream-report <old> <new>` for the to-do list.
3. `cargo xtask codegen` and `cargo xtask test-manifest`.
4. Port the changes, and update `synced` in `upstream/map.toml`, `FONTTOOLS_VERSION` and
   the badge above; bump the minor version.
5. `cargo xtask parity` to re-check against the new release.

## Relationship to fontations and fontc

[fontations](https://github.com/googlefonts/fontations) is a Rust font stack modeled on
HarfBuzz (fast zero-copy reading, shaping, subsetting with skera), and
[fontc](https://github.com/googlefonts/fontc) is a font compiler that replaces fontmake.
Neither is a port of fontTools: they make their own choices, and their output is not
meant to match fontTools'. fontoxide is for when you want fontTools itself, in Rust: its
API, its command lines, TTX, and its exact output.

## License

MIT, like fontTools. See [LICENSE](LICENSE), which includes fontTools' notice.

Some crates include third-party code or data under their own licenses:

- `fontoxide-repack` is a port of the HarfBuzz repacker
  ([`LICENSE-HARFBUZZ`](crates/fontoxide-repack/LICENSE-HARFBUZZ)).
- `fontoxide-misc` includes the Adobe Glyph List data, copyright Adobe, under the
  BSD-3-Clause license ([`LICENSE-AGL`](crates/fontoxide-misc/LICENSE-AGL)).
- `fontoxide-unicodedata` includes data from the Unicode Character Database, under the
  Unicode License v3 ([`LICENSE-UNICODE`](crates/fontoxide-unicodedata/LICENSE-UNICODE)).
- `fontoxide-encodings` includes character mapping tables taken from CPython's codecs,
  under the Python Software Foundation License
  ([`LICENSE-PYTHON`](crates/fontoxide-encodings/LICENSE-PYTHON)).
