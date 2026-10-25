//! The `evif` command line, parameterized by a [`Registry`] so a companion
//! binary with extra families or judges gets the same interface.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use evif::{Bpp, Evif};
use serde_json::Value;

use crate::family::Family;
use crate::image::Rgb8Image;
use crate::judge::{self, Judge};
use crate::registry::Registry;
use crate::rng::Rng;
use crate::search::{self, Baseline, Sample};
use crate::{Res, write};

const USAGE: &str = "\
evif: stress-test image quality metrics with a self-reporting container

  evif probe   --family F --target KEY --out DIR [--judges a,b,c] [--samples N]
               [--keep K] [--seed S] [--baseline-step Q] IMAGE.png...
  evif inspect FILE.evif...        print header, bpp, recipe and scores
  evif verify  FILE.evif...        check SEAL, PIXL decode, pixel digest
  evif extract FILE.evif DIR       write the interior bitstream and PNG
  evif render  --family F --out DIR (--qualities 5,10,... | --params FILE.jsonl)
               IMAGE.png...        encode with a family, write decoded PNGs and
                                   DIR/render.tsv (image, tag, bytes, bpp, png, params)
  evif list                        families, judges and score keys";

/// Run the CLI and exit with its status.
pub fn main(registry: &Registry) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(registry, &args) {
        Ok(()) => std::process::exit(0),
        Err(e) if e == USAGE => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("evif: {e}");
            std::process::exit(1);
        }
    }
}

pub fn run(registry: &Registry, args: &[String]) -> Res<()> {
    match args.first().map(String::as_str) {
        Some("probe") => probe(registry, &args[1..]),
        Some("inspect") => inspect(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("extract") => extract(&args[1..]),
        Some("list") => list(registry),
        Some("render") => render(registry, &args[1..]),
        _ => Err(USAGE.to_string()),
    }
}

fn list(registry: &Registry) -> Res<()> {
    println!("families: {}", registry.family_names().join(", "));
    for name in registry.judge_names() {
        let j = registry.judge(name).expect("listed judge exists");
        println!("judge {name}: keys {}", j.keys().join(", "));
    }
    Ok(())
}

struct ProbeArgs {
    family: String,
    target: String,
    out: PathBuf,
    judges: Vec<String>,
    samples: usize,
    keep: usize,
    seed: u64,
    baseline_step: f64,
    images: Vec<PathBuf>,
}

fn parse_probe(registry: &Registry, args: &[String]) -> Res<ProbeArgs> {
    let mut a = ProbeArgs {
        family: String::new(),
        target: String::new(),
        out: PathBuf::new(),
        judges: registry
            .judge_names()
            .iter()
            .map(|s| s.to_string())
            .collect(),
        samples: 64,
        keep: 4,
        seed: 1,
        baseline_step: 5.0,
        images: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--family" => a.family = value()?,
            "--target" => a.target = value()?,
            "--out" => a.out = PathBuf::from(value()?),
            "--judges" => a.judges = value()?.split(',').map(str::to_string).collect(),
            "--samples" => a.samples = value()?.parse().map_err(|e| format!("--samples: {e}"))?,
            "--keep" => a.keep = value()?.parse().map_err(|e| format!("--keep: {e}"))?,
            "--seed" => a.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--baseline-step" => {
                a.baseline_step = value()?
                    .parse()
                    .map_err(|e| format!("--baseline-step: {e}"))?;
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => a.images.push(PathBuf::from(arg)),
        }
    }
    if a.family.is_empty() || a.target.is_empty() || a.out.as_os_str().is_empty() {
        return Err("--family, --target and --out are required".into());
    }
    if a.images.is_empty() {
        return Err("no images given".into());
    }
    if a.baseline_step.is_nan() || a.baseline_step <= 0.0 {
        return Err("--baseline-step must be positive".into());
    }
    Ok(a)
}

