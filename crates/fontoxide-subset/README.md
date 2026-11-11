# fontoxide-subset

A Rust port of [`fontTools.subset`](https://fonttools.readthedocs.io/en/latest/subset/):
removes the glyphs, characters, layout features and other data a font does not
need. It is part of [fontoxide](https://github.com/MuTsunTsai/fontoxide), a port of
fontTools to Rust.

Most projects should depend on the [`fontoxide`](https://crates.io/crates/fontoxide)
crate, which re-exports this one as `fontoxide::subset` (feature `subset`, on by
default). Depend on this crate directly to keep the dependency tree small.

## Example

```rust,no_run
use fontoxide_subset::{Options, Subsetter};
use fontoxide_ttlib::{Font, ReadOptions, WriteOptions};

let mut font = Font::read(std::fs::read("font.ttf")?, ReadOptions::default())?;
let mut subsetter = Subsetter::new(Options::default());
subsetter.populate(&[], &[], &[], "Hello");
subsetter.subset(&mut font)?;
std::fs::write("subset.ttf", font.write(&WriteOptions::default())?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Options` has the fields of `fontTools.subset.Options`, and `main` takes the same
arguments as `pyftsubset` (`fontoxide subset` on the command line, from
[`fontoxide-cli`](https://crates.io/crates/fontoxide-cli)).

## Compatibility

This version follows **fontTools 4.68.0**. Given the same font and options, the
output is the same as fontTools', byte for byte, as checked against fontTools on
a corpus of fonts. For WOFF 2.0 output, the decompressed data is the same, but the
compressed bytes can differ. Differences from fontTools are listed in
[COMPATIBILITY.md](https://github.com/MuTsunTsai/fontoxide/blob/main/COMPATIBILITY.md).

The crate is pure safe Rust and builds for `wasm32-unknown-unknown`.

## License

MIT, like fontTools. See [LICENSE](LICENSE).
