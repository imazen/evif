"""Build with a narrow compatibility patch to the pinned zenbench 0.1.10.

Only scratch paths, locking and run-ID metadata change. Hashing, timing,
interleaving and statistics are untouched. The original Cargo sources and
lockfile are preserved. The prepared copy lives under the caller's TMPDIR.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent
manifest = root / "Cargo.toml"
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--format-version", "1", "--locked",
    "--manifest-path", str(manifest),
]))
package, = [p for p in metadata["packages"] if p["name"] == "zenbench"]
assert package["version"] == "0.1.10"
assert "075f80b86bb78bbf11984e28a01a73cd3524f5ae" in package["source"]
source = Path(package["manifest_path"]).parent
identity = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()[:16]
prepared = Path(os.environ["TMPDIR"]) / f"evif-zenbench-wasi-{identity}"
if not prepared.exists():
    shutil.copytree(source, prepared, ignore=shutil.ignore_patterns(".git", "target"))
    engine = prepared / "src/engine.rs"
    text = engine.read_text()
    old = 'let dir = std::env::temp_dir().join("zenbench");'
    assert text.count(old) == 1
    text = text.replace(old, '''let dir = PathBuf::from(
        std::env::var_os("TMPDIR").expect("WASI benchmark requires TMPDIR")
    ).join("zenbench");''')
    old = 'Some(std::env::temp_dir().join("zenbench"))'
    assert text.count(old) == 1
    text = text.replace(old, 'None // WASI has no cross-process host lock.')
    engine.write_text(text)
    results = prepared / "src/results.rs"
    text = results.read_text()
    old = '''let pid = std::process::id();
        RunId(format!("{}-{:x}", now.as_secs(), pid))'''
    assert text.count(old) == 1
    results.write_text(text.replace(old, 'RunId(format!("wasm-{}", now.as_nanos()))'))

lock = root / "Cargo.lock"
original_lock = lock.read_bytes()
try:
    subprocess.run([
        "cargo", "build", "--release", "--manifest-path", str(manifest),
        "--target", "wasm32-wasip1", "--features", "blake3/wasm32_simd",
        "--config", f'patch."https://github.com/imazen/zenbench".zenbench.path={json.dumps(str(prepared))}',
    ], check=True)
finally:
    lock.write_bytes(original_lock)
print(f"WASI compatibility source: {prepared}")
