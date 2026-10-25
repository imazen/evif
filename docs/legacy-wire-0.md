# EVIF format, version 0.1 (draft)

An EVIF file wraps one distorted image for image-quality-metric testing. It
holds an optional candidate bitstream, one lossless reference decode of the
scored pixels, the recipe that made it, and the metric scores computed on that
rendition. The
file reports on itself: anyone can check the scores against the pixels.

All integers are little-endian. Draft files (major version 0) may change
without notice.

## 1. Layout

```
signature    8 bytes   8B 45 56 49 46 0D 0A 1A   ("\x8BEVIF\r\n\x1A")
major        u8        0
minor        u8        1
reserved     u16       0
chunks       ...
```

The signature's high first byte and CR LF / SUB bytes catch 7-bit and
newline-converting transfers, as in PNG.

Each chunk:

```
tag          4 bytes   ASCII letters
length       u32       payload bytes
payload      length bytes
```

The case of the tag's first letter says how a reader treats an unknown tag:
upper case is **critical** (a reader that doesn't know it refuses the file);
lower case is **ancillary** (skipped when unknown). Every chunk below is
allowed at most once.

| tag | kind | required | position | payload |
|---|---|---|---|---|
| `HEAD` | critical | yes | first | image header (2) |
| `BITS` | critical | no | anywhere | interior bitstream (3) |
| `BINF` | critical | no | anywhere | recorded candidate codec, size and checksum (3.1) |
| `PNGF` / `IMIJ` | critical | exactly one | anywhere | lossless reference decode (4) |
| `PIXL` | critical | legacy alternative | anywhere | version-0 PNG reference decode (4.1) |
| `DIGS` | critical | yes | anywhere | pixel digests (5) |
| `clam` | ancillary | no | anywhere | claimed size (6) |
| `srce` | ancillary | no | anywhere | source description, JSON (7) |
| `knob` | ancillary | no | anywhere | recipe, JSON (7) |
| `scor` | ancillary | no | anywhere | metric scores, JSON (7) |
| `SEAL` | critical | yes | last | file digest (8) |

A reader refuses the file (corrupt) on a bad signature, a nonzero reserved
field, an unknown major version, a chunk that runs past the end of the file, a
duplicate chunk, a missing required chunk, `HEAD` not first, bytes after
`SEAL`, or a `SEAL` mismatch. It refuses (unsupported) an unknown critical tag.

## 2. `HEAD`

```
width        u32       1 ..= 65535 x 65535 pixels in total under 2^31
height       u32
layout       u8        1 gray, 2 gray+alpha, 3 RGB, 4 RGBA
sample       u8        1 = u8, 2 = u16
transfer     u8        1 = sRGB (the only defined value)
reserved     u8        0
```

Alpha, when present, is straight (not premultiplied).

## 3. `BITS`

```
codec        4 bytes   "jpeg" "webp" "avif" "jxl " "png " "imi " ...
bitstream    rest      the candidate codec's complete encoded bytes
```

**Interior bits per pixel** is `8 * (length - 4) / (width * height)`: the
encoded image without the EVIF container. The codec tag identifies the candidate
compression, including `"evif"` for EVIF-specific coding that is not an external
codec. Such a payload is a candidate codestream, not a nested EVIF container;
its coding version and parameters belong in `knob`. This container does not
define that compression algorithm or require its decoder.

`BITS` is optional for codec-generated and synthetic images alike. Omitting it
does not change the authoritative pixels or their scores. To retain an actual
encoded cost when dropping the bytes, write `BINF`; do not use `clam` for this.

### 3.1. `BINF`

```
codec        4 bytes   candidate codec tag, as BITS
bytes        u64       observed candidate encoded byte count
xxh3         u64       XXH3-64 of the complete candidate encoded bytes
```

The payload is exactly 20 bytes. When `BITS` is present, all three fields must
match it; a mismatch is corrupt, even if `SEAL` is valid. With no `BITS`, the
record is retained provenance, not independently verified encoded bytes.
Candidate bpp precedence is embedded `BITS`, recorded `BINF`, synthetic `clam`,
then unknown. Readers label these cases distinctly. Neither reference-decode
storage nor EVIF overhead enters candidate bpp. XXH3 is not authentication.

## 4. `PNGF` or `IMIJ`: one reference decode

`PNGF` contains a complete PNG, without an additional format prefix. `IMIJ`
contains a complete lossless IMI file. Exactly one reference-decode chunk is
required. Two reference decodes are corrupt, even if their tags or bytes differ.
A normal file does not store duplicate PNG and IMIJ copies.

This raster is the candidate decoder's output, or a synthetic distortion's
output. Metrics score this raster and viewers show it. It must decode to
`HEAD`'s dimensions, layout and sample type, and its pixel digest must equal
`DIGS.distorted`. Lossless means exact reproduction of the **scored raster**;
the candidate that produced that raster may have been lossy.

The current HEAD contract remains sRGB with straight alpha and u8/u16 samples.
PNG color overrides (`gAMA`, `cHRM`, `iCCP`) are not written. IMIJ uses the same
sRGB interpretation. This revision does not add native HDR, gain-map roles,
other primaries, or floating-point samples.

EVIF-specific candidate compression belongs in `BITS`, never in the reference
decode. The container library treats PNGF and IMIJ as opaque bytes. Pixel
verification and format conversion require the corresponding codec plugin;
raw extraction and container checksum verification do not.

### 4.1. Legacy `PIXL`

Readers also accept the version-0 `PIXL` payload: four bytes `"png "` followed
by a complete PNG. It occupies the same single reference-decode slot as PNGF
or IMIJ. New writers emit version 0.1 with PNGF/IMIJ; rewriting a legacy file
preserves its PNG bytes, decoded pixels and metadata, but updates the chunk tag
and file seal. Older readers refuse the new critical tags instead of silently
misinterpreting a new file.

## 5. `DIGS`

```
count        u8        1 ..= 8
entries      count x { role u8, digest u64 }
```

Roles: 0 = distorted (the reference-decode raster, required), 1 = reference (the
source raster the metrics compared against). Each role appears at most once.

Each digest is XXH3-64 (seed 0, default secret) of this 26-byte descriptor
followed by the raster:

```
domain       16 bytes  ASCII "EVIF/pdig/v1" then four zero bytes
width        u32
height       u32
layout       u8        as HEAD
sample       u8        as HEAD
raster       bytes     rows top to bottom, pixels left to right, channels
                       in layout order, each u16 sample little-endian,
                       no row padding
```

The design follows IMI's decoded bitmap checksum (`pSUM`): a hash of the
decoded raster, independent of how it was encoded.

## 6. `clam`

```
claimed      u64       bytes the distortion pretends to cost
```

For a synthetic distortion with no real bitstream, the size it is scored at.
Readers report bpp from `clam` only when both `BITS` and `BINF` are absent,
and label it claimed.

## 7. JSON chunks

UTF-8 JSON objects. Unknown fields are ignored.

`srce`: `{"id": "...", "corpus": "...", "url": "..."}`; every field optional.

`knob`: the recipe.

```json
{
  "family": "zenjpeg-quant",
  "codec": "zenjpeg",
  "codec_version": "0.9.0",
  "codec_commit": "64a92f78",
  "target": "ssim2",
  "params": { "any": "codec-specific knobs" }
}
```

`target` names the metric the search optimized for, if any.

`scor`: `{"scores": [row, ...]}` with each row

```json
{
  "metric": "butteraugli",
  "variant": "3-norm",
  "version": "0.9.2",
  "implementation": "butteraugli@a1b2c3d4",
  "value": 1.83,
  "better": "lower"
}
```

`better` is `"lower"` or `"higher"`. Every score compares the `reference`
raster to the `distorted` raster named in `DIGS`.

## 8. `SEAL`

```
digest       u64       XXH3-64 of every file byte before this chunk's tag
```

`SEAL` is the last chunk; it detects truncation and accidental damage. It is
not authentication.