/// One TSV row. Baseline rows carry the sweep's raw scores and no gains.
struct Row<'a> {
    image: &'a str,
    family: &'a str,
    kind: &'a str,
    index: usize,
    bpp: Option<f64>,
    scores: &'a BTreeMap<String, f64>,
    gain: &'a BTreeMap<String, f64>,
    disagreement: Option<f64>,
    params: &'a Value,
}

impl Row<'_> {
    fn tsv(&self, keys: &[String]) -> String {
        let mut cols = vec![
            self.image.to_string(),
            self.family.to_string(),
            self.kind.to_string(),
            self.index.to_string(),
            self.bpp.map_or(String::new(), |b| format!("{b:.5}")),
        ];
        cols.extend(keys.iter().map(|k| {
            self.scores
                .get(k)
                .map_or(String::new(), |v| format!("{v:.5}"))
        }));
        cols.extend(keys.iter().map(|k| {
            self.gain
                .get(k)
                .map_or(String::new(), |v| format!("{v:.5}"))
        }));
        cols.push(
            self.disagreement
                .map_or(String::new(), |d| format!("{d:.5}")),
        );
        cols.push(self.params.to_string());
        cols.join("\t")
    }
}

fn probe(registry: &Registry, args: &[String]) -> Res<()> {
    let a = parse_probe(registry, args)?;
    let fam: Box<dyn Family> = registry
        .family(&a.family)
        .ok_or_else(|| format!("unknown family {}", a.family))?;
    let judges: Vec<Box<dyn Judge>> = a
        .judges
        .iter()
        .map(|n| {
            registry
                .judge(n)
                .ok_or_else(|| format!("unknown judge {n}"))
        })
        .collect::<Res<_>>()?;
    let keys = judge::keys(&judges);
    if !keys.iter().any(|k| k == &a.target) {
        return Err(format!(
            "target {} is not a key of the chosen judges ({})",
            a.target,
            keys.join(", ")
        ));
    }
    fs::create_dir_all(&a.out).map_err(|e| format!("{}: {e}", a.out.display()))?;

    let ledger_path = a.out.join("ledger.tsv");
    let fresh = !ledger_path.exists();
    let mut ledger = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&ledger_path)
        .map_err(|e| format!("{}: {e}", ledger_path.display()))?;
    if fresh {
        let mut head = vec!["image", "family", "kind", "index", "bpp"];
        head.extend(keys.iter().map(String::as_str));
        let gains: Vec<String> = keys.iter().map(|k| format!("gain:{k}")).collect();
        head.extend(gains.iter().map(String::as_str));
        head.extend(["disagreement", "params"]);
        writeln!(ledger, "{}", head.join("\t")).map_err(|e| e.to_string())?;
    }

    let qualities: Vec<f64> = {
        let mut q = 5.0;
        let mut v = Vec::new();
        while q <= 95.0 + 1e-9 {
            v.push(q);
            q += a.baseline_step;
        }
        v
    };

    let mut summary: Vec<(String, Vec<Sample>)> = Vec::new();
    for img_path in &a.images {
        let id = img_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let t0 = Instant::now();
        let bytes = fs::read(img_path).map_err(|e| format!("{}: {e}", img_path.display()))?;
        let reference = Rgb8Image::from_png(&bytes).map_err(|e| format!("{id}: {e}"))?;
        eprintln!(
            "{id}: {}x{}, baseline sweep ({} points)",
            reference.width,
            reference.height,
            qualities.len()
        );

        let baseline = Baseline::build(fam.as_ref(), &judges, &reference, &qualities)?;
        if let Some(b) = &baseline {
            for (i, (bpp, scores)) in b.points().iter().enumerate() {
                let params = fam.baseline(qualities[i]).unwrap_or_default();
                let no_gain = BTreeMap::new();
                let row = Row {
                    image: &id,
                    family: fam.name(),
                    kind: "baseline",
                    index: i,
                    bpp: Some(*bpp),
                    scores,
                    gain: &no_gain,
                    disagreement: None,
                    params: &params,
                }
                .tsv(&keys);
                writeln!(ledger, "{row}").map_err(|e| e.to_string())?;
            }
        }

        let mut rng = Rng::for_key(a.seed, &id);
        let setup = search::Setup {
            family: fam.as_ref(),
            judges: &judges,
            baseline: baseline.as_ref(),
            reference: &reference,
            target: &a.target,
        };
        let mut skipped = 0usize;
        let results = setup.run(
            &mut rng,
            a.samples,
            |i, _, _| {
                if (i + 1) % 8 == 0 || i + 1 == a.samples {
                    eprintln!("{id}: {}/{} samples", i + 1, a.samples);
                }
            },
            |_, why| {
                skipped += 1;
                if skipped <= 3 {
                    eprintln!("{id}: skipped an unencodable sample: {why}");
                }
            },
        )?;
        if skipped > 0 {
            eprintln!("{id}: {skipped} samples skipped as unencodable");
        }
        for (i, (s, _)) in results.iter().enumerate() {
            let row = Row {
                image: &id,
                family: fam.name(),
                kind: "sample",
                index: i,
                bpp: s.bpp,
                scores: &search::by_key(&s.scores),
                gain: &s.gain,
                disagreement: s.disagreement,
                params: &s.params,
            }
            .tsv(&keys);
            writeln!(ledger, "{row}").map_err(|e| e.to_string())?;
        }

        // Keep the strongest disagreements. Without a baseline every
        // disagreement is None and the order is the sampling order.
        let mut order: Vec<usize> = (0..results.len()).collect();
        order.sort_by(|&x, &y| {
            let dx = results[x].0.disagreement.unwrap_or(f64::NEG_INFINITY);
            let dy = results[y].0.disagreement.unwrap_or(f64::NEG_INFINITY);
            dy.total_cmp(&dx)
        });
        let dir = a.out.join(&id);
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut kept = Vec::new();
        for &i in order.iter().take(a.keep) {
            let (s, enc) = &results[i];
            let file = write::build(fam.as_ref(), &reference, &id, Some(&a.target), s, enc)?;
            let bytes = file.to_bytes().map_err(|e| e.to_string())?;
            let name = format!("{}-{:03}.evif", fam.name(), i);
            fs::write(dir.join(&name), &bytes).map_err(|e| format!("{name}: {e}"))?;
            kept.push(s.clone());
        }
        eprintln!(
            "{id}: kept {} of {} in {:.1}s",
            kept.len(),
            results.len(),
            t0.elapsed().as_secs_f64()
        );
        summary.push((id, kept));
    }

    let mut md = String::new();
    md.push_str(&format!(
        "# probe: family {}, target {}, judges {}\n\n",
        fam.name(),
        a.target,
        a.judges.join(", ")
    ));
    md.push_str("Gain per key is log2(equivalent bpp / actual bpp): the bpp an honest encode would need to reach this score, over the bytes actually spent. +1 means the metric rates the encode like an honest file twice its size. Disagreement is the target's gain minus the mean of the others'.\n\n");
    md.push_str("| image | file | bpp | disagreement |");
    for k in &keys {
        md.push_str(&format!(" {k} | gain |"));
    }
    md.push_str("\n|---|---|---|---|");
    for _ in &keys {
        md.push_str("---|---|");
    }
    md.push('\n');
    for (id, kept) in &summary {
        for (n, s) in kept.iter().enumerate() {
            let by = search::by_key(&s.scores);
            md.push_str(&format!(
                "| {id} | #{n} | {} | {} |",
                s.bpp.map_or("-".into(), |b| format!("{b:.3}")),
                s.disagreement.map_or("-".into(), |d| format!("{d:+.3}"))
            ));
            for k in &keys {
                md.push_str(&format!(
                    " {} | {} |",
                    by.get(k).map_or("-".into(), |v| format!("{v:.2}")),
                    s.gain.get(k).map_or("-".into(), |g| format!("{g:+.3}"))
                ));
            }
            md.push('\n');
        }
    }
    fs::write(a.out.join("summary.md"), md).map_err(|e| e.to_string())?;
    eprintln!(
        "wrote {} and {}",
        ledger_path.display(),
        a.out.join("summary.md").display()
    );
    Ok(())
}

