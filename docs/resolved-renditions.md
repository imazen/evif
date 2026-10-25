# Resolved SDR and HDR renditions

Design contract, 2026-10-25. This specializes the
[HDR/video proposal](hdr-video-evaluations.md). The implemented
[major-1 wire format](../SPEC.md) uses the same resolved-signal boundary with a
smaller wire schema: cumulative exact durations, implicit straight alpha and
progressive complete frames, and rational luminance scaling. The Python prototype
below remains a design check, not the wire schema or a carrier decoder.

## One rendition is enough

A rendition is one fully reconstructed signal: complete SDR or HDR frames and
their presentation timeline. It does not require a gain map, a paired SDR/HDR
representation, a display simulation, or a particular metric. "Resolved" means
that no gain-map application or frame composition remains necessary to recover
the declared samples. It does not mean that HLG has already been rendered for a
particular display, or that alpha has been flattened.

For a gain-map source, choose the reconstruction being studied and store that
result. Record the reconstruction implementation and settings in provenance.
The original candidate packaging, including its gain map, may remain in BITS.
Omitting BITS must not prevent playback or scoring of the frozen rendition.

Store both SDR and HDR only when both are objects of the experiment. They are
separate identified renditions with an explicit relationship, not two required
copies of every candidate. An SDR preview cannot stand in for the HDR rendition.

Gain maps become required inputs only when the experiment tests adaptive
reconstruction itself across displays. That is a separate rendering-input
profile with base/map assets, interpretation, and dependencies. This resolved
profile refuses such unresolved inputs. Likewise, dynamic tone-mapping metadata
may be retained as provenance, but if its interpretation is required to produce
the intended samples, freeze the chosen reconstruction first or use a future
rendering-input profile. This does not preclude scoring the underlying PQ signal
with an explicitly chosen metric display model.

## Required media description

The prototype schema is `evif-resolved-rendition-draft-1`. Required fields are:

| Field | Meaning |
|---|---|
| `kind` | `still` or `sequence`; a one-frame sequence is not a still |
| `size`, `layout` | Fixed width/height and gray, gray-alpha, RGB or RGBA complete canvases |
| `sample` | u8, u16, f16 or f32; floating-point bit patterns remain exact |
| `alpha` | None or straight; premultiplied inputs require a separately declared preparation |
| `pixel_aspect` | Positive reduced rational; no implicit square-pixel assumption |
| `scan`, `orientation`, `composition` | Progressive, identity orientation, complete frames in this profile |
| `signal` | Primaries, transfer, signal domain, units and sample scaling |
| `plays` | Still: null; sequence: positive total play count, or zero for infinite looping |
| `frames` | Ordered sample hashes and exact rational PTS/durations |

The initial color vocabulary is BT.709, Display-P3/D65 and BT.2020. Gray uses the
declared neutral axis. This profile stores full-range RGB/gray, not native YCbCr.
For integers, encoded/relative values normalize by 255 or 65535, respectively;
alpha always normalizes the same way. For floats, alpha is a unit fraction.
Source 10/12-bit code alignment and limited-range conversion are preparation,
never an implicit `/65535` interpretation of unexpanded source codes. Native
planar samples require another profile; do not claim RGB preserves those planes.

The `signal` object has exactly `primaries`, `transfer`, `domain`, `units` and
`nits_per_unit`. The following combinations are admitted:

| Transfer | Domain | Units | `nits_per_unit` | Meaning |
|---|---|---|---|---|
| sRGB | encoded display-relative | normalized | null | Display EOTF; viewing luminance is an evaluation choice |
| BT.709 | encoded scene-relative | normalized | null | Signal uses the BT.709 OETF; display treatment is explicit preparation |
| PQ | encoded display-absolute | normalized | null | PQ supplies the absolute luminance relation; no invented content peak |
| HLG | encoded scene-relative | normalized | null | HLG signal; system/display treatment belongs to presentation |
| linear | linear scene-relative or display-relative | relative | null | Explicit relative-light domain; no guessed nit scale |
| linear | linear display-absolute | nits | positive decimal string | Float sample times this scale gives luminance; alpha is not scaled |

Absolute linear storage initially requires f16/f32. All transfers may use floats
except that this profile rejects an integer absolute-linear representation until
its scaling contract is specified. The profile does not require a redundant
`hdr` boolean. Transfer and units describe the signal; a flag cannot replace them.
Mastering peak and MaxCLL are optional provenance, not the viewing display and
not substitutes for signal units. ICC/custom primaries and premultiplied alpha
are outside this initial profile, rather than silently treated as one of its
named RGB spaces.

Sequence PTS begin at zero, are contiguous, and have positive durations. Keep
source timecode/original offsets in provenance or evaluation alignment. A frame
hold is a longer duration; repeated frames may share sample hashes. Frame rate
is derived when constant, not a second conflicting source of timing. Interlaced
input requires a recorded conversion to progressive frames. Mixed geometry or
signal interpretation requires separate assets until a switching profile exists.

## Storage and integrity

The storage plan is separate from the media descriptor. An ordered segment list
covers every logical frame exactly once. Each segment identifies its carrier,
object location, first frame and frame count. Segments are independently decodable
and may use PNG for individual integer frames, APNG for integer sequences, or
lossless IMI for integer/float frames or sequences. No duplicate carrier is required.

At decode time, expand APNG blend/disposal into complete canvases and exclude a
nonparticipating poster. Local carrier time starts at zero; frame durations and
order must agree with the manifest. The manifest owns whole-asset looping, so
decoders read each segment once. A single PNG has no internal timeline: the
manifest supplies its duration. If APNG cannot express a duration exactly, use
individual PNG segments or IMI with verified exact timing. Do not round it.

Use bounded segments and a range-readable index for long video. A production
reader must not require all compressed bytes or decoded frames in memory. This
prototype checks logical coverage, not a production index or memory bound.

Per-frame sample hashes and interpretation/timeline form the media identity.
Storage compression, segment boundaries, paths, notes and scores do not.
The prototype uses a domain-separated identity over the descriptor and its
sample hashes; the production sample-hash byte encoding remains to be specified.
Payload SHA-256 is optional for embedded storage, required for external objects
whose exact bytes must be retrieved. No separate hashes are required for every
metadata field. Lossless sample verification requires actual decoding; validating
a hash string or descriptor alone is not evidence of correct pixels.

Metric configuration, viewing conditions, conversion receipts and results remain
evaluation records. They do not redefine the candidate's stored samples or native
timeline. An evaluation can identify and hash a prepared derivative when useful.
Trailing appendable score records and media sealing require their own wire-format
revision; this profile does not change the current SEAL behavior.

## Execution gates

The major-1 reader/writer and bounded metadata index now implement transport.
Before claiming general SDR/HDR playback, verify carrier samples, interpretation
and timing against the media description for every admitted decoder. Add cross-carrier repacking checks that
preserve the media identity, including float bits and transparent RGB. Exercise
VFR, loops, one-frame sequences, segment boundaries and malformed/truncated inputs.
Existing [carrier tests](../evif-probe/tests/sequence_carriers.rs) now exercise
real PQ/HLG APNG through the container and an independent decode oracle. Metric adapters and
display-accurate playback are additional consumers, not requirements for storing
and extracting the rendition.
