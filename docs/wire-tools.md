# Working with major-1 files

The [wire specification](../SPEC.md) defines symmetric media and appendable
evaluations. `evif wire` packages existing lossless carriers; it does not encode
video, infer signal interpretation, fetch external assets, or run metrics.

```
evif wire pack plan.json output.evif
evif wire inspect output.evif
evif wire append output.evif score.json
```

`pack` refuses an existing output file. A failure after writing starts retains the
partial output and reports its path. An incomplete media section has no valid
MEND, so a strict reader refuses it. `append` validates the existing file and the
new record, then preserves the score JSON bytes exactly. It does not overwrite
existing scores. Serialize writers; interrupted appends are reported as truncated
tails rather than silently repaired.

The plan is an object with:

```json
{
  "assets": [],
  "segments": [],
  "bitstreams": []
}
```

This empty outline is not valid media. Populate `assets` with ASST objects from
the specification. Each `segments` entry is `{"segment": <SEGM>, "path": "clip.png"}`
for embedded storage, or `{"segment": <SEGM>, "path": null}` for external storage.
Paths resolve relative to the plan file, not the current directory. A segment's
sample hashes must come from its actual decoded frames. `pack` checks declared
payload hashes and media structure, but cannot certify those sample hashes.

Optional `bitstreams` entries contain `asset` (local ID), `codec` (four ASCII
characters, e.g. `"avif"` or `"evif"`), and `path`. The same encoding works for
reference and candidate. Omit this array or leave it empty when original encoded
bytes are unnecessary. Original bytes never substitute for rendition segments.

The packer prints media hashes to use in the `candidate`/`reference` fields of a
score envelope. Score `data` belongs to the named adapter and can retain metric
versions, viewing conditions, transformations and results. Use explicit status
instead of inventing a numeric result when a metric was not run.

For application integration, `evif::wire::Writer` emits records sequentially to
any `Write`; `segment` and `bitstream` copy an exact byte count from a `Read`.
`wire::read` indexes metadata and absolute payload spans using bounded buffers.
`wire::scan` indexes a byte slice without copying payloads and also works without
std. Unknown ancillary records and original scoring JSON remain addressable by
their spans for lossless pass-through. External references remain unresolved.

`frame_hash` accepts strided, little-endian raw rows and preserves float bits.
It returns canonical XXH3-128 and optional SHA-256 checks in `SampleHashes`, computing
both by default in one traversal. `frame_hash_xxh3` computes only XXH3; omitting
SHA-256 does not change media identity. Both hash the same descriptor and samples,
excluding row padding, without color conversion. Media roots use the canonical
XXH3 digests and are not cryptographic commitments to the raw pixels.
`append_record` preserves scoring JSON spelling;
`append_score` serializes a typed envelope. Both require exclusive application
control of the file during validation and append.

## Verification

`just test-container` exercises framing, coverage, identities, exact float bits,
append behavior, malformed/truncated files and u64 offsets with a virtual stream
larger than 4 GiB. This is not a memory benchmark.

`just sequence-carriers` creates real PQ/HLG APNG fixtures and major-1 files with
two symmetric assets. It prints the artifact directory and also writes pack plans
and score JSON. Run `just wire-oracle <that-directory>` to independently parse the
wire, recompute media hashes, inspect PNG timing/CICP/CRCs, and use FFmpeg plus the
system xxHash shared library to check every decoded sample digest with both
algorithms when present. Both tools are required for this check; missing tools
are errors. The oracle retains extracted payloads beside the
fixtures. It is deliberately limited to small RGBA16 PQ/HLG APNG fixtures and
refuses other profiles when decoding is requested.

These checks do not establish CVVDP/HDR-VDP execution, display-accurate HDR playback,
native YCbCr storage, or unresolved gain-map rendering. Those are separate adapters
or profiles; they do not change this framing grammar.