fn read(path: &Path) -> Res<Evif> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Evif::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn inspect(args: &[String]) -> Res<()> {
    for p in args {
        let f = read(Path::new(p))?;
        let h = f.header;
        println!("{p}");
        println!("  {}x{} {:?} {:?}", h.width, h.height, h.layout, h.sample);
        match &f.bits {
            Some(b) => println!(
                "  bitstream: {} {} bytes",
                String::from_utf8_lossy(&b.codec).trim(),
                b.data.len()
            ),
            None => println!("  bitstream: none"),
        }
        match f.bits_per_pixel() {
            Some(Bpp::Interior(b)) => println!("  interior bpp: {b:.4}"),
            Some(Bpp::Claimed(b)) => println!("  claimed bpp: {b:.4}"),
            None => println!("  bpp: unknown"),
        }
        if let Some(s) = &f.source {
            println!("  source: {}", s.id.as_deref().unwrap_or("?"));
        }
        if let Some(r) = &f.recipe {
            println!(
                "  recipe: {} via {} {} ({})",
                r.family,
                r.codec.as_deref().unwrap_or("-"),
                r.codec_version.as_deref().unwrap_or(""),
                r.codec_commit
                    .as_deref()
                    .map(|c| &c[..c.len().min(8)])
                    .unwrap_or("")
            );
            if let Some(t) = &r.target {
                println!("  target: {t}");
            }
            println!("  params: {}", r.params);
        }
        if let Some(s) = &f.scores {
            for sc in &s.scores {
                println!(
                    "  {:<22} {:>10.4}  ({:?}, {})",
                    sc.key(),
                    sc.value,
                    sc.better,
                    sc.implementation.as_deref().unwrap_or("")
                );
            }
            for (k, v) in &s.extra {
                println!("  {k}: {v}");
            }
        }
    }
    Ok(())
}

