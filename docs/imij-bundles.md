# Lossless reference prediction with IMIJ

The major-1 wire format supports one self-contained IMIJ page bundle shared by
several assets. Store the pristine reference first; encode each distorted image
with an explicit earlier reference page. IMIJ chooses prediction only when it
saves bytes. It preserves the distorted samples exactly, including HDR float bits.

For a reference/candidate pair EVIF writes **two `SEGM` records and one `IMIJ`
payload**. The first segment owns the self-contained bundle; the second selects
its distorted page through `shared-pages` and has no following payload. All IMI
prediction dependencies are pages inside that one bundle. An external EVIF
carrier URI is a storage location, never an IMI prediction reference.

`embedded-pages` owns the payload and selects its reference window.
`shared-pages` addresses that earlier owner and selects a candidate window.
Prediction dependencies are internal to IMIJ. The EVIF parser follows one backward
address, never a chain of assets. A decoder must read the selected pages with
their declared dependencies and verify the selected sample hashes.

The IMIJ-enabled companion provides:

```
evif-private bundle plan.json experiment.evif
```

Its plan contains `assets`, each with a complete major-1 `asset` descriptor and a
`frames` array. Each frame has a raw sample `path` (relative to the plan),
`stride_bytes`, nullable `duration`, and nullable `reference_page`. Raw files use
little-endian sample slots in the declared channel order; padding is ignored.
`reference_page` is the absolute earlier page ordinal across the flattened assets.
Optional `provenance` is an object retained in an ancillary `note` record.

For two stills, reference frame `reference_page` is null and candidate frame
`reference_page` is 0. For two aligned N-frame sequences, store the N reference
frames first, then N candidate frames predicting from pages 0 through N−1.
EVIF durations define playback; the IMIJ page collection is not an animation.
The plan assigns no implicit reference/candidate roles: appended scores do that.

`evif-private unbundle INPUT.evif NEW_DIRECTORY` decodes selected pages, verifies
every declared supported sample hash, and writes raw little-endian frames plus a
new plan retaining the exact presentation durations. Unsupported hash algorithms
are reported as unsupported. It refuses an existing output directory.

The companion checks every encoded page against the input samples before writing
the EVIF file. Inputs must share geometry, sample type and signal interpretation.
The current command bounds a bundle to 4096 pages and 256 MiB of raw inputs; use
bounded segments for larger clips. It refuses an existing output path.

This command is separate from legacy `render --rendition imij`: legacy containers
still store independent candidate renditions. The public binary can package and
index shared carriers with `wire pack`, but does not include an IMIJ decoder.
Shared storage bytes are not an experimental codec's bitrate; retain that codec's
actual cost separately when comparing compression efficiency.
