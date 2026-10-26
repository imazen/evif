# EVIF wire format, major 1, minor 0 (draft)

EVIF stores media assets and evaluations. Reference and candidate use identical
asset, segment and payload encodings; an evaluation assigns their roles. One
resolved SDR or HDR rendition per asset is sufficient. Original codec bytes,
gain maps, paired SDR/HDR copies and metric-specific configuration are not
requirements for replaying the frozen samples.

Implemented by `evif::wire`: slice/streaming readers, sequential writer, appended
scores and decoded-frame hashing. Container validation does not decode carriers
or certify sample hashes. The [legacy major-0 format](docs/legacy-wire-0.md)
remains supported by `Evif` and legacy probe commands. Major 1 is an incompatible
wire layout, not a crate release or a claim that the draft is frozen.

## 1. Framing

All binary integers are unsigned little-endian. No alignment padding, varints,
nested records, implicit lengths, global compression or offset backpatches.

File header, exactly 16 bytes:

```
signature   8 bytes   8B 45 56 49 46 0D 0A 1A
major       u16       1
minor       u16       0
reserved    u32       0
```

Every record has the same 12-byte header:

```
tag         4 bytes   ASCII letters
length      u64       body byte length, excluding this header
body        length bytes
```

Zero length means an empty body, not a sentinel. Check length/offset arithmetic
before allocation. Read exactly the declared length or report truncation. EOF at
a record boundary is valid only after MEND.

Unknown uppercase-first tags are critical: refuse semantic interpretation.
Unknown lowercase-first tags are ancillary: skip or preserve their exact bytes.
Repeated ancillary records are permitted. A rewriter should preserve them. It
must not claim old evaluations still apply after changing their identified media.

Unknown major versions are unsupported. Higher minor versions retain this grammar
and may add ancillary records. Changing known semantics requires a new critical
record or profile/schema; an ignored field must never alter pixel interpretation.
Reserved flags must be zero. Unsupported files can still be copied as opaque bytes.

## 2. Records and order

| Tag | Body | Rule |
|---|---|---|
| ASST | Asset JSON | Unique nonzero u32 local asset ID, declared before use |
| SEGM | Segment JSON | Next contiguous frame range for a declared asset |
| PNGF | Complete PNG/APNG file | Immediately follows its embedded SEGM |
| IMIJ | Complete lossless IMI file | Immediately follows its embedded SEGM |
| BITS | Asset u32, codec FourCC, original encoded bytes | Optional, at most one per asset |
| MEND | Array of media roots | Exactly once; closes immutable media |
| scor | Evaluation JSON | Repeatable, only after MEND |
| other lowercase | Opaque bytes | Before/after MEND, except inside a SEGM/payload pair |

```
header
ASST reference-descriptor
ASST candidate-descriptor
SEGM reference-segment-0
PNGF reference-apng-bytes
SEGM candidate-segment-0
IMIJ candidate-imi-bytes
BITS optional-candidate-encode
MEND both-media-roots
scor first-evaluation
scor another-evaluation
```

Assets and segments may interleave. Segments cover each asset's frames exactly
once, in order from ordinal zero. No media records follow MEND. Appending scores
does not rewrite MEND or change media identity. There is no global SEAL.

## 3. JSON rules

Known JSON bodies are UTF-8 without BOM. Duplicate keys at any depth, nonfinite
numbers, invalid Unicode and trailing non-whitespace bytes are errors. Core
objects admit exactly the fields below. Adapter-owned `data` is an arbitrary
JSON object. Nullable fields may be omitted and mean null.

All u64 quantities are decimal **strings**, digits only, without leading zero
except `"0"`, in range 0..18446744073709551615. u32 quantities are JSON integers.
A rational is `{"num":"1001","den":"30000"}`: positive reduced u64 values.
Every hash uses the same object, including sample, payload, media and score-reference hashes:

```json
{"algorithm":"xxh3-128","digest":"0123456789abcdeffedcba9876543210"}
```

`algorithm` is a case-sensitive ASCII identifier, 1..64 bytes, matching
`[a-z][a-z0-9-]*`. `digest` is a nonempty, even-length lowercase hexadecimal
string. There is no fixed digest-width ceiling in the wire grammar; readers
apply their enclosing JSON resource limits before allocation. Algorithm IDs
identify the complete algorithm, output width and fixed parameters; a new mode,
seed convention or XOF output width requires a distinct ID, not inferred defaults.

Registered algorithms:

