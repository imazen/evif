# EVIF format, version 0 (draft)

An EVIF file wraps one distorted image for image-quality-metric testing. It
holds the interior bitstream an encoder produced (or nothing, for a synthetic
distortion), a lossless rendition of the pixels our decoder produced from it,
the recipe that made it, and the metric scores computed on that rendition. The
file reports on itself: anyone can check the scores against the pixels.

All integers are little-endian. Draft files (major version 0) may change
without notice.

## 1. Layout

```
signature    8 bytes   8B 45 56 49 46 0D 0A 1A   ("\x8BEVIF\r\n\x1A")
major        u8        0
minor        u8        0
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
| `PIXL` | critical | yes | anywhere | lossless rendition (4) |
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
bitstream    rest      the codec's complete file, byte for byte
```

**Interior bits per pixel** is `8 * (length - 4) / (width * height)`: the
encoded image without the EVIF container. Extracting `bitstream` gives a valid
file of that codec, so any decoder can check it.

## 4. `PIXL`

```
format       4 bytes   "png "
data         rest      a complete PNG
```

The rendition is the raster our decoder produced from `BITS` (or the
synthetic distortion's output when `BITS` is absent). Metrics score this
raster, and viewers show it, so people see exactly what the metrics saw. It
decodes to `HEAD`'s dimensions, layout and sample type, and its digest equals
the `distorted` digest in `DIGS`. PNG color chunks (`gAMA`, `cHRM`, `iCCP`)
are not written; the raster is sRGB per `HEAD`.

## 5. `DIGS`

```
count        u8        1 ..= 8
entries      count x { role u8, digest u64 }
```

Roles: 0 = distorted (the `PIXL` raster, required), 1 = reference (the
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
Readers report bpp from `clam` only when `BITS` is absent, and label it
claimed.

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
