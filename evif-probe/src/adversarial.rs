//! One-process chroma pilot: fixed source membership, matched budgets, durable candidates.
use crate::{
    Res,
    adversarial_judges::{self, sha},
    family::Family,
    image::Rgb8Image,
    judge::Judge,
    registry::Registry,
    rng::Rng,
    search::{Baseline, Setup},
    write,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    build_commit: String,
    samples: usize,
    seed: u64,
    views: Vec<View>,
    controls: Vec<Control>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    id: String,
    source: String,
    source_path: PathBuf,
    source_sha256: String,
    class: String,
    content_class: String,
    split: String,
    path: PathBuf,
    sha256: String,
    /// Family -> admitted ladder vectors, normalized to [0,1].
    seeds: BTreeMap<String, Vec<Vec<f64>>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    reference: PathBuf,
    distorted: PathBuf,
    expected: f64,
    reference_pixels_sha256: String,
    distorted_pixels_sha256: String,
}

fn save(path: impl AsRef<Path>, value: &(impl Serialize + ?Sized)) -> Res<()> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn image(path: &Path) -> Res<Rgb8Image> {
    Rgb8Image::from_png(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
}
fn margin(gains: &BTreeMap<String, f64>, target: &str) -> Res<f64> {
    let get = |k: &str| {
        gains
            .get(k)
            .copied()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("missing/nonfinite gain: {k}"))
    };
    let t = get(target)?;
    Ok((t - get("ssim2-frozen")?).min(t - get("butteraugli")?))
}

/// DE/rand/1/bin on a bounded normalized cube; random and DE share initialization.
struct Proposals {
    rng: Rng,
    population: Vec<(Vec<f64>, f64)>,
    cursor: usize,
    dimension: usize,
    optimize: bool,
}
impl Proposals {
    fn new(seed: u64, key: &str, dimension: usize, optimize: bool) -> Self {
        Self {
            rng: Rng::for_key(seed, key),
            population: Vec::new(),
            cursor: 0,
            dimension,
            optimize,
        }
    }
    fn next(&mut self, seeds: &[Vec<f64>]) -> Vec<f64> {
        if self.cursor < seeds.len().min(2) {
            return seeds[self.cursor].clone();
        }
        if !self.optimize || self.population.len() < 6 {
            return (0..self.dimension).map(|_| self.rng.unit()).collect();
        }
        let i = self.cursor % 6;
        let mut indices: Vec<usize> = (0..6).filter(|&n| n != i).collect();
        for j in 0..3 {
            let k = j + self.rng.below(indices.len() - j);
            indices.swap(j, k);
        }
        let [a, b, c] = [indices[0], indices[1], indices[2]];
        let forced = self.rng.below(self.dimension);
        (0..self.dimension)
            .map(|j| {
                if j == forced || self.rng.chance(0.9) {
                    (self.population[a].0[j]
                        + 0.8 * (self.population[b].0[j] - self.population[c].0[j]))
                        .clamp(0.0, 1.0)
                } else {
                    self.population[i].0[j]
                }
            })
            .collect()
    }
    fn observe(&mut self, vector: Vec<f64>, score: f64) {
        if self.population.len() < 6 {
            self.population.push((vector, score));
        } else {
            let i = self.cursor % 6;
            if score > self.population[i].1 {
                self.population[i] = (vector, score);
            }
        }
        self.cursor += 1;
    }
}

fn dimensions(family: &str) -> Res<usize> {
    match family {
        "zenjpeg-color" | "zenav1-svt" => Ok(2),
        "zenjpeg-expert" => Ok(8),
        _ => Err(format!("unsupported chroma family {family}")),
    }
}
fn params(family: &dyn Family, x: &[f64]) -> Res<Value> {
    let name = family.name();
    if x.len() != dimensions(name)? || x.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err("invalid normalized chroma vector".into());
    }
    let mut p = family.baseline(95.0).ok_or("family has no baseline")?;
    match name {
        "zenjpeg-color" => {
            // XYB's X and B controls, fixed Y/quality/subsampling/AQ.
            p["color"] = json!({"path":"xyb", "b_quarter":true});
            p["tables"]["chroma_scales"] = json!(
                x.iter()
                    .map(|v| (0.1_f64.ln() + v * 50_f64.ln()).exp())
                    .collect::<Vec<_>>()
            );
        }
        "zenjpeg-expert" => {
            p["subsampling"] = json!("s444");
            for c in 1..3 {
                for b in 0..4 {
                    p["quant_scale"][c][b] = json!((x[(c - 1) * 4 + b] * 12.0 - 6.0).exp2());
                }
            }
        }
        "zenav1-svt" => {
            p["chroma_q"] = json!(
                x.iter()
                    .map(|v| (v * 127.0).round() as i16 - 64)
                    .collect::<Vec<_>>()
            )
        }
        _ => unreachable!(),
    }
    Ok(p)
}

