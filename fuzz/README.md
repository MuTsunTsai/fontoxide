# Fuzzing fontoxide

fontoxide reads untrusted input (fonts uploaded by users, TTX, feature files,
UFO data) and must not panic on it: bad input is an `Err`. This package runs
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer) targets
against the library. The crates forbid `unsafe`, so the findings are panics:
indexing out of bounds, arithmetic overflow (`overflow-checks` is on in the
release profile here), `unwrap`, stack overflows from deep recursion, huge
allocations and timeouts on small inputs.

It is a separate Cargo workspace (it needs nightly and libFuzzer), depending on
the umbrella crate `fontoxide` with all features.

## Targets

| Target         | What it does                                                                  | `-max_len` |
| -------------- | ----------------------------------------------------------------------------- | ---------- |
| `font_read`    | `Font::read` (first font of a collection), decompile every table, TTX of all tables, `write` | 65536 |
| `woff2`        | first byte selects WOFF 2.0 (even) or WOFF 1.0 (odd); the signature of the rest is overwritten, then read, dumped and written in the same flavor (Brotli quality 0) | 65536 |
| `ttx_import`   | `Font::import_ttx` on the input as (lossy) UTF-8, without loading `src` files; then `write` | 65536 |
| `fealib_parse` | feaLib parser (includes not followed), `as_fea`, and build into a font with the glyph order of the builder tests | 16384 |
| `glif`         | ufoLib `read_glyph_from_string` into a glyph and a point pen, validating and not | 16384 |
| `plist`        | `plistlib::loads_bytes`, then `dumps` of the result | 16384 |
| `subset`       | `Font::read`, subset to a few fixed characters, `write` | 65536 |

`corpus/<target>/` holds small seed corpora taken from fontTools' `Tests`
(fonts under 15 KB, a sample of the AOTS fonts, TTX, feature files, GLIF and
plist files; the WOFF seeds were made from the font seeds with fontTools). Keep
it small: let the corpus that grows while fuzzing live outside the repository,
and pass the seeds as a second, read-only corpus directory (libFuzzer adds new
inputs to the first directory only).

`artifacts/<target>/` holds minimal inputs (hand-made from the fuzzer's
findings) that crash, run out of memory or take far too long. Each one should
become a regression test of the crate it belongs to once fixed; a target can
replay one with `cargo fuzz run <target> artifacts/<target>/<file>`. CI does not
replay them, as they still fail.

## Running on Linux or WSL

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz build -O
# Grow a corpus outside the repository from the committed seeds.
mkdir -p ~/fuzz-corpus/font_read
cargo +nightly fuzz run -O font_read ~/fuzz-corpus/font_read corpus/font_read -- \
	-max_len=65536 -rss_limit_mb=2048 -timeout=10 -max_total_time=1800 -jobs=8 -workers=8
```

In WSL, build in a WSL-local target directory (`CARGO_TARGET_DIR=~/fontoxide-fuzz-target`):
building under `/mnt/c` is much slower.

Minimize a crash and the corpus:

```sh
cargo +nightly fuzz tmin -O font_read artifacts/font_read/crash-...
cargo +nightly fuzz cmin -O font_read ~/fuzz-corpus/font_read
```

## Running on Windows

Use WSL (above). Natively with the MSVC toolchain, linking failed with nightly
2026-11-06 (`LNK2001: unresolved external symbol __start___sancov_cntrs`, with
and without a sanitizer). When that works again, the commands are:

```powershell
cd fuzz
cargo +nightly fuzz build -s none -O
cargo +nightly fuzz run -s none -O font_read $env:TEMP\fuzz-corpus\font_read corpus\font_read -- `
	-max_len=65536 -rss_limit_mb=2048 -timeout=10 -max_total_time=1800 -jobs=8 -workers=8
```

Windows Defender's Controlled Folder Access blocks the nightly `cargo` and the
fuzz binaries from writing under `Documents` (`Cargo.lock`, `artifacts/`,
corpora): either allow them, or set `CARGO_TARGET_DIR` and the corpus and
`-artifact_prefix=` directories outside it.

## CI

The `fuzz` job of `.github/workflows/ci.yml` builds the targets and runs each
for 60 seconds from the committed seeds, so that they keep compiling.
