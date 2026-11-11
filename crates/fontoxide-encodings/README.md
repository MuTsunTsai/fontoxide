# fontoxide-encodings

A Rust port of [`fontTools.encodings`](https://fonttools.readthedocs.io/en/latest/encodings/),
plus the Python codecs that fontTools relies on for `name` table records: the
Mac legacy encodings (including fontTools' own `x_mac_*_ttx` codecs) and the
CJK encodings (Shift-JIS, GB 2312, Big5, EUC-KR, Johab). It is part of
[fontoxide](https://github.com/MuTsunTsai/fontoxide), a port of fontTools to Rust.

[`fontoxide-ttlib`](https://crates.io/crates/fontoxide-ttlib) uses this crate to
decode and encode `name` records, so most projects do not need it directly. The
[`fontoxide`](https://crates.io/crates/fontoxide) crate does not re-export it;
depend on this crate to use the codecs yourself.

## Example

```rust,no_run
use fontoxide_encodings::codecs::decode;

assert_eq!(decode("mac_roman", b"caf\x8e").as_deref(), Ok("café"));
```

## Compatibility

This version follows **fontTools 4.68.0**. The codecs decode and encode exactly
as CPython's codecs do, with the same error handlers; the mapping tables in
`data/codecs` were exported from CPython 3.13 in the fontTools reference
environment. Differences from fontTools are listed in
[COMPATIBILITY.md](https://github.com/MuTsunTsai/fontoxide/blob/main/COMPATIBILITY.md).

The crate is pure safe Rust and builds for `wasm32-unknown-unknown`.

## License

MIT, like fontTools. See [LICENSE](LICENSE).
The character mapping tables are taken from CPython's codecs, under the Python
Software Foundation License: see [LICENSE-PYTHON](LICENSE-PYTHON).
