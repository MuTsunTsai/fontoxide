# fontoxide-ttlib

A Rust port of [`fontTools.ttLib`](https://fonttools.readthedocs.io/en/latest/ttLib/):
the `Font` object (`TTFont`), reading and writing sfnt, TTC, WOFF and WOFF2
files, the table implementations, and TTX serialization. It is part of
[fontoxide](https://github.com/MuTsunTsai/fontoxide), a port of fontTools to Rust.

Most projects should depend on the [`fontoxide`](https://crates.io/crates/fontoxide)
crate, which re-exports this one as `fontoxide::ttlib` (always included). Depend on
this crate directly to keep the dependency tree small.

## Example

```rust,no_run
use fontoxide_ttlib::{Font, ReadOptions, TtxOptions, tables::Head};

let data = std::fs::read("font.ttf")?;
let mut font = Font::read(data, ReadOptions::default())?;
let head: &mut Head = font.require::<Head>()?;
println!("{}", head.units_per_em);
let ttx = font.to_ttx(&TtxOptions::default())?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Features

The features are fontTools' extras, all on by default. Without one, the crate
behaves as fontTools does without that Python package:

| Feature | fontTools extra (package) | Without it |
|---|---|---|
| `woff` | `woff` (brotli, zopfli) | WOFF 2.0, and WOFF 1.0 with `WriteOptions::use_zopfli`, fail with `Error::Import` |
| `graphite` | `graphite` (lz4) | LZ4-compressed `Silf` and `Glat` tables are kept compressed, with a warning |
| `pathops` | `pathops` (skia-pathops) | `remove_overlaps` fails with `Error::Import` |
| `repacker` | `repacker` (uharfbuzz) | `GSUB` and `GPOS` are packed with fontTools' own serializer |

## Compatibility

This version follows **fontTools 4.68.0**. The TTX dumps, binary round trips and
fonts compiled from TTX are the same as fontTools', byte for byte, as checked
against fontTools on a corpus of fonts. Compressed data (WOFF 2.0, WOFF 1.0 with
zopfli, LZ4 in Graphite tables) decompresses to the same content, but the
compressed bytes can differ; `remove_overlaps` gives a geometrically equivalent
result, not an identical one. Differences from fontTools are listed in
[COMPATIBILITY.md](https://github.com/MuTsunTsai/fontoxide/blob/main/COMPATIBILITY.md).

The crate is pure safe Rust and builds for `wasm32-unknown-unknown`.

## License

MIT, like fontTools. See [LICENSE](LICENSE).
