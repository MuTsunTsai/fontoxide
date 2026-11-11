# Contributing to fontoxide

Thanks for your interest! fontoxide is a port of fontTools, so most contributions are
about matching fontTools more closely: a difference in output, a missing feature, a
crash on bad input.

## Setup

You need the latest stable Rust, [uv](https://docs.astral.sh/uv/) and the git
submodules:

```sh
git clone --recurse-submodules https://github.com/MuTsunTsai/fontoxide
```

Before sending a pull request, run:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets   # no warnings
cargo fmt --all --check
```

## How compatibility is checked

- `upstream/fonttools` is the fontTools release being tracked (do not modify it), and
  `tools/reference` is a uv project that runs that exact checkout, with fontTools'
  optional dependencies installed, to produce reference data. Run Python scripts with
  `uv run --project tools/reference python ...` and `PYTHONHASHSEED=0`; to run
  fontTools' own tests in the submodule, set `PYTHONPATH=Lib`.
- `upstream/tests.toml` lists every upstream test with its porting status (`ported`,
  `pending` or `n/a` with a reason). `cargo xtask test-manifest --check` verifies it.
  Ported tests go in one Rust test file per upstream test file, whose doc comment names
  the upstream file, keeping the test names recognizable.
- `cargo xtask parity [PATHS…]` runs fontTools and fontoxide on a font corpus and
  compares their output byte for byte (see [corpus.md](corpus.md)). After changing
  ttLib, subset or varLib, run it and compare the results with the previous run.

## Conventions

- **Port faithfully.** Read the Python source and follow it. Names follow Rust
  conventions, and the rustdoc names the Python counterpart. Floating-point arithmetic
  follows CPython's semantics (`fontoxide_misc::py`).
- **Match fontTools byte for byte.** The default behavior is that of `fonttools[all]`.
  Fallbacks that fontTools only takes when an optional package is missing are not ported.
  Keep the module structure and dependencies of fontTools, so that upstream changes are
  easy to follow.
- **Upstream bugs are reproduced.** When fontTools has a bug that affects its output,
  reproduce it in the reference environment first, then do the same in fontoxide and mark
  the code `UPSTREAM-BUG(Bn)`. Where fontTools crashes and fontoxide is more lenient, or
  fails at a different point, mark it `LENIENT(Ln)`. In a pull request, mark new ones
  `UPSTREAM-BUG:` or `LENIENT:` and describe them; the maintainer assigns the numbers.
  User-visible differences are listed in [COMPATIBILITY.md](COMPATIBILITY.md).
- **Untrusted input.** Crates are `#![forbid(unsafe_code)]`; bad input returns an error
  and never panics.
- Generated code (`cargo xtask codegen`) is not edited by hand: change the generator.
- Tests write only to temporary directories, a unique one per test, since tests run in
  parallel.
- Results that depend on the platform's math library can differ in the last digits
  between platforms, in fontTools too. Test data for them starts with
  `# platform: <sys.platform>` and is compared with `fontoxide_misc::float_vectors`.
- Indent with tabs. Code, comments, documentation and commit messages are in English.