| ID | Digest bytes | Parameters and byte order |
|---|---:|---|
| `xxh3-128` | 16 | Standard default secret, zero seed (`XXH3_128bits`); high 64-bit half then low half, each big-endian |
| `sha256` | 32 | Standard initialization and digest byte order |

Known IDs require their exact lengths. Unknown IDs are retained verbatim with
all digest bytes, without claiming verification. Hash equality includes both
algorithm and digest. Optional fields below are nullable only where stated;
`checks` is an array and may be omitted, but may not be null.

No canonical JSON is
required; whitespace/key order do not affect media identity. Exact pass-through
retains original record bytes, since reserialization can change numeric spelling.

## 4. ASST: resolved-rgb/1

| Field | Meaning |
|---|---|
| id | Nonzero u32 local address; not part of media identity |
| profile | Exactly `resolved-rgb/1` |
| kind | `still` or `sequence` |
| width, height | Positive u32, fixed for every frame |
| channels | `gray`, `gray-alpha`, `rgb`, `rgba` |
| samples | `u8`, `u16`, `f16`, `f32` |
| primaries | `bt709`, `display-p3-d65`, `bt2020` |
| transfer | `srgb`, `bt709`, `pq`, `hlg`, `linear` |
| domain | `display-relative`, `scene-relative`, `display-absolute` |
| nits_per_unit | Positive rational or null |
| pixel_aspect | Positive rational: pixel width / pixel height |
| frame_count | Positive u64 string; exactly `"1"` for a still |
| plays | Null for still; sequence total plays as u64 string, zero = infinite |

This profile means progressive, orientation-applied, complete frames with straight
alpha. Integers are full-range codes normalized by 255/65535; float alpha is a unit
fraction. Preserve transparent RGB. Gray follows the declared neutral axis. Native
YCbCr, interlacing, unresolved gain maps, premultiplied alpha and custom/ICC color
interpretations require another profile or explicit preparation; never relabel them.

| Transfer | Domain | Interpretation |
|---|---|---|
| srgb | display-relative | Normalized signal; evaluation selects display luminance |
| bt709 | scene-relative | Normalized BT.709 OETF signal; evaluation selects display treatment |
| pq | display-absolute | Normalized PQ code, with its intrinsic luminance relation |
| hlg | scene-relative | Normalized HLG signal; evaluation selects system/display treatment |
| linear | scene-relative or display-relative | Relative light, no implicit nit scale |
| linear | display-absolute | f16/f32 only; sample times nits_per_unit gives nits |

`nits_per_unit` is required only for absolute linear, null otherwise. It never
scales alpha. Float storage preserves IEEE bits, including signed zero and NaN
payloads; metrics may reject nonfinite samples but storage must not repair them.
Mastering peak is not the experimental display. No redundant HDR flag is required.

## 5. SEGM and rendition payloads

```
{
  "asset": 2,
  "first_frame": "0",
  "carrier": "apng",
  "frames": [
    {"duration":{"num":"1001","den":"30000"},
     "samples":{
       "canonical":{"algorithm":"xxh3-128","digest":"<32 lowercase hex digits>"},
       "checks":[{"algorithm":"sha256","digest":"<64 lowercase hex digits>"}]
     }}
  ],
  "storage": {"kind":"embedded","hash":null}
}
```

Each nonempty frame entry has exactly `duration` and `samples`. Still duration is
null. Sequence durations are positive reduced rationals in seconds. PTS is their
exact cumulative sum from zero. No separate FPS or rounded timestamps override
it. Holds have long durations; repeats may reuse sample hashes. Source timecode
and experiment alignment belong in provenance/evaluations. Gaps/overlaps need a
separate profile.

Carriers: `png` = one integer frame; `apng` = integer frames; `imi` = integer or
float frames. Embedded storage must immediately precede its matching PNGF/IMIJ
record. The body is the carrier file without another wrapper, and must be nonempty.
Optional `hash` covers those exact bytes.

IMI page bundles can share lossless reference prediction across renditions:

```json
{"kind":"embedded-pages","hash":null,"first_page":"0","total_pages":"2"}
{"kind":"shared-pages","source_asset":1,"source_first_frame":"0","first_page":"1"}
```

`embedded-pages` immediately precedes one self-contained IMIJ **pages** file.
`total_pages` is its exact page count, including prediction helper pages.
`first_page` selects consecutive pages, one per SEGM frame. `shared-pages`
selects another window from an earlier `embedded-pages` segment, addressed by
its asset ID and first frame ordinal; it has no following payload. References
must be direct and backward: no chains, cycles, forward references or external
objects. Both segments have identical geometry, sample format, color, nit scale
and pixel aspect. Their kind, frame count and playback count may differ.
Page indices and counts are u64 strings; checked window ends must not exceed
`total_pages`. Readers limit the full carrier page count, not just selected pages.

