# Dejavu Task 3 and Task 4 Go golden fixtures

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

For Task 4, `generate.go` also creates `chunk-boundaries.json` with the pinned
`github.com/restic/chunker` v0.5.0 implementation and polynomial
`0x3DA3358B4DC173`. Its 20 MiB input starts from xorshift64 state
`0x4d595df4d0f33173`; each byte applies left-13, right-7, and left-17 XOR
steps with uint64 wrapping and takes the low byte. Each JSON entry records the
Go chunk offset, length, and SHA-1 of its original bytes.

`chunk-max-boundaries.json` uses the same pinned chunker with an 8 MiB + 1
byte input starting from xorshift64 seed `825`. The first boundary is forced by
`MaxSize` at exactly 8 MiB and the second boundary is the final byte, so this
fixture distinguishes the maximum-boundary rule from a single EOF chunk.

The checked-in fixture SHA-256 hashes are:

```text
f0dce2763d6753b75a1826bb3fed506df664a0bf80807775e97feac9cab07fd5  aes-gcm-siyuan.bin
5803452f02b767eae05e3002ad2750d8db5b1b17a202142214220422b64bd769  file-object.bin
11f984d88a5fa861303aa92e507a465ad5726328b6af8fe2a63777cd87234c9f  index-object.bin
2d5f7c44ffb9ca87ba00a076d57496982e3642f2cffbc97c9f96b92c1725096e  kdf-key.bin
29f678581b00be7f309911063e4be20b82bb263e7ed7f42559bccf68397aa0b2  chunk-boundaries.json
9bd6187b445255a4323013d2b9e55d822712ef5c9d729200ad11c5430771d4ca  chunk-max-boundaries.json
```
