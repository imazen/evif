//! `evif`: probe metrics, and inspect, verify and unpack EVIF files.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use evif::{Bpp, Evif};
use evif_probe::family::{self, Family};
use evif_probe::image::Rgb8Image;
use evif_probe::judge::{self, Judge};
use evif_probe::rng::Rng;
use evif_probe::search::{self, Baseline};
use evif_probe::{Res, write};

const USAGE: &str = "\
evif: stress-test image quality metrics with a self-reporting container

  evif probe   --family F --target KEY --out DIR [--judges a,b,c] [--samples N]
               [--keep K] [--seed S] [--baseline-step Q] IMAGE.png...
  evif inspect FILE.evif...        print header, bpp, recipe and scores
  evif verify  FILE.evif...        check SEAL, PIXL decode, pixel digest
  evif extract FILE.evif DIR       write the interior bitstream and PNG
  evif list                        families, judges and score keys

Families: zenjpeg-expert, corruption. Judges: zensim, ssim2, butteraugli
(keys zensim, ssim2, butteraugli, butteraugli/3-norm).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("probe") => probe(&args[1..]),
        Some("inspect") => inspect(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("extract") => extract(&args[1..]),
        Some("list") => list(),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("evif: {e}");
        std::process::exit(1);
    }
}

fn list() -> Res<()> {
    println!("families: {}", family::ALL.join(", "));
    for name in judge::ALL {
        let j = judge::by_name(name).expect("listed judge exists");
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

fn parse_probe(args: &[String]) -> Res<ProbeArgs> {
    let mut a = ProbeArgs {
        family: String::new(),
        target: String::new(),
        out: PathBuf::new(),
        judges: judge::ALL.iter().map(|s| s.to_string()).collect(),
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

fn probe(args: &[String]) -> Res<()> {
    let a = parse_probe(args)?;
    let fam: Box<dyn Family> =
        family::by_name(&a.family).ok_or_else(|| format!("unknown family {}", a.family))?;
    let judges: Vec<Box<dyn Judge>> = a
        .judges
        .iter()
        .map(|n| judge::by_name(n).ok_or_else(|| format!("unknown judge {n}")))
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
    let row = |image: &str, kind: &str, index: usize, s: &search::Sample| -> String {
        let mut cols = vec![
            image.to_string(),
            fam.name().to_string(),
            kind.to_string(),
            index.to_string(),
            s.bpp.map_or(String::new(), |b| format!("{b:.5}")),
        ];
        let by = search::by_key(&s.scores);
        cols.extend(
            keys.iter()
                .map(|k| by.get(k).map_or(String::new(), |v| format!("{v:.5}"))),
        );
        cols.extend(
            keys.iter()
                .map(|k| s.gain.get(k).map_or(String::new(), |v| format!("{v:.5}"))),
        );
        cols.push(s.disagreement.map_or(String::new(), |d| format!("{d:.5}")));
        cols.push(s.params.to_string());
        cols.join("\t")
    };

    let qualities: Vec<f64> = {
        let mut q = 5.0;
        let mut v = Vec::new();
        while q <= 95.0 + 1e-9 {
            v.push(q);
            q += a.baseline_step;
        }
        v
    };

    let mut summary: Vec<(String, Vec<search::Sample>)> = Vec::new();
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
                let s = search::Sample {
                    params: fam.baseline(qualities[i]).unwrap_or_default(),
                    bpp: Some(*bpp),
                    scores: Vec::new(),
                    gain: BTreeMap::new(),
                    disagreement: None,
                };
                // Baseline rows carry raw scores through the gain columns'
                // neighbours: rebuild Score rows so the TSV has them.
                let mut cols = row(&id, "baseline", i, &s)
                    .split('\t')
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                for (k, key) in keys.iter().enumerate() {
                    cols[5 + k] = scores.get(key).map_or(String::new(), |v| format!("{v:.5}"));
                }
                writeln!(ledger, "{}", cols.join("\t")).map_err(|e| e.to_string())?;
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
        let results = setup.run(&mut rng, a.samples, |i, _, _| {
            if (i + 1) % 8 == 0 || i + 1 == a.samples {
                eprintln!("{id}: {}/{} samples", i + 1, a.samples);
            }
        })?;
        for (i, (s, _)) in results.iter().enumerate() {
            writeln!(ledger, "{}", row(&id, "sample", i, s)).map_err(|e| e.to_string())?;
        }

        // Keep the strongest disagreements (or, without a baseline, the
        // samples the target likes most relative to the judges' rank).
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
    md.push_str("Gains are score minus the honest baseline at the same bpp, in units of that key's baseline span. Disagreement is the target's gain minus the mean of the others'.\n\n");
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
        let p = dir.join(format!("{stem}.{ext}"));
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
