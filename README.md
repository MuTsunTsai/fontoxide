# fontoxide

**A Rust port of [fontTools](https://github.com/fonttools/fonttools).**

fontoxide aims to be a complete, behavior-compatible port of fontTools: the same
tables, the same operations (subsetting, instancing, merging, feature compilation, …),
and — wherever it can be verified — byte-for-byte identical output, including TTX.
It is pure safe Rust and compiles to `wasm32-unknown-unknown`.

> **Status:** early development. Nothing is usable yet.
> This version tracks **fontTools 4.68.0**.

## Crates

Each crate corresponds to a fontTools subpackage, e.g. `fontoxide-ttlib` ↔
`fontTools.ttLib`, `fontoxide-subset` ↔ `fontTools.subset`; small top-level modules such
as `fontTools.agl` are modules of `fontoxide-misc`. The `fontoxide` crate
re-exports all of them. Names follow Rust conventions (`TTFont` → `Font`,
`instantiateVariableFont` → `instantiate_variable_font`), and the rustdoc of each item
names its fontTools counterpart.

## Compatibility with fontTools

Compatibility is checked in three ways:

- **Ported upstream tests.** `upstream/tests.toml` lists every test function in
  fontTools' test suite with its porting status (`cargo xtask test-manifest`).
- **Differential testing.** `cargo xtask parity [PATHS…]` runs fontTools and fontoxide
  on a font corpus and compares their output byte for byte. See [corpus.md](corpus.md).
- **Fuzzing and property tests.**

## Development

Requirements: the latest stable Rust, [uv](https://docs.astral.sh/uv/) (for the
fontTools reference environment), and git submodules:

```sh
git clone --recurse-submodules https://github.com/MuTsunTsai/fontoxide
```

`upstream/fonttools` is the fontTools release being tracked. `tools/reference` runs
that exact checkout to produce reference data.

| Command | Purpose |
|---|---|
| `cargo xtask reference` | Show the fontTools reference environment |
| `cargo xtask codegen` | Export table definitions from fontTools and regenerate code |
| `cargo xtask test-manifest` | Sync `upstream/tests.toml` with the upstream test suite |
| `cargo xtask parity [PATHS…]` | Compare against fontTools on a font corpus |
| `cargo xtask upstream-report FROM TO` | List upstream changes between two tags, grouped by crate |

### Updating to a new fontTools release

1. Check out the new tag in `upstream/fonttools`.
2. `cargo xtask upstream-report <old> <new>` for the to-do list.
3. `cargo xtask codegen` and `cargo xtask test-manifest`.
4. Port the changes, and update `synced` in `upstream/map.toml` and `FONTTOOLS_VERSION`.
5. `cargo xtask parity` to re-check against the new release.

## Supported Rust versions

Only the latest stable Rust is supported while fontoxide is pre-1.0.

## License

MIT, like fontTools. See [LICENSE](LICENSE).

The Adobe Glyph List data in `crates/fontoxide-misc/data/agl` is copyright Adobe,
under the BSD-3-Clause license stated at the top of those files.