For example, page 0 stores the pristine reference and page 1 stores a distortion
using IMI's lossless prediction from page 0. Both assets address the same carrier
bytes. The codec chooses independent coding when prediction does not save space.
For video, a bundle can hold reference frames followed by corresponding distorted
frames; each SEGM selects its own contiguous window. All prediction dependencies
stay inside that carrier. Helper pages are not extra presentation frames.
A sample verifier must decode the selected pages with their declared IMI
references, validate the exact carrier page count and signal, and check each
selected frame hash. SEGM alone supplies page-bundle presentation durations;
IMI page ordering is not a playback clock. Changing storage or prediction choices
does not change media identity. A streaming reader may need to retain or revisit
the shared carrier; use bounded bundles to bound that cost.

External storage instead has exactly `kind:"external"`, nonempty `uri`, and a
required `hash` over the external carrier's exact bytes. No payload record
follows. Readers do not automatically fetch URIs. Missing objects mean unavailable
samples, never permission to substitute a similarly named file.

The current verified reader/writer supports `sha256` for embedded payload checks.
An unsupported embedded-payload algorithm returns an explicit unsupported error,
not success or a mismatch. External payload hashes can use unknown algorithms
because the container reader does not fetch or verify those objects. The hash
record itself remains parseable independently of algorithm support.

Each segment resolves to a self-contained carrier (possibly shared as above).
For ordinary embedded/external carriers, expand blend/disposal to
complete canvases and exclude a nonparticipating APNG poster. Decoded frame count,
geometry, layout/type, interpretation, hashes and durations must match ASST/SEGM.
Carrier time starts locally at zero; ASST owns whole-asset looping. Single PNGs
have no internal duration; SEGM supplies it. APNG duration numerator/denominator
must fit u16 exactly. Otherwise use individual PNG segments or exact IMI timing.

Explicit carrier color signaling must agree with ASST. Absent signaling uses
ASST. Conflicting ICC/CICP/other declarations require resolution during preparation,
not a silent replay override. Codec verification, not container parsing, checks this.

Bounded segments support long clips and range reads. A sequential scan yields an
offset index. No second authoritative offset table needs reconciliation. A future
optional index may accelerate seeking, but must match the actual records.

## 6. Sample and media hashes

The `samples` object requires a `canonical` hash record and permits `checks`,
an array of additional hash records (default empty). XXH3-128 is the default
canonical algorithm; writers should add a SHA-256 check. Every entry covers the
same canonical frame bytes. No algorithm ID may occur twice, including between
`canonical` and `checks`.

Other canonical algorithms, including unknown algorithms and longer digests,
are preserved. A transport reader can bind them into media identity without
knowing how to compute them. It does not thereby verify decoded samples.
Sample verifiers must report which algorithms were checked and which are
unsupported; every check they perform must match. They must not label an
unsupported algorithm as verified or quietly substitute another algorithm.

Canonical frame bytes for every sample-hash entry:

```
"EVIF/frame/1\0" || width:u32 || height:u32 || channels:u8 || samples:u8
                || tightly-packed little-endian sample bytes
```

XXH3-128 uses the standard default secret and zero seed (`XXH3_128bits`). SHA-256
uses its standard initialization. Here `\0` means one zero byte. Rows run top to bottom, pixels left to right, channels
in declared order. Exclude row padding and preserve float bits. Enum codes:
channels gray=1, gray-alpha=2, rgb=3, rgba=4; samples u8=1, u16=2, f16=3, f32=4.

Media digest is SHA-256 over this binary concatenation, integers LE:

```
"EVIF/media/4\0"
profile-code:u8 = 1
kind:u8                   still=1, sequence=2
width:u32 height:u32
channels:u8 samples:u8
primaries:u8              bt709=1, display-p3-d65=2, bt2020=3
transfer:u8               srgb=1, bt709=2, pq=3, hlg=4, linear=5
domain:u8                 display-relative=1, scene-relative=2, display-absolute=3
scale-present:u8          0/1; if 1, num:u64 den:u64
pixel-aspect-num:u64 pixel-aspect-den:u64
frame-count:u64
plays-present:u8          0/1; if 1, plays:u64
for each frame in presentation order:
    duration-present:u8   0/1; if 1, num:u64 den:u64
    algorithm-length:u64
    algorithm:bytes      ASCII ID, exactly algorithm-length bytes
    digest-length:u64
    digest:bytes         decoded hex, exactly digest-length bytes
```

