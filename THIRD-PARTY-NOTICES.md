# Third-party notices

Multilang IME is free software licensed under the GNU General Public License,
version 3 or (at your option) any later version. See `LICENSE`.

The distribution includes or is built from the following third-party components.
Their license terms apply to those components.

## Mozc (mozc_stdio.exe / mozc_server.exe and its runtime)

- Files: `mozc\mozc_stdio.exe` (or `mozc\mozc_server.exe`), `mozc\documents\credits_en.html`
- Source: https://github.com/google/mozc. `mozc_stdio.exe` is built from the Mozc source with a small
  stdin/stdout front end (`mozc-helper/` in this project, GPL-3.0-or-later; the only change to Mozc
  itself is Bazel visibility for that package). `mozc_server.exe`, when shipped, is the official
  release build, unmodified.
- License: BSD 3-Clause, "Copyright 2010-2018, Google Inc."
  Full text: `licenses\mozc-LICENSE.txt`. `mozc\documents\credits_en.html` lists
  the licenses of the components and dictionaries bundled inside Mozc.
- The protocol definitions used by the Mozc client (`mozc/proto`) come from the same
  source under the same license.

## Microsoft Visual C++ runtime

- Files: `mozc\vcruntime140*.dll`, `mozc\msvcp140*.dll`
- Shipped alongside Mozc as distributable code under the Microsoft Visual C++
  Redistributable license terms.

## libhangul Hanja dictionary

- File: `hanja.txt`
- Source: https://github.com/libhangul/libhangul (`data/hanja/hanja.txt`)
- License: BSD 3-Clause, "Copyright (c) 2005,2006 Choe Hwanjin". The full notice is
  kept at the top of the file.

## wordfreq data

- File: `engine_model.json` (word frequencies and key-sequence n-grams derived from
  wordfreq word lists for English, Korean and Japanese)
- Source: https://github.com/rspeer/wordfreq, by Robyn Speer
- License: the wordfreq data is licensed under Creative Commons
  Attribution-ShareAlike 4.0 International (CC BY-SA 4.0),
  https://creativecommons.org/licenses/by-sa/4.0/. The data includes information from
  sources listed in the wordfreq README (Wikipedia, SUBTLEX, OpenSubtitles, Google Books
  Ngrams, the Leeds Internet Corpus, and others). `engine_model.json` is an adaptation
  and is shared under the same license.

## Rust crates (compiled into multilang_tip.dll and backend.exe)

| Crate | License |
| --- | --- |
| windows, windows-core, windows-collections, windows-future, windows-implement, windows-interface, windows-link, windows-numerics, windows-result, windows-strings, windows-threading | MIT OR Apache-2.0 |
| serde, serde_core, serde_derive, serde_json | MIT OR Apache-2.0 |
| prost, prost-derive | Apache-2.0 |
| anyhow, either, itertools, itoa, proc-macro2, quote, syn | MIT OR Apache-2.0 |
| unicode-ident | (MIT OR Apache-2.0) AND Unicode-3.0 |
| memchr | Unlicense OR MIT |
| bytes, zmij | MIT |

Each crate's license text is in its source package on https://crates.io.
Apache-2.0 and MIT are compatible with GPL-3.0.
