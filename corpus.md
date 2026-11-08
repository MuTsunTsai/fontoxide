# Font corpus

`cargo xtask parity` compares fontoxide with fontTools on a set of fonts. By default it
scans `corpus/`, which is git-ignored; put fonts there or pass paths explicitly.

Suggested sources:

| Source | Why |
|---|---|
| `upstream/fonttools/Tests` | Small fonts exercising edge cases; always available |
| TTX files in `upstream/fonttools/Tests` | Compile them with the reference fontTools (`uv run --project tools/reference fonttools ttx -o corpus/fonttools-ttx/X.ttf X.ttx`) for tables no test font has, such as `COLR`/`CPAL` |
| [google/fonts](https://github.com/google/fonts) | Thousands of fonts, many variable |
| [notofonts](https://notofonts.github.io/) | Broad script coverage, CJK, color fonts |
| Iosevka, JetBrains Mono, Source Code Pro | Heavy GSUB use, Extension lookups |
| Legacy Big5 CJK fonts | Non-Unicode cmaps, broken legacy tables |

Reference outputs are cached in `target/parity/ref/<fontTools version>/`, keyed by font
content. The latest results are written to `target/parity/report.json`.