fn replay(controls: &[Control], out: &Path) -> Res<()> {
    if controls.len() != 4 {
        return Err("exactly four frozen teacher controls required".into());
    }
    let mut rows = Vec::new();
    for c in controls {
        let r = image(&c.reference)?;
        let d = image(&c.distorted)?;
        let value = adversarial_judges::Frozen.score(&r, &d)?[0].value;
        let rh = sha(&r.data);
        let dh = sha(&d.data);
        let pass = value.to_bits() == c.expected.to_bits()
            && rh == c.reference_pixels_sha256
            && dh == c.distorted_pixels_sha256;
        rows.push(json!({"reference":c.reference,"distorted":c.distorted,"expected":c.expected,"value":value,
            "expected_bits":format!("{:016x}",c.expected.to_bits()),"value_bits":format!("{:016x}",value.to_bits()),
            "reference_pixels_sha256":rh,"distorted_pixels_sha256":dh,"pass":pass}));
    }
    save(out.join("CONTROL.json"), &rows)?;
    if rows.iter().any(|r| r["pass"] != true) {
        return Err("frozen teacher control replay failed; pilot refused".into());
    }
    Ok(())
}

pub(crate) fn run(registry: &Registry, args: &[String]) -> Res<()> {
    if args.len() != 2 {
        return Err("usage: evif adversarial SPEC.json NEW_OUTPUT_DIR".into());
    }
    let spec_bytes = fs::read(&args[0]).map_err(|e| e.to_string())?;
    let spec: Spec = serde_json::from_slice(&spec_bytes).map_err(|e| e.to_string())?;
    if spec.samples < 12 || spec.views.is_empty() || spec.build_commit.is_empty() {
        return Err("pilot requires >=12 samples, views and build_commit".into());
    }
    let out = Path::new(&args[1]);
    fs::create_dir(out).map_err(|e| format!("new output directory required: {e}"))?;
    fs::write(out.join("SPEC.json"), &spec_bytes).map_err(|e| e.to_string())?;
    replay(&spec.controls, out)?;
    let mut judges: Vec<Box<dyn Judge>> = ["zensim-e", "zensim-e-codec", "ssim2-frozen"]
        .into_iter()
        .map(|n| registry.judge(n).ok_or_else(|| format!("judge {n} absent")))
        .collect::<Res<_>>()?;
    fs::create_dir(out.join("maps")).map_err(|e| e.to_string())?;
    judges.push(Box::new(adversarial_judges::MappedButteraugli(
        out.join("maps"),
    )));
    let families = ["zenjpeg-color", "zenjpeg-expert", "zenav1-svt"]
        .into_iter()
        .map(|n| {
            registry
                .family(n)
                .ok_or_else(|| format!("family {n} absent; enable possession-svt"))
        })
        .collect::<Res<Vec<_>>>()?;
    let mut ids = BTreeSet::new();
    let mut canonical_sources = BTreeMap::new();
    for v in &spec.views {
        if v.split != "train"
            || v.source.is_empty()
            || v.id.is_empty()
            || v.id.contains(['/', '\\'])
            || !ids.insert(&v.id)
        {
            return Err(
                "pilot requires unique safe view IDs and TRAIN-only source identities".into(),
            );
        }
        for (family, seeds) in &v.seeds {
            let n = dimensions(family)?;
            if seeds.len() > 2
                || seeds.iter().any(|x| {
                    x.len() != n || x.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
                })
            {
                return Err(format!("invalid admitted seeds for {} {family}", v.id));
            }
        }
    }
    for v in &spec.views {
        if !canonical_sources.contains_key(&v.source) {
            let bytes = fs::read(&v.source_path).map_err(|e| e.to_string())?;
            if sha(&bytes) != v.source_sha256 {
                return Err(format!("original source hash differs: {}", v.source));
            }
            let pixels = Rgb8Image::from_png(&bytes)?;
            let identity = format!("{}x{}:{}", pixels.width, pixels.height, sha(&pixels.data));
            canonical_sources.insert(v.source.clone(), identity);
        }
    }
    save(out.join("SOURCE_IDENTITIES.json"), &canonical_sources)?;
    let mut ledger = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("ledger.jsonl"))
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut completed = 0usize;
    for v in &spec.views {
        let source = &canonical_sources[&v.source];
        let bytes = fs::read(&v.path).map_err(|e| e.to_string())?;
        if sha(&bytes) != v.sha256 {
            return Err(format!("source view hash differs: {}", v.id));
        }
        let reference = Rgb8Image::from_png(&bytes)?;
        let partition = if u64::from_str_radix(&sha(source.as_bytes())[..16], 16)
            .unwrap()
            .is_multiple_of(5)
        {
            "gauntlet"
        } else {
            "training-leg"
        };
        let dir = out.join(&v.id);
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        for family in &families {
            let mut points = Vec::new();
            for q in (0..=100).step_by(5) {
                let p = family.baseline(q as f64).ok_or("no quality baseline")?;
                let enc = family.encode(&p, &reference)?;
                let setup = Setup {
                    family: family.as_ref(),
                    judges: &judges,
                    baseline: None,
                    reference: &reference,
                    target: "zensim-e",
                };
                let s = setup.evaluate(p, &enc)?;
                points.push((
                    s.bpp.ok_or("baseline cost absent")?,
                    crate::search::by_key(&s.scores),
                ));
                let f = write::build(family.as_ref(), &reference, source, None, &s, &enc)?;
                fs::write(
                    dir.join(format!("{}-baseline-q{q:03}.evif", family.name())),
                    f.to_bytes().map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
            let baseline = Baseline::from_points(points);
            save(
                dir.join(format!("{}-baseline.json", family.name())),
                baseline.points(),
            )?;
            for target in ["zensim-e", "zensim-e-codec"] {
                let setup = Setup {
                    family: family.as_ref(),
                    judges: &judges,
                    baseline: Some(&baseline),
                    reference: &reference,
                    target,
                };
                let seeds = v.seeds.get(family.name()).map(Vec::as_slice).unwrap_or(&[]);
                // Same initialization and source-specific seed in both arms; alternate arm order by source.
                let arms = if partition == "gauntlet" {
                    ["de", "random"]
                } else {
                    ["random", "de"]
                };
                for arm in arms {
                    let key = format!("{}:{}:{target}", source, family.name());
                    let mut proposals =
                        Proposals::new(spec.seed, &key, dimensions(family.name())?, arm == "de");
                    let arm_start = Instant::now();
                    let mut finds = 0;
                    let mut unique = BTreeSet::new();
                    for i in 0..spec.samples {
                        let vector = proposals.next(seeds);
                        let p = params(family.as_ref(), &vector)?;
                        // Refusals fail the cell; never replace a failed encode outside the budget.
                        let enc = family.encode(&p, &reference)?;
                        let mut s = setup.evaluate(p, &enc)?;
                        if s.scores.iter().any(|s| !s.value.is_finite()) {
                            return Err("nonfinite score".into());
                        }
                        let objective = margin(&s.gain, target)?;
                        s.disagreement = Some(objective);
                        proposals.observe(vector.clone(), objective);
                        let bits = enc.bits.as_ref().ok_or("real encode bytes required")?;
                        let encoded_sha = sha(&bits.data);
                        let fresh = unique.insert(encoded_sha.clone());
                        let found = objective > 0.0;
                        if found && fresh {
                            finds += 1;
                        }
                        let mut f = write::build(
                            family.as_ref(),
                            &reference,
                            source,
                            Some(target),
                            &s,
                            &enc,
                        )?;
                        let metadata = json!({"source":source,"source_label":v.source,"view":v.id,"class":v.class,"content_class":v.content_class,"partition":partition,
                            "split_rule":"sha256(source UTF-8), first 64 bits big endian modulo 5: zero=gauntlet, otherwise=training-leg",
                            "arm":arm,"index":i,"vector":vector,"two_reference_margin":objective,"find":found,
                            "unique_in_arm":fresh,"encoded_sha256":encoded_sha,"training_target":"ssim2-frozen"});
                        f.scores
                            .as_mut()
                            .unwrap()
                            .extra
                            .insert("adversarial".into(), metadata.clone());
                        let name = format!("{}-{target}-{arm}-{i:04}.evif", family.name());
                        fs::write(dir.join(&name), f.to_bytes().map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                        let row = json!({"view":v.id,"source":source,"source_label":v.source,"class":v.class,"content_class":v.content_class,"partition":partition,"family":family.name(),
                            "target":target,"arm":arm,"index":i,"find":found,"unique_in_arm":fresh,"encoded_sha256":encoded_sha,
                            "gains":s.gain,"scores":s.scores,"margin":objective,"params":s.params,"file":format!("{}/{name}",v.id)});
                        writeln!(ledger, "{row}").map_err(|e| e.to_string())?;
                        ledger.flush().map_err(|e| e.to_string())?;
                    }
                    let receipt = json!({"view":v.id,"family":family.name(),"target":target,"arm":arm,"encodes":spec.samples,
                        "unique_encodes":unique.len(),"unique_finds":finds,"wall_seconds":arm_start.elapsed().as_secs_f64()});
                    save(
                        dir.join(format!("{}-{target}-{arm}.json", family.name())),
                        &receipt,
                    )?;
                    eprintln!("{receipt}");
                    completed += 1;
                    save(
                        out.join("PROGRESS.json"),
                        &json!({"pid":std::process::id(),"build_commit":spec.build_commit,
                        "completed_arms":completed,"required_arms":spec.views.len()*12,"elapsed_seconds":start.elapsed().as_secs_f64(),"stage":"search"}),
                    )?;
                }
            }
        }
    }
    save(
        out.join("COMPLETE.json"),
        &json!({"completed_arms":completed,"views":spec.views.len(),"wall_seconds":start.elapsed().as_secs_f64()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_references_must_agree_and_legacy_judges_do_not_vote() {
        let mut g = BTreeMap::from([
            ("t".into(), 1.0),
            ("ssim2-frozen".into(), 0.2),
            ("butteraugli".into(), 1.2),
            ("legacy".into(), -100.0),
        ]);
        assert!(margin(&g, "t").unwrap() < 0.0);
        g.insert("butteraugli".into(), 0.5);
        assert_eq!(margin(&g, "t").unwrap(), 0.5);
        g.remove("ssim2-frozen");
        assert!(margin(&g, "t").is_err());
    }
    #[test]
    fn optimizer_uses_feedback_reproducibly_within_bounds() {
        fn trial(optimize: bool) -> Vec<Vec<f64>> {
            let mut p = Proposals::new(7, "test", 2, optimize);
            let mut xs = Vec::new();
            for _ in 0..24 {
                let x = p.next(&[]);
                assert!(x.iter().all(|v| (0.0..=1.0).contains(v)));
                p.observe(x.clone(), -((x[0] - 0.3).powi(2) + (x[1] - 0.7).powi(2)));
                xs.push(x);
            }
            xs
        }
        let a = trial(true);
        assert_eq!(a, trial(true));
        let r = trial(false);
        assert_eq!(a[..6], r[..6]);
        assert_ne!(a[6..], r[6..]);
    }
    #[test]
    fn expert_chroma_vectors_preserve_luma_and_other_controls() {
        let family = crate::family::zenjpeg_expert::ZenjpegExpert;
        let a = params(&family, &[0.0; 8]).unwrap();
        let b = params(&family, &[1.0; 8]).unwrap();
        assert_eq!(a["quant_scale"][0], json!([1., 1., 1., 1.]));
        for k in [
            "quality",
            "zero_bias_scale",
            "subsampling",
            "adaptive_quant",
            "trellis",
            "deringing",
        ] {
            assert_eq!(a[k], b[k]);
        }
        assert_ne!(a["quant_scale"][1], b["quant_scale"][1]);
    }
}
