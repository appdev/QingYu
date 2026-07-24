# Dejavu Task 3 Go golden fixtures

These fixtures were generated on 2026-07-25 from the read-only Dejavu checkout
at commit `8462fe30163c6e6e95ae2da832cfe76058e0e830` with Go 1.26.2. The checkout's
`go.mod` pins:

- `github.com/siyuan-note/encryption` at
  `v0.0.0-20260715062728-9cb8e9548044`;
- `github.com/klauspost/compress` at `v1.19.0`;
- `github.com/88250/gulu` at
  `v1.2.3-0.20260409163331-8c1dab1828ba`.

`generate.go` calls the pinned encryption KDF/AES functions, Dejavu entity
types, gulu JSON marshaling, and a klauspost zstd encoder configured with
`SpeedDefault`, checksum disabled, and a 512 KiB window. Run it from the root
of that exact Dejavu checkout:

```sh
go run /absolute/path/to/golden/generate.go /absolute/path/to/golden
```

The KDF inputs are password `oracle-password` and salt `oracle-salt`. The
AES-only plaintext is `siyuan`. `file-object.bin` is File JSON compressed then
encrypted. `index-object.bin` is Index JSON compressed without encryption.
AES-GCM nonces are cryptographically random, so regenerated encrypted fixture
hashes will differ while retaining the same wire contract.

The checked-in fixture SHA-256 hashes are:

```text
f0dce2763d6753b75a1826bb3fed506df664a0bf80807775e97feac9cab07fd5  aes-gcm-siyuan.bin
5803452f02b767eae05e3002ad2750d8db5b1b17a202142214220422b64bd769  file-object.bin
11f984d88a5fa861303aa92e507a465ad5726328b6af8fe2a63777cd87234c9f  index-object.bin
2d5f7c44ffb9ca87ba00a076d57496982e3642f2cffbc97c9f96b92c1725096e  kdf-key.bin
```
