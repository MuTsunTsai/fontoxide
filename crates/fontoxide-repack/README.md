# fontoxide-repack

A Rust port of the [HarfBuzz](https://github.com/harfbuzz/harfbuzz) repacker
(`hb-repacker.hh` and `src/graph/`), which fontTools uses through uharfbuzz's
`serialize_with_tag` to pack `GSUB` and `GPOS` tables: it orders the subtables
and resolves offset overflows by moving, duplicating and splitting subtables and
promoting lookups to Extension lookups. It is part of
[fontoxide](https://github.com/MuTsunTsai/fontoxide), a port of fontTools to Rust.

[`fontoxide-ttlib`](https://crates.io/crates/fontoxide-ttlib) uses this crate
when its `repacker` feature is on (the default), as fontTools does when
uharfbuzz is installed, so most projects do not need it directly. The
[`fontoxide`](https://crates.io/crates/fontoxide) crate re-exports it as
`fontoxide::repack` (feature `repacker`, on by default).

## Compatibility

The output is the same, byte for byte, as that of the HarfBuzz version bundled
with uharfbuzz 0.57.0 (HarfBuzz 15.0.0), which fontTools 4.68.0 uses with
`fonttools[all]`. Differences from fontTools are listed in
[COMPATIBILITY.md](https://github.com/MuTsunTsai/fontoxide/blob/main/COMPATIBILITY.md).

The crate is pure safe Rust and builds for `wasm32-unknown-unknown`.

## License

MIT. See [LICENSE](LICENSE).

This crate is a port of HarfBuzz's repacker, which is under HarfBuzz's "Old
MIT" license. See [LICENSE-HARFBUZZ](LICENSE-HARFBUZZ).
