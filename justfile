# EVIF tasks. Heavy work goes through run-heavy where it exists.

# Set HEAVY to a wrapper for CPU-heavy commands, e.g. `nice -n19 ionice -c3`.
heavy := env_var_or_default('HEAVY', '')
corpus := env_var_or_default('CODEC_CORPUS', env_var('HOME') / 'work/codec-corpus')

default:
    @just --list

build:
    {{heavy}} cargo build --release

test:
    {{heavy}} cargo test --all-targets
    {{heavy}} cargo test --doc

# The container crate alone, as published: no_std-clean and no default features.
test-container:
    {{heavy}} cargo test -p evif
    {{heavy}} cargo build -p evif --no-default-features

lint:
    cargo fmt --all --check
    {{heavy}} cargo clippy --all-targets -- -D warnings

# A short probe over two corpus images; results land in out/smoke.
smoke: build
    target/release/evif probe --family zenjpeg-expert --target ssim2 --out out/smoke \
        --samples 24 --baseline-step 10 --keep 3 \
        {{corpus}}/gb82/baby-lossless.png {{corpus}}/gb82-sc/gui.png
    target/release/evif verify out/smoke/*/*.evif
    cat out/smoke/summary.md

# Full probe of one family against one target over a directory of PNGs.
probe family target dir out samples='256':
    {{heavy}} target/release/evif probe --family {{family}} --target {{target}} \
        --out {{out}} --samples {{samples}} {{dir}}/*.png

# Stage the static site as Pages deploys it (index.html and logo.svg side by side).
site:
    rm -rf out/site && mkdir -p out/site
    cp site/* out/site/ && cp logo.svg out/site/
    @echo "open out/site/index.html"

# Carrier feasibility for the proposed HDR/sequence contract.
sequence-carriers:
    {{heavy}} cargo test -p evif-probe --test sequence_carriers -- --nocapture

sequence-oracle artifacts:
    {{heavy}} python3 tools/contracts/verify_apng.py {{artifacts}}

# Executable design checks; no codec or metric inference is performed.
contract-check:
    PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/contracts -p 'test_*.py' -v

# Independent framing/media-hash parser and FFmpeg read-back of APNG wire fixtures.
wire-oracle artifacts:
    {{heavy}} python3 tools/wire_oracle.py --decode-apng {{artifacts}}/*.evif
