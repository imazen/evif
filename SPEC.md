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
Hashes are exactly 64 lowercase hexadecimal characters. No canonical JSON is
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
    {"duration":{"num":"1001","den":"30000"},"samples":"<SHA-256>"}
  ],
  "storage": {"kind":"embedded","sha256":null}
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
Optional `sha256` covers those exact bytes.

External storage instead has exactly `kind:"external"`, nonempty `uri`, and a
required `sha256` over the external carrier's exact bytes. No payload record
follows. Readers do not automatically fetch URIs. Missing objects mean unavailable
samples, never permission to substitute a similarly named file.

Each segment is independently decodable. Read it once; expand blend/disposal to
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

SHA-256 is a content/integrity identifier, not authentication. There is no required
hash per metadata record. Notes and scores can change independently of media.

Frame digest:

```
SHA256("EVIF/frame/1\0" || width:u32 || height:u32 || channels:u8 || samples:u8
       || tightly-packed little-endian sample bytes)
```

Here `\0` means one zero byte. Rows run top to bottom, pixels left to right, channels
in declared order. Exclude row padding and preserve float bits. Enum codes:
channels gray=1, gray-alpha=2, rgb=3, rgba=4; samples u8=1, u16=2, f16=3, f32=4.

Media digest is SHA-256 over this binary concatenation, integers LE:

```
"EVIF/media/1\0"
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
    frame-sample-digest:32 bytes
```

Exclude local ID, storage compression/segmentation, paths, BITS and scores.
Include samples, interpretation, timing and looping. Identical assets may have
identical media hashes under different IDs. Lossless repacking preserves identity.

MEND is `[{"asset":1,"sha256":"..."}, ...]`: exactly one entry per declared asset,
sorted by numeric asset ID. Require full coverage and match each computed root.
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
| candidate | Media SHA-256 present in this file's MEND |
| reference | Media SHA-256 or null; may identify an external reference |
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
