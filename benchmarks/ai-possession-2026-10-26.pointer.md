# AI codec possession verification — 2026-10-26

Code under test: `4632dba0`; the model-backed test gate is added with this
record. zenjpegai is pinned at `1192fca6`; zencoolchic at `1cecd620`.
These are functional checks, not a quality or speed comparison.

The model-backed check ran:

```sh
ZENJPEGAI_MODELS=<upstream models directory> cargo test -p evif-probe \
  --features jpegai-models-test --test ai_possession -- --test-threads=1
```

The 64×64 RGB8 JPEG AI input encoded into a `jaic` codestream, which the pinned
decoder read back. The wrapper's scored pixels equaled a separate second
decode of those bytes. Both tests passed; zero ignored. The run-heavy wrapper
reported `rc=0`, 18 s total, peak RSS 0.68 GiB and minimum available memory
29,546 MiB. That 18 s includes compilation; it is not an encoder benchmark.

The six `.pth` files came from the [JPEG AI reference software](https://gitlab.com/wg1/jpeg-ai/jpeg-ai-reference-software),
model beta 0.012, base operating point. Their SHA-256 digests, relative to its
`models/` directory:

```text
1bac8841e7adff8c2b48b2b5a44bf3c28e6785598e681ca6b136962fdae77ac8  VM_common_int/Y_0.012.pth
12f30daebb3151fbffece06cec66c83d714ae7183b17eb62cea9709b4ec30406  VM_common_int/UV_0.012.pth
2e38d718aa00f1bebfaa40e48b255e849f3003291033ec89069fe9552acdaf97  VM_bop/encoder_Y_0.012.pth
0fe3ed0b425bde070bc9d6f147ec08dc89fdebf3c96bcf5d21891f5c39963180  VM_bop/encoder_UV_0.012.pth
9aa156fea0344023a3a00cfe61036de608b7b9fa427ed9e0833c40cb97edfdb4  VM_bop/decoder_Y_0.012.pth
25d146d99f4054b484c81ac50663cef95d11097b2e0be2d4c2c837214f6fe78d  VM_bop/decoder_UV_0.012.pth
```

The later recipe's six-file SHA-256 digest is
`cdc1850088c6517759ecb1cf33c8deb285b8fb8be11fdbceb814c233b1598fc3`.
It hashes a domain label, each relative path length and path, each file length,
and each file's bytes in the order above. An independent Python `hashlib`
calculation agreed with the Rust recipe.

The Cool-chic 8×8 RGB8 test wrote `cool` native bytes; a second decode equaled
the scored pixels. The test passed with zero ignored. In the unoptimized test
profile it ran for 189.80 s; the run-heavy wrapper reported `rc=0`, 267 s
total, peak RSS 0.91 GiB and minimum available memory 26,387 MiB. This is a
functional diagnostic on a shared machine, not a throughput result.

No checkpoint or test bitstream is committed. The later recipe revision
`da5c4f1c` binds
a SHA-256 digest of these six named checkpoint files in addition to the code
revision and model ID. Preserve exact model bytes for reproducible replay.