Length-prefix the algorithm and digest independently. Digest bytes follow the
hex string from left to right; there is no integer-endian reversal. Thus an
XXH3-128 value contributes its high half first, unlike the earlier draft's
little-endian u128. These rules also bind unknown algorithms and longer digests
without truncation or ambiguous concatenation.

Exclude all optional sample `checks`, local ID, storage compression/segmentation,
paths, BITS and scores. Include canonical sample digests, interpretation, timing
and looping. Adding, removing or reordering optional checks does not change identity.
Identical assets may have
identical media hashes under different IDs. Lossless repacking preserves identity.

The current media root uses SHA-256 over the canonical hash records. With the
default XXH3-128 canonical algorithm, it is not a cryptographic
commitment to the raw samples: a sample XXH3 collision also survives this root.
Optional frame SHA-256 checks independently detect differences when verified.
External payload hashes still address the exact external bytes. None of
these hashes authenticates a producer. Metadata records have no required hash;
notes and scores can change independently of media.

This unreleased draft replaces algorithm-specific fields and bare hash strings
with tagged records, and changes the media-hash domain to revision 4. Existing
XXH3-128/SHA-256 digests can be wrapped without rehashing samples, but media roots
and score references must be recomputed. XXH3-64 cannot be converted to XXH3-128;
that older draft still requires decoded samples. The header remains wire major 1,
minor 0. Older schemas are refused rather than silently reinterpreted.

MEND is `[{"asset":1,"hash":{"algorithm":"sha256","digest":"..."}}, ...]`:
exactly one entry per asset, sorted by numeric asset ID. Require full coverage and
match each computed root, including its algorithm. The current implementation
supports `sha256` media roots and reports other root algorithms as unsupported.
The record shape accommodates future root algorithms without widening a field.
MEND closes the media description; it does not prove the carrier decodes to the
claimed samples.

## 7. Original encoded bytes and provenance

BITS is `asset:u32 || codec:4 bytes || encoded bytes` for either reference or
candidate. `evif` means EVIF-specific coding, not a nested EVIF container. Codec
identifiers are never filesystem paths. Encoded bytes must be nonempty. BITS is
optional and cannot substitute for a rendition.

Recorded size/checksum after byte omission, encoder/decoder versions, seeds,
source labels and recipes may use ancillary application-owned JSON (`meta`, for
example). Label recorded size separately from measured embedded BITS size. Never
count rendition storage as candidate codec cost.

## 8. Appendable scoring metadata

| Field | Meaning |
|---|---|
| schema | u32, currently 1 |
| id | Unique nonempty string, at most 256 UTF-8 bytes |
| supersedes | Null or ID of an earlier score record |
| candidate | Hash object matching a media root in this file's MEND, including algorithm |
| reference | Hash object or null; may identify an external reference using an unknown algorithm |
| adapter | Nonempty adapter/schema identifier |
| data | Adapter-owned object: preparation, viewing conditions, implementation/model, results, diagnostics |

The container validates the envelope; adapters validate their configurations and
reproduction requirements. An external reference hash alone is not self-contained
reproduction. The same media bytes may serve either role in different evaluations.

There is no implicit last-wins rule. Corrections explicitly supersede an earlier
record; prior records remain readable. An append is complete when its declared
body is fully present and valid. A partial trailing header/body is an error;
strict readers never silently drop it. Earlier complete record boundaries remain
valid recovery points. The API does not truncate or repair files automatically.
Serialize concurrent writers. Flush is not a filesystem durability guarantee.

No mandatory score checksum or whole-file SEAL. An edit that preserves valid score
JSON need not be detected. External hashing or a future signature record may cover
a specific file snapshot without redefining media identity.

## 9. Parser and resource limits

1. Validate the 16-byte file header.
2. Read tag + u64 length; check limits and overflow.
3. Parse bounded known JSON, consume payloads in bounded blocks, or skip ancillary
   bytes. Refuse unknown critical records.
4. Track each asset's next frame and incremental media hash. Require embedded
   SEGM's payload next. Check optional payload hashes while streaming.
5. At MEND check coverage/roots. Thereafter accept scoring and ancillary records.
6. Accept EOF only at a complete record boundary after MEND.

The library exposes bounds on assets/records, per-record/total JSON bytes, and
per-segment/total indexed frames. These are resource policies, not wire maxima.
Metadata is indexed; payloads are not buffered whole. The slice reader records
borrowed locations; std reader/writer transfers payloads in fixed-size blocks.
Codec decoders impose their own pixel/resource limits.
