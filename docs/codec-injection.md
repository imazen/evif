# Add a codec without changing the probe engine

The probe binary receives a `Registry`. A codec adapter implements
`evif_probe::family::Family` and registers a constructor in the binary:

```rust
let mut registry = evif_probe::registry::Registry::builtin();
registry.add_family("my-codec", || Box::new(MyCodec::new()));
evif_probe::cli::main(&registry);
```

`MyCodec` supplies `name`, `codec`, `sample`, `baseline`, and `encode`. The
constructor can capture model paths or other dependency configuration. A
replacement registration with the same name overrides a built-in family.
The existing CLI then accepts `--family my-codec` for both `probe` and `render`;
the search and EVIF writer need no codec-specific match arms. See
[`tests/registry_injection.rs`](../evif-probe/tests/registry_injection.rs)
for an executable minimal family.

`encode` must return the **decoded pixels from its encoded bytes**, not its
input pixels or an encoder-side approximation. Put native bytes in `bits` with
a four-byte syntax identifier, and leave `claimed_bytes` empty. If native bytes
cannot be retained, report the measured size separately through the existing
omission path. `baseline(quality)` must describe this same codec; do not compare
a candidate against another codec's quality curve. Recipes should reject
unknown or unsupported settings and record exact implementation and model
revisions needed for replay. A codec that needs external weights should resolve
them in its constructor or `encode` and report a missing source explicitly.

For a built-in family, add a feature-gated module in `evif-probe/src/family.rs`,
one registration in `Registry::builtin`, a Cargo feature/dependency and a pin
in `src/pins.rs`. For a companion binary, add the dependency there and register
the adapter; no change to `evif-probe` is needed.

The major-1 wire format is independent of the codec registry. `ASST` and `SEGM`
describe decoded media for either role; `PNGF`/`IMIJ` carry a lossless rendition;
optional `BITS` carries the codec's original stream. An evaluation assigns the
reference and candidate roles. See [SPEC.md](../SPEC.md#7-original-encoded-bytes-and-provenance).