fn verify(args: &[String]) -> Res<()> {
    let mut bad = 0;
    for p in args {
        match read(Path::new(p)).and_then(|f| {
            let img = Rgb8Image::from_png(&f.png)?;
            if (img.width, img.height) != (f.header.width, f.header.height) {
                return Err("PIXL dimensions differ from HEAD".into());
            }
            if img.digest() != f.digests.distorted {
                return Err("PIXL digest differs from DIGS".into());
            }
            Ok(())
        }) {
            Ok(()) => println!("{p}: ok"),
            Err(e) => {
                bad += 1;
                println!("{p}: FAILED: {e}");
            }
        }
    }
    if bad > 0 {
        return Err(format!("{bad} file(s) failed"));
    }
    Ok(())
}

fn extract(args: &[String]) -> Res<()> {
    let [file, dir] = args else {
        return Err("extract FILE.evif DIR".into());
    };
    let f = read(Path::new(file))?;
    let dir = Path::new(dir);
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    if let Some(b) = &f.bits {
        let ext = String::from_utf8_lossy(&b.codec).trim().to_string();
        let bit_dir = if ext.eq_ignore_ascii_case("png") {
            let bit_dir = dir.join("bitstreams");
            fs::create_dir_all(&bit_dir).map_err(|e| format!("{}: {e}", bit_dir.display()))?;
            bit_dir
        } else {
            dir.to_path_buf()
        };
        let p = bit_dir.join(format!("{stem}.{ext}"));
        fs::write(&p, &b.data).map_err(|e| format!("{}: {e}", p.display()))?;
        println!("{}", p.display());
    }
    let p = dir.join(format!("{stem}.png"));
    fs::write(&p, &f.png).map_err(|e| format!("{}: {e}", p.display()))?;
    println!("{}", p.display());
    if let Some(s) = &f.scores {
        let p = dir.join(format!("{stem}.scores.json"));
        fs::write(
            &p,
            serde_json::to_string_pretty(s).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", p.display()))?;
        println!("{}", p.display());
    }
    Ok(())
}

/// Encode each image with explicit parameter vectors (one JSON object per
/// line of `--params`) or the family's baseline at `--qualities`, and write
/// the decoded renditions as PNG with a TSV of their sizes. The baseline
/// form gives the honest rate-distortion anchor any external scorer can use.
fn render(registry: &Registry, args: &[String]) -> Res<()> {
    let (mut family, mut out, mut qualities, mut params_file) = (None, None, None, None);
    let mut images = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let next = |i: usize| args.get(i + 1).cloned().ok_or_else(|| USAGE.to_string());
        match args[i].as_str() {
            "--family" => family = Some(next(i)?),
            "--out" => out = Some(PathBuf::from(next(i)?)),
            "--qualities" => qualities = Some(next(i)?),
            "--params" => params_file = Some(PathBuf::from(next(i)?)),
            a if a.starts_with("--") => return Err(USAGE.to_string()),
            a => {
                images.push(PathBuf::from(a));
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    let (Some(family), Some(out)) = (family, out) else {
        return Err(USAGE.to_string());
    };
    let fam = registry
        .family(&family)
        .ok_or_else(|| format!("unknown family {family}"))?;
    let mut jobs: Vec<(String, Value)> = Vec::new();
    match (qualities, params_file) {
        (Some(q), None) => {
            for v in q.split(',') {
                let q: f64 = v.trim().parse().map_err(|_| format!("bad quality {v}"))?;
                let p = fam
                    .baseline(q)
                    .ok_or_else(|| format!("{family} has no baseline"))?;
                jobs.push((format!("q{q:05.1}"), p));
            }
        }
        (None, Some(f)) => {
            let text = fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            for (n, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
                let p: Value = serde_json::from_str(line)
                    .map_err(|e| format!("params line {}: {e}", n + 1))?;
                jobs.push((format!("p{n:04}"), p));
            }
        }
        _ => return Err(USAGE.to_string()),
    }
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let tsv_path = out.join("render.tsv");
    let fresh = !tsv_path.exists();
    let mut tsv = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&tsv_path)
        .map_err(|e| format!("{}: {e}", tsv_path.display()))?;
    if fresh {
        writeln!(tsv, "image\ttag\tbytes\tbpp\tpng\tparams").map_err(|e| e.to_string())?;
    }
    for img_path in &images {
        let id = img_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::read(img_path).map_err(|e| format!("{}: {e}", img_path.display()))?;
        let reference = Rgb8Image::from_png(&bytes).map_err(|e| format!("{id}: {e}"))?;
        for (tag, p) in &jobs {
            let enc = match fam.encode(p, &reference) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("{id} {tag}: refused: {e}");
                    continue;
                }
            };
            let png = out.join(format!("{id}-{family}-{tag}.png"));
            fs::write(&png, enc.pixels.to_png()?).map_err(|e| format!("{}: {e}", png.display()))?;
            if let Some(bits) = &enc.bits {
                // A separate directory avoids colliding with the rendition when
                // the encoded format itself is PNG.
                let dir = out.join("bitstreams");
                fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let ext = String::from_utf8_lossy(&bits.codec).trim().to_string();
                let path = dir.join(format!("{id}-{family}-{tag}.{ext}"));
                fs::write(&path, &bits.data).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            let b = enc.bytes().map_or("nan".to_string(), |b| b.to_string());
            let bpp = enc.bpp().map_or("nan".to_string(), |v| format!("{v:.6}"));
            writeln!(tsv, "{id}\t{tag}\t{b}\t{bpp}\t{}\t{p}", png.display())
                .map_err(|e| e.to_string())?;
        }
        eprintln!("{id}: {} renditions", jobs.len());
    }
    Ok(())
}
