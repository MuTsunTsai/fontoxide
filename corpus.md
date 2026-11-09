# Font corpus

`cargo xtask parity` compares fontoxide with fontTools on a set of fonts. By default it
scans `corpus/`, which is git-ignored; put fonts there or pass paths explicitly.

Suggested sources:

| Source | Why |
|---|---|
| `upstream/fonttools/Tests` | Small fonts exercising edge cases; always available |
| TTX files in `upstream/fonttools/Tests` | Compile them with the reference fontTools (`uv run --project tools/reference fonttools ttx -o corpus/fonttools-ttx/X.ttf X.ttx`) for tables no test font has, such as `COLR`/`CPAL` |
| `tools/corpus/ttx_fonts.py` | Compiles every complete font among the TTX files of `upstream/fonttools/Tests` (about 160; covers `BASE`, `MATH`, variable fonts…) (`uv run --project tools/reference python -I tools/corpus/ttx_fonts.py upstream/fonttools/Tests corpus/fonttools-tests`) |
| `tools/corpus/aat_fonts.py` | Wraps the binary data of fontTools' AAT table tests (`ankr`, `bsln`, `cidg`, `gcid`, `lcar`, `mort`, `morx`, `opbd`, `prop`) into fonts (`uv run --project tools/reference python -I tools/corpus/aat_fonts.py upstream/fonttools/Tests corpus/aat`) |
| `tools/corpus/overflow_ttx.py` | Writes TTX files with `GSUB`/`GPOS` tables that overflow 16-bit offsets, so that compiling them exercises overflow resolution (`uv run --project tools/reference python -I tools/corpus/overflow_ttx.py corpus/overflow`) |
| `tools/corpus/beyond64k_fonts.py` | Converts fonts to the beyond-64k companion tables, and writes two fonts with more than 65536 glyphs (`uv run --project tools/reference python -I tools/corpus/beyond64k_fonts.py corpus/beyond64k <fonts>…`) |
| `tools/corpus/woff2_fonts.py` | Compresses fonts to WOFF 2.0 with the default transforms, the `hmtx` transform, no transforms, and metadata plus private data (`uv run --project tools/reference python -I tools/corpus/woff2_fonts.py corpus/woff2 <fonts>…`) |
| `tools/corpus/ift_probe.py` | Adds `IFT `/`IFTX` tables to a font (`uv run --project tools/reference python tools/corpus/ift_probe.py <in.ttf> corpus/fonttools-ttx/IFT-probe.ttf`) |
| [google/fonts](https://github.com/google/fonts) | Thousands of fonts, many variable |
| [notofonts](https://notofonts.github.io/) | Broad script coverage, CJK, color fonts |
| Iosevka, JetBrains Mono, Source Code Pro | Heavy GSUB use, Extension lookups |
| Legacy Big5 CJK fonts | Non-Unicode cmaps, broken legacy tables |

The `subset` operation is not run by default (`cargo xtask parity --op subset`): both sides
subset each font to every third character of its Unicode `cmap` (or to every third glyph, if
it has no such `cmap`), with the default options.
Other options are passed as `pyftsubset` takes them, e.g.
`cargo xtask parity --op subset --subset-option=--desubroutinize --subset-option=--no-hinting`;
their reference outputs are cached apart.

Besides fonts, the corpus may hold TTX files named `*.parity.ttx`: for these, only the
`fromttx` operation runs, compiling the file itself on both sides.

WOFF 2.0 files are compared with their Brotli streams decompressed and the fields that
depend on the compressed sizes zeroed: fontoxide's pure-Rust Brotli encoder does not always
produce the same bytes as the C library that fontTools uses.

Reference outputs are cached in `target/parity/ref/<fontTools version>/`, keyed by font
content. The latest results are written to `target/parity/report.json`.
