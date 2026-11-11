# Security policy

fontoxide is meant to process untrusted input: fonts, TTX, feature files, UFOs and the
other formats it reads may come from anyone. Its crates are `#![forbid(unsafe_code)]`,
and bad input must return an error. A panic, a hang, a stack overflow or an excessive
allocation on any input is a bug. The parsers are fuzzed with cargo-fuzz (see `fuzz/`).

## Supported versions

Only the latest 0.x release receives fixes.

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub's
[private vulnerability reporting](https://github.com/MuTsunTsai/fontoxide/security/advisories/new)
(the "Report a vulnerability" button on the repository's Security tab), rather than in a
public issue. Include the input that triggers the problem and the API call or command
you ran, if you can.

If you are unsure whether a panic or a hang is a security issue, report it privately
anyway.
