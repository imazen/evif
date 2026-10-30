# EVIF tasks. Heavy work goes through run-heavy where it exists.

# Set HEAVY to a wrapper for CPU-heavy commands, e.g. `nice -n19 ionice -c3`.
heavy := env_var_or_default('HEAVY', '')
corpus := env_var_or_default('CODEC_CORPUS', env_var('HOME') / 'work/codec-corpus')

default:
    @just --list

adversarial-check:
    cargo fmt -p evif -p evif-probe --check
    {{heavy}} cargo clippy -p evif-probe --features adversarial,possession-svt --all-targets -- -D warnings
    {{heavy}} cargo test -p evif-probe --features adversarial,possession-svt --lib

# Copy only this workspace; no preserved source timestamps can hide a changed file.
adversarial-stage host destination:
    rsync -r --checksum --no-times --exclude .git --exclude .jj --exclude target --exclude .workongoing ./ '{{host}}:{{destination}}/'

adversarial-build:
    {{heavy}} cargo build --release -p evif-probe --features adversarial,possession-svt

adversarial-models directory:
    python3 tools/adversarial_models.py '{{directory}}'

adversarial-golden:
    {{heavy}} cargo test -p evif-probe --features adversarial-models-test,possession-svt --lib adversarial_judges::golden

adversarial-prepare study output commit:
    python3 tools/adversarial_prepare.py '{{study}}' '{{output}}' --build-commit '{{commit}}'

adversarial-run spec output:
    {{heavy}} target/release/evif adversarial '{{spec}}' '{{output}}'

adversarial-summary output:
    python3 tools/adversarial_summary.py '{{output}}'

adversarial-verify output ffmpeg jobs='1':
    {{heavy}} python3 tools/adversarial_verify.py '{{output}}' --ffmpeg '{{ffmpeg}}' --evif target/release/evif --jobs '{{jobs}}'

adversarial-archive run archive models binary binary_sha:
    PYTHONDONTWRITEBYTECODE=1 {{heavy}} python3 tools/adversarial_archive.py '{{run}}' '{{archive}}' --models '{{models}}' --binary '{{binary}}' --binary-sha256 '{{binary_sha}}'

adversarial-local-mirror-check archive receipt mirror output:
    {{heavy}} python3 tools/adversarial_mirror.py '{{archive}}' '{{receipt}}' '{{output}}' --local-mirror '{{mirror}}'

adversarial-r2-mirror-check archive receipt signed_url_file output:
    {{heavy}} python3 tools/adversarial_mirror.py '{{archive}}' '{{receipt}}' '{{output}}' --signed-url-file '{{signed_url_file}}'

# Trade ladders, byte-matched pairs and the DE trade search (feature `trade`).
trade-check:
    cargo fmt -p evif -p evif-probe --check
    {{heavy}} cargo clippy -p evif-probe --features trade --all-targets -- -D warnings
    {{heavy}} cargo test -p evif-probe --features trade --lib
    PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_trade*.py' -v

trade-build:
    {{heavy}} cargo build --release -p evif-probe --features trade

# Specs come from the frozen view list and the stored controls (tools/trade_controls.py).
trade-prepare pilot controls output commit *args:
    python3 tools/trade_prepare.py '{{pilot}}' '{{controls}}' '{{output}}' --build-commit '{{commit}}' {{args}}

trade-run spec output:
    {{heavy}} target/release/evif trade '{{spec}}' '{{output}}'

trade-gate run *args:
    python3 tools/trade_gate.py '{{run}}' {{args}}

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

# Isolated research tool; no additional dependencies in the shipped crates.
hash-cost-build:
    {{heavy}} cargo build --release --manifest-path tools/hash-cost/Cargo.toml

hash-cost output:
    {{heavy}} tools/hash-cost/target/release/evif-hash-cost {{output}}

hash-cost-xxh3 output:
    {{heavy}} tools/hash-cost/target/release/evif-hash-cost {{output}} --xxh3-only

# WASI benchmark; SIMD is an explicit module requirement, not runtime detection.
hash-cost-wasm-build:
    RUSTFLAGS='-C target-feature=+simd128' {{heavy}} python3 tools/hash-cost/build-wasi.py

# Pass a guest-visible output path under the preopened artifact directory.
hash-cost-wasm artifacts output:
    {{heavy}} wasmtime run --dir {{artifacts}}::/artifacts --env TMPDIR=/artifacts --env ZENBENCH_NO_BUSY_GATE=1 tools/hash-cost/target/wasm32-wasip1/release/evif-hash-cost.wasm /artifacts/{{output}} --portable-only

hash-cost-wasm-v8 artifacts output:
    {{heavy}} node tools/hash-cost/run-wasi.mjs tools/hash-cost/target/wasm32-wasip1/release/evif-hash-cost.wasm {{artifacts}} {{output}}

hash-cost-check:
    cargo fmt --manifest-path tools/hash-cost/Cargo.toml --check
    {{heavy}} cargo clippy --release --manifest-path tools/hash-cost/Cargo.toml -- -D warnings
    tools/hash-cost/target/release/evif-hash-cost --check

hash-quality-controls:
    {{heavy}} cargo run --release --manifest-path tools/hash-cost/Cargo.toml --bin quality-controls

# Opt-in real codec possession; serial tests keep codec memory bounded.
possession-check:
    cargo fmt -p evif-probe --check
    {{heavy}} cargo clippy --release -p evif-probe --features possession --all-targets -- -D warnings

possession-test:
    {{heavy}} cargo test --release -p evif-probe --features possession --test possession --test ai_possession --test registry_injection -- --test-threads=1

ai-possession-check:
    cargo fmt -p evif-probe --check
    {{heavy}} cargo check -p evif-probe --features possession-jpegai,possession-coolchic

ai-possession-test:
    {{heavy}} cargo test -p evif-probe --features possession-jpegai,possession-coolchic --test registry_injection --test ai_possession -- --test-threads=1

jpegai-models-test models:
    ZENJPEGAI_MODELS='{{models}}' {{heavy}} cargo test -p evif-probe --features jpegai-models-test --test ai_possession -- --test-threads=1

shared-pages-check:
    {{heavy}} cargo test --release -p evif --test wire

# Native PQ chroma pilot; deliberately files only until HDR rendition I/O exists.
chroma-hdr input output:
    {{heavy}} cargo run --release -p evif-probe --features possession-svt --example chroma_hdr -- {{quote(input)}} {{quote(output)}}

chroma-tools *args:
    {{heavy}} cargo run --release -p evif-probe --example chroma_tools -- {{args}}

chroma-hdr-a3 input output:
    {{heavy}} cargo run --release -p evif-probe --features possession-svt --example chroma_hdr -- a3 {{quote(input)}} {{quote(output)}}

render-union-check:
    {{heavy}} cargo test --release -p evif-probe --test render_union
