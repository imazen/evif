# Checks around `evif trade` runs

Stdlib Python (numpy, Pillow and pyarrow where noted). None of them fits or tunes anything.

- `audit_pilot.py` EVIF_BIN FRACTION RUN_DIR...: every ledger row has its file; a sample of EVIFs is parsed, compared with its
  row bit for bit and passed to `evif verify`.
- `assemble_grid.py` CAS OUT LEDGER.parquet...: pull each finished job's ledger and receipts out of a content-addressed store
  (pyarrow).
- `verify_cas.py` CAS LEDGER.parquet...: every landed object, tar pack member and control receipt of the finished jobs.
- `sample_evif_verify.py` CAS ASSEMBLED EVIF_BIN N_JOBS PER_JOB SCRATCH: sampled EVIFs out of the packs, bit for bit against
  the ledger and `evif verify`.
- `recheck_gate.py` DIR: a second implementation of the registered inversion count (adjacent trade pairs), with the 64 px
  amendment, written from `docs/trade-ladders.md` without importing `trade_gate.py`.
- `compare_whole_vs_parts.py`: a view run whole and as family jobs gives the same ledger rows.
- `small_sanity.py`: each reference's level at rung 0 by view size.
- `oracle_check.py` DIR: third-party decoders (libjpeg-turbo, libavif, lcms) against the pixels an EVIF stores; needs numpy
  and Pillow. A disagreement is a finding, not a vote.
