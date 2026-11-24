# Kiwi (Korean spacing + grammar hints)

Engine: bab2min/Kiwi v0.24.1, Apache-2.0 (LICENSE in this folder, copied from the v0.24.1 tag).
Binding: our own minimal FFI in `crates/dtool-text/src/kiwi_ffi.rs` (libloading, Kiwi C API `kiwi.h`/`capi.h`). No kiwi-rs, no runtime download; everything is offline.

## Release assets (downloaded once at dev time)

| Asset | URL | Bytes | SHA-256 |
|---|---|---|---|
| kiwi_win_x64_v0.24.1.zip | https://github.com/bab2min/Kiwi/releases/download/v0.24.1/kiwi_win_x64_v0.24.1.zip | 38261195 | 857576C5E8492EC66F8327E35572B20F286B092A8E760FE780ACE1A99AFC3B30 |
| kiwi_model_v0.24.1_base.tgz | https://github.com/bab2min/Kiwi/releases/download/v0.24.1/kiwi_model_v0.24.1_base.tgz | 87967233 | 50D163E1AF817444EF21F7EC5EA07EF69FB347282D57A6B49260F2039D4DC346 |
| LICENSE | https://raw.githubusercontent.com/bab2min/Kiwi/v0.24.1/LICENSE | 11372 | 4070EF92694190AE0D19410380654C37F0602E8A2AA3DDCC45E2DAD3039B5B20 |

## Bundle layout (what ships next to dtool.exe)

```
<exe dir>\kiwi\kiwi.dll                 (from zip: lib\kiwi.dll, 23307264 bytes, sha256 8E47F47A4209B655032E7F9A2A2FB5B062A6DB71254E979A550D5132EBC336AA)
<exe dir>\kiwi\model\*                  (from tgz: models\cong\base\*, 9 files, 109229245 bytes)
<exe dir>\kiwi\LICENSE                  (Apache-2.0, Kiwi)
<exe dir>\kiwi\NOTICE                   (Kiwi NOTICE; Apache-2.0 section 4(d) requires shipping it)
<exe dir>\kiwi\THIRD_PARTY_NOTICES.md   (licences of the code statically linked into kiwi.dll)
```

`fetch_kiwi.ps1 -Dest <dir>` also copies LICENSE, NOTICE and THIRD_PARTY_NOTICES.md into `<dir>` when `<dir>` is not this folder.

Total about 132.5 MB unpacked. The binaries and model are NOT in git (cong.mdl alone is 75.9 MB); `.gitignore` here keeps them out.

## Lookup order at runtime (`ko_grammar::candidate_dirs`)

1. `%DTOOL_KIWI_DIR%` (dev/test override)
2. `<exe dir>\kiwi` (shipped bundle)
3. `crates/dtool-text/data/kiwi` (dev checkout after running the fetch script)

Loaded lazily on first Hangul text (`ko_grammar::warm_kiwi`, also from `spelling::warm_korean`). English-only documents never load it.

## Idle unload

Kiwi lives on its own worker thread (`dtool-kiwi`). It is loaded, used and closed only on that thread, and the DLL is freed on unload; closing it from another thread left about 340 MB stuck in kiwi.dll's allocator.
After `ko_grammar::DEFAULT_IDLE_UNLOAD` (300 s) with no Korean check, the reaper drops the worker. The next Korean check reloads it (about 2 s).
`ko_grammar::set_idle_unload_secs(n)` sets the timeout (0 = never unload). The app passes `Prefs::ko_grammar_idle_unload_s` to it (prefs field is in a hub patch).
Measured (debug test): private bytes 604 MB loaded, 42 MB after unload (34 MB of that is hunspell-ko), 612 MB after reload.

## Common-errors list

`crates/dtool-text/data/ko_common_errors.tsv` is embedded as the default list (`wrong<TAB>right[<TAB>mode]`).
Modes: none = prefix of an eojeol (josa and endings may follow), `any` = anywhere in an eojeol, `exact` = whole eojeol, `final` = whole eojeol at the end of a sentence.
A user file at `%APPDATA%\DTool\ko_common_errors.tsv` (same format) adds entries or overrides them; `ko_grammar::reload_common_errors()` re-reads it.
List hits are grammar issues, and they win over Kiwi issues in the same range.

## Dev setup

```
powershell -ExecutionPolicy Bypass -File crates\dtool-text\data\kiwi\fetch_kiwi.ps1 -Dest crates\dtool-text\data\kiwi
```

The script downloads the two assets, verifies SHA-256, and lays out `kiwi.dll` + `model\`.
Tests skip with `KIWI_SKIP` when no bundle is found; set `DTOOL_KIWI_REQUIRED=1` to make a missing bundle a failure.

## Third-party code inside kiwi.dll (to list on the About/licenses screen)

`THIRD_PARTY_NOTICES.md` in this folder holds the full licence texts at the submodule commits pinned by the v0.24.1 tag: Eigen (MPL-2.0, with its BSD files), cpp-btree (Apache-2.0), streamvbyte (Apache-2.0), cpuinfo (BSD-2-Clause), mimalloc (MIT), nlohmann/json (MIT). tclap and googletest are CLI/test only and are not in kiwi.dll.
`NOTICE` is Kiwi's own NOTICE file.
