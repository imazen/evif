//! Luma-gain / chroma-collapse trade ladders, byte-matched pairs and a DE search over the
//! trade dimensions. Every definition is registered in `docs/trade-ladders.md`; the truth rule
//! and the gate are evaluated from the ledger by `tools/trade_gate.py`.
use crate::{
    Res,
    adversarial::{Control, Proposals, View, replay, save},
    adversarial_judges::{GatedButteraugli, Research, sha},
    family::{Encoded, Family},
    image::Rgb8Image,
    judge::Judge,
    registry::Registry,
    search::{Baseline, Sample, Setup, by_key},
    trade_judges::{CvvdpJudge, LumaDiag, Shared},
    write,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

// Chroma rung lists of the chroma-only ladders EVIF already renders. The literals are the
// stored recipes' values, so they stay as written (not `consts::SQRT_2`).
#[allow(clippy::approx_constant)]
const CHROMA: [f64; 16] = [
    0.0,
    0.5,
    0.707_106_78,
    1.0,
    1.414_213_56,
    2.0,
    2.828_427_12,
    4.0,
    5.656_854_25,
    8.0,
    11.313_708_5,
    16.0,
    22.627_417,
    32.0,
    45.254_834,
    64.0,
];
#[allow(clippy::approx_constant)]
const SCALES: [f64; 14] = [
    0.1,
    0.141_421_36,
    0.2,
    0.282_842_71,
    0.4,
    0.565_685_42,
    0.8,
    1.0,
    1.131_370_85,
    1.6,
    2.262_741_7,
    3.2,
    4.0,
    5.0,
];
#[allow(clippy::approx_constant)]
const EXT_TAIL: [f64; 16] = [
    6.4,
    8.0,
    11.313_708_5,
    16.0,
    22.627_417,
    32.0,
    45.254_834,
    64.0,
    90.509_668,
    128.0,
    181.019_336,
    256.0,
    362.038_672,
    512.0,
    724.077_344,
    1024.0,
];
const DELTAS: [i16; 17] = [
    -64, -56, -48, -40, -32, -24, -16, -8, 0, 8, 16, 24, 32, 40, 48, 56, 63,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tf {
    Ycc444,
    Ycc420,
    XybExtended,
    XybCapped,
    Svt420,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ramp {
    None,
    Tiny,
    Medium,
    Large,
}

impl Ramp {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Ramp::None => "none",
            Ramp::Tiny => "tiny",
            Ramp::Medium => "medium",
            Ramp::Large => "large",
        }
    }
    pub(crate) fn parse(s: &str) -> Res<Self> {
        [Ramp::None, Ramp::Tiny, Ramp::Medium, Ramp::Large]
            .into_iter()
            .find(|r| r.id() == s)
            .ok_or_else(|| format!("unknown ramp {s}"))
    }
    /// Expert luma multiplier at the last rung.
    fn expert_end(self) -> f64 {
        [1.0, 0.8, 0.5, 0.25][self as usize]
    }
    /// `xyb-capped` quality at the last rung.
    fn color_quality_end(self) -> f64 {
        [95.0, 96.2, 98.1, 99.6][self as usize]
    }
    /// `svt-420` quality at the last rung.
    fn svt_quality_end(self) -> f64 {
        [95.0, 96.0, 97.5, 99.0][self as usize]
    }
}

fn mul4(v: f64) -> Value {
    json!([v, v, v, v])
}

impl Tf {
    pub(crate) const ALL: [Tf; 5] = [
        Tf::Ycc444,
        Tf::Ycc420,
        Tf::XybExtended,
        Tf::XybCapped,
        Tf::Svt420,
    ];
    pub(crate) fn id(self) -> &'static str {
        match self {
            Tf::Ycc444 => "ycc444",
            Tf::Ycc420 => "ycc420",
            Tf::XybExtended => "xyb-extended",
            Tf::XybCapped => "xyb-capped",
            Tf::Svt420 => "svt-420",
        }
    }
    pub(crate) fn parse(s: &str) -> Res<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.id() == s)
            .ok_or_else(|| format!("unknown trade family {s}"))
    }
    pub(crate) fn codec_family(self) -> &'static str {
        match self {
            Tf::Ycc444 | Tf::Ycc420 | Tf::XybExtended => "zenjpeg-expert",
            Tf::XybCapped => "zenjpeg-color",
            Tf::Svt420 => "zenav1-svt",
        }
    }
    pub(crate) fn rungs(self) -> usize {
        match self {
            Tf::Ycc444 | Tf::Ycc420 => CHROMA.len(),
            Tf::XybExtended => SCALES.len() + EXT_TAIL.len(),
            Tf::XybCapped => SCALES.len(),
            Tf::Svt420 => DELTAS.len(),
        }
    }
    /// The chroma control value of rung `k`.
    pub(crate) fn chroma(self, k: usize) -> f64 {
        match self {
            Tf::Ycc444 | Tf::Ycc420 => CHROMA[k],
            Tf::XybExtended => SCALES
                .iter()
                .chain(&EXT_TAIL)
                .copied()
                .nth(k)
                .expect("rung in range"),
            Tf::XybCapped => SCALES[k],
            Tf::Svt420 => f64::from(DELTAS[k]),
        }
    }
    /// Expert recipe with explicit luma and chroma multipliers on all four bands.
    fn expert(self, base: &dyn Family, luma: f64, chroma: f64, quality: f64) -> Res<Value> {
        let mut p = base.baseline(quality).ok_or("family has no baseline")?;
        let xyb = self == Tf::XybExtended;
        p["subsampling"] = json!(if self == Tf::Ycc444 { "s444" } else { "s420" });
        p["quant_scale"] = if xyb {
            json!([mul4(chroma), mul4(luma), mul4(chroma)])
        } else {
            json!([mul4(luma), mul4(chroma), mul4(chroma)])
        };
        p["xyb"] = json!(xyb);
        Ok(p)
    }
    fn capped(base: &dyn Family, scales: [f64; 2], quality: f64) -> Res<Value> {
        let mut p = base.baseline(quality).ok_or("family has no baseline")?;
        p["color"] = json!({"path":"xyb","b_quarter":true});
        p["tables"]["chroma_scales"] = json!(scales);
        Ok(p)
    }
    fn svt(base: &dyn Family, u: i64, v: i64, quality: f64) -> Res<Value> {
        let mut p = base.baseline(quality).ok_or("family has no baseline")?;
        p["chroma_q"] = json!([u, v]);
        Ok(p)
    }
    /// The deterministic recipe of rung `k` on `ramp`.
    pub(crate) fn ladder(self, base: &dyn Family, ramp: Ramp, k: usize) -> Res<Value> {
        let rungs = self.rungs();
        if k >= rungs {
            return Err(format!("{} has {rungs} rungs", self.id()));
        }
        let frac = k as f64 / (rungs - 1) as f64;
        let chroma = self.chroma(k);
        match self {
            Tf::Ycc444 | Tf::Ycc420 | Tf::XybExtended => {
                self.expert(base, ramp.expert_end().powf(frac), chroma, 95.0)
            }
            Tf::XybCapped => Self::capped(
                base,
                [chroma, chroma],
                95.0 + (ramp.color_quality_end() - 95.0) * frac,
            ),
            Tf::Svt420 => Self::svt(
                base,
                chroma as i64,
                chroma as i64,
                95.0 + (ramp.svt_quality_end() - 95.0) * frac,
            ),
        }
    }
    /// Allocation ratios of the byte-matched pairs (chroma-heavy to luma-heavy).
    pub(crate) fn bm_rhos(self) -> &'static [f64] {
        match self {
            Tf::Ycc444 | Tf::Ycc420 | Tf::XybExtended => &[0.25, 1.0, 4.0, 16.0, 64.0],
            Tf::XybCapped => &[0.25, 1.0, 4.0, 5.0],
            Tf::Svt420 => &[-32.0, 0.0, 32.0, 63.0],
        }
    }
    /// Recipe at allocation `rho` and rate knob `quality`.
    pub(crate) fn bytematch(self, base: &dyn Family, rho: f64, quality: f64) -> Res<Value> {
        match self {
            Tf::Ycc444 | Tf::Ycc420 | Tf::XybExtended => {
                self.expert(base, rho.powf(-0.5), rho.powf(0.5), quality)
            }
            Tf::XybCapped => Self::capped(base, [rho, rho], quality),
            Tf::Svt420 => Self::svt(base, rho as i64, rho as i64, quality),
        }
    }
    fn bm_quality_range(self) -> (f64, f64) {
        match self {
            Tf::Svt420 => (1.0, 100.0),
            _ => (5.0, 100.0),
        }
    }
    pub(crate) fn dims(self) -> usize {
        match self {
            Tf::Ycc444 | Tf::Ycc420 => 9,
            _ => 3,
        }
    }
    /// A recipe from a normalized vector in `[0,1]^dims`.
    pub(crate) fn de(self, base: &dyn Family, x: &[f64]) -> Res<Value> {
        if x.len() != self.dims() || x.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("invalid normalized trade vector".into());
        }
        let luma = (-2.0 * x[0]).exp2();
        let log_scale = |v: f64, lo: f64, hi: f64| (lo.ln() + v * (hi / lo).ln()).exp();
        match self {
            Tf::Ycc444 | Tf::Ycc420 => {
                let mut p = self.expert(base, luma, 1.0, 95.0)?;
                for c in 1..3 {
                    for b in 0..4 {
                        p["quant_scale"][c][b] =
                            json!((x[1 + (c - 1) * 4 + b] * 12.0 - 6.0).exp2());
                    }
                }
                Ok(p)
            }
            Tf::XybExtended => {
                let mut p = self.expert(base, luma, 1.0, 95.0)?;
                p["quant_scale"][0] = mul4(log_scale(x[1], 0.1, 1024.0));
                p["quant_scale"][2] = mul4(log_scale(x[2], 0.1, 1024.0));
                Ok(p)
            }
            Tf::XybCapped => Self::capped(
                base,
                [log_scale(x[0], 0.1, 5.0), log_scale(x[1], 0.1, 5.0)],
                95.0 + x[2] * 4.6,
            ),
            Tf::Svt420 => Self::svt(
                base,
                (x[0] * 127.0).round() as i64 - 64,
                (x[1] * 127.0).round() as i64 - 64,
                95.0 + x[2] * 4.0,
            ),
        }
    }
}

/// The three reference margins `gain(t) - gain(S, B, C)`, the registered second-largest
/// (a find when positive) and the chroma pilot's two-reference minimum.
pub(crate) fn trade_margins(
    gains: &BTreeMap<String, f64>,
    target: &str,
) -> Res<([f64; 3], f64, f64)> {
    let get = |k: &str| {
        gains
            .get(k)
            .copied()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("missing/nonfinite gain: {k}"))
    };
    let t = get(target)?;
    let m = [
        t - get("ssim2-frozen")?,
        t - get("butteraugli")?,
        t - get("cvvdp")?,
    ];
    let mut sorted = m;
    sorted.sort_by(f64::total_cmp);
    Ok((m, sorted[1], m[0].min(m[1])))
}

struct Matched {
    params: Value,
    enc: Encoded,
    quality: f64,
    evals: usize,
    matched: bool,
}
/// Secant search on log size against `quality` for an encode within 1 % of `target` bytes.
fn match_bytes(
    tf: Tf,
    base: &dyn Family,
    reference: &Rgb8Image,
    rho: f64,
    target: u64,
) -> Res<Option<Matched>> {
    let (lo, hi) = tf.bm_quality_range();
    let ln_target = (target as f64).ln();
    let mut best: Option<Matched> = None;
    let mut best_err = f64::INFINITY;
    let mut points: Vec<(f64, f64)> = Vec::new();
    let mut q = 55.0;
    for n in 0..8 {
        let params = tf.bytematch(base, rho, q)?;
        let Ok(enc) = base.encode(&params, reference) else {
            return Ok(best);
        };
        let bytes = enc.bytes().ok_or("real encode bytes required")? as f64;
        let err = (bytes / target as f64 - 1.0).abs();
        if err < best_err {
            best_err = err;
            best = Some(Matched {
                params,
                enc,
                quality: q,
                evals: n + 1,
                matched: err <= 0.01,
            });
        }
        if err <= 0.01 {
            break;
        }
        points.push((q, bytes.ln()));
        let next = match points.as_slice() {
            [(_, y)] => q + if *y > ln_target { -20.0 } else { 20.0 },
            [.., (q0, y0), (q1, y1)] => {
                let slope = (y1 - y0) / (q1 - q0);
                if slope.is_finite() && slope > 1e-6 {
                    q1 + (ln_target - y1) / slope
                } else {
                    q + if *y1 > ln_target { -10.0 } else { 10.0 }
                }
            }
            [] => unreachable!(),
        }
        .clamp(lo, hi);
        if (next - q).abs() < 0.01 {
            break;
        }
        q = next;
    }
    if let Some(m) = best.as_mut() {
        m.evals = points.len().max(m.evals);
    }
    Ok(best)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricControl {
    reference: PathBuf,
    distorted: PathBuf,
    cvvdp: f64,
    butteraugli: f64,
    butteraugli_3norm: f64,
    /// SHA-256 of the PNG file bytes.
    reference_sha256: String,
    distorted_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByteControl {
    reference: PathBuf,
    /// SHA-256 of the PNG file bytes.
    reference_sha256: String,
    family: String,
    ramp: String,
    rung: usize,
    encoded_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    label: String,
    model: String,
    sha256: String,
}
fn de_samples() -> usize {
    16
}
fn map_mod() -> u64 {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    build_commit: String,
    seed: u64,
    views: Vec<View>,
    controls: Vec<Control>,
    #[serde(default)]
    metric_controls: Vec<MetricControl>,
    #[serde(default)]
    byte_controls: Vec<ByteControl>,
    families: Vec<String>,
    ramps: Vec<String>,
    /// Any of `ladder`, `bytematch`, `de`; the honest curves are always written.
    kinds: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default = "de_samples")]
    de_samples: usize,
    /// Honest-curve qualities (multiples of 5) whose sizes are the byte budgets.
    #[serde(default)]
    budgets: Vec<u32>,
    #[serde(default = "map_mod")]
    map_sample_mod: u64,
    /// Views that also run the labelled YCbCr range probe out to multiplier 1024.
    #[serde(default)]
    probe_views: Vec<String>,
    #[serde(default)]
    candidates: Vec<Candidate>,
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(x), Some(y)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in y {
            x.insert(k.clone(), v.clone());
        }
    }
    a
}
fn stable_sample(view: &str, name: &str, modulus: u64) -> bool {
    modulus != 0
        && u64::from_str_radix(&sha(format!("{view}|{name}").as_bytes())[..16], 16)
            .unwrap()
            .is_multiple_of(modulus)
}

/// An honest curve and the encoded sizes of its qualities.
type Honest = (Baseline, Vec<(u32, u64)>);

struct Runner<'a> {
    out: &'a Path,
    spec: &'a Spec,
    judges: Vec<Box<dyn Judge>>,
    gate: Rc<Cell<bool>>,
    butter: Rc<GatedButteraugli>,
    cvvdp: Rc<CvvdpJudge>,
    ledger: fs::File,
    rows: usize,
}
struct Vx<'a> {
    view: &'a View,
    source: &'a str,
    partition: &'static str,
    reference: &'a Rgb8Image,
    dir: PathBuf,
}

impl Runner<'_> {
    /// Score one encode (maps are written only while `want_maps` is set).
    fn score(
        &self,
        vx: &Vx,
        fam: &dyn Family,
        baseline: Option<&Baseline>,
        params: Value,
        enc: &Encoded,
        want_maps: bool,
    ) -> Res<Sample> {
        self.gate.set(want_maps);
        let setup = Setup {
            family: fam,
            judges: &self.judges,
            baseline,
            reference: vx.reference,
            target: "zensim-e",
        };
        let s = setup.evaluate(params, enc);
        self.gate.set(false);
        let s = s?;
        if s.scores.iter().any(|s| !s.value.is_finite()) {
            return Err("nonfinite score".into());
        }
        Ok(s)
    }
    /// Attach the native maps to an already scored sample (the scalars must not move).
    fn add_maps(&self, vx: &Vx, s: &mut Sample, enc: &Encoded) -> Res<()> {
        self.gate.set(true);
        let fresh = [
            self.butter.score(vx.reference, &enc.pixels),
            self.cvvdp.score(vx.reference, &enc.pixels),
        ];
        self.gate.set(false);
        for scores in fresh {
            for n in scores? {
                let old = s
                    .scores
                    .iter_mut()
                    .find(|o| o.key() == n.key())
                    .ok_or("map pass produced an unknown key")?;
                if old.value.to_bits() != n.value.to_bits() {
                    return Err(format!("map pass moved {}", n.key()));
                }
                *old = n;
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn write_evif(
        &self,
        vx: &Vx,
        fam: &dyn Family,
        target: Option<&str>,
        s: &Sample,
        enc: &Encoded,
        name: &str,
        meta: &Value,
    ) -> Res<String> {
        let mut f = write::build(fam, vx.reference, vx.source, target, s, enc)?;
        f.scores
            .as_mut()
            .ok_or("scores absent")?
            .extra
            .insert("trade".into(), meta.clone());
        let rel = format!("{}/{name}.evif", vx.view.id);
        fs::write(
            self.out.join(&rel),
            f.to_bytes().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok(rel)
    }
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, vx: &Vx, head: Value, s: &Sample, enc: &Encoded, file: &str) -> Res<()> {
        let row = merge(
            json!({"view":vx.view.id,"source":vx.source,"source_label":vx.view.source,"class":vx.view.class,
                "content_class":vx.view.content_class,"role":vx.view.role,"partition":vx.partition,
                "bytes":enc.bytes(),"bpp":enc.bpp(),"encoded_sha256":sha(&enc.bits.as_ref().ok_or("real encode bytes required")?.data),
                "scores":by_key(&s.scores),"gain":s.gain,"params":s.params,"file":file}),
            head,
        );
        writeln!(self.ledger, "{row}").map_err(|e| e.to_string())?;
        self.ledger.flush().map_err(|e| e.to_string())?;
        self.rows += 1;
        Ok(())
    }
    fn refused(&mut self, vx: &Vx, head: Value, why: &str) -> Res<()> {
        let row = merge(
            json!({"view":vx.view.id,"source":vx.source,"partition":vx.partition,"refused":why}),
            head,
        );
        writeln!(self.ledger, "{row}").map_err(|e| e.to_string())?;
        self.ledger.flush().map_err(|e| e.to_string())
    }
}

/// The `none` ramp must reproduce the encoded bytes of the stored chroma-only cells.
fn byte_controls(
    spec: &Spec,
    codec_families: &BTreeMap<&str, Box<dyn Family>>,
    out: &Path,
) -> Res<()> {
    let mut rows = Vec::new();
    for c in &spec.byte_controls {
        let tf = Tf::parse(&c.family)?;
        let fam = codec_families
            .get(tf.codec_family())
            .ok_or("byte control family not configured")?;
        let bytes =
            fs::read(&c.reference).map_err(|e| format!("{}: {e}", c.reference.display()))?;
        if sha(&bytes) != c.reference_sha256 {
            return Err(format!(
                "byte control reference differs: {}",
                c.reference.display()
            ));
        }
        let reference = Rgb8Image::from_png(&bytes)?;
        let params = tf.ladder(fam.as_ref(), Ramp::parse(&c.ramp)?, c.rung)?;
        let enc = fam.encode(&params, &reference)?;
        let got = sha(&enc.bits.as_ref().ok_or("real encode bytes required")?.data);
        rows.push(
            json!({"reference":c.reference,"family":c.family,"ramp":c.ramp,"rung":c.rung,
            "expected":c.encoded_sha256,"actual":got,"pass":got == c.encoded_sha256}),
        );
    }
    save(out.join("BYTE_CONTROL.json"), &rows)?;
    if rows.iter().any(|r| r["pass"] != true) {
        return Err("stored chroma-only cell bytes not reproduced; run refused".into());
    }
    Ok(())
}

fn controls(runner_judges: &[Box<dyn Judge>], spec: &Spec, out: &Path) -> Res<()> {
    replay(&spec.controls, out)?;
    let by = |name: &str| runner_judges.iter().find(|j| j.name() == name);
    let (butter, cv) = (by("butteraugli"), by("cvvdp"));
    let mut rows = Vec::new();
    for c in &spec.metric_controls {
        let read = |p: &Path| fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
        let (rb, db) = (read(&c.reference)?, read(&c.distorted)?);
        let (r, d) = (Rgb8Image::from_png(&rb)?, Rgb8Image::from_png(&db)?);
        let cvv = cv.ok_or("cvvdp judge absent")?.score(&r, &d)?[0].value;
        let b = butter.ok_or("butteraugli judge absent")?.score(&r, &d)?;
        let (rh, dh) = (sha(&rb), sha(&db));
        let pass = cvv.to_bits() == c.cvvdp.to_bits()
            && b[0].value.to_bits() == c.butteraugli.to_bits()
            && b[1].value.to_bits() == c.butteraugli_3norm.to_bits()
            && rh == c.reference_sha256
            && dh == c.distorted_sha256;
        rows.push(json!({"reference":c.reference,"distorted":c.distorted,
            "cvvdp":{"expected_bits":format!("{:016x}",c.cvvdp.to_bits()),"value_bits":format!("{:016x}",cvv.to_bits())},
            "butteraugli":{"expected_bits":format!("{:016x}",c.butteraugli.to_bits()),"value_bits":format!("{:016x}",b[0].value.to_bits())},
            "butteraugli_3norm":{"expected_bits":format!("{:016x}",c.butteraugli_3norm.to_bits()),"value_bits":format!("{:016x}",b[1].value.to_bits())},
            "pass":pass}));
    }
    save(out.join("METRIC_CONTROL.json"), &rows)?;
    if rows.iter().any(|r| r["pass"] != true) {
        return Err("cvvdp/butteraugli control replay failed; run refused".into());
    }
    Ok(())
}

pub(crate) fn run(registry: &Registry, args: &[String]) -> Res<()> {
    let [spec_path, out_path] = args else {
        return Err("usage: evif trade SPEC.json NEW_OUTPUT_DIR".into());
    };
    let spec_bytes = fs::read(spec_path).map_err(|e| format!("{spec_path}: {e}"))?;
    let spec: Spec = serde_json::from_slice(&spec_bytes).map_err(|e| e.to_string())?;
    if spec.views.is_empty() || spec.build_commit.is_empty() || spec.families.is_empty() {
        return Err("trade requires views, families and build_commit".into());
    }
    let families: Vec<Tf> = spec
        .families
        .iter()
        .map(|f| Tf::parse(f))
        .collect::<Res<_>>()?;
    let ramps: Vec<Ramp> = spec
        .ramps
        .iter()
        .map(|r| Ramp::parse(r))
        .collect::<Res<_>>()?;
    for k in &spec.kinds {
        if !["ladder", "bytematch", "de"].contains(&k.as_str()) {
            return Err(format!("unknown kind {k}"));
        }
    }
    if spec.kinds.iter().any(|k| k == "de") && (spec.targets.is_empty() || spec.de_samples < 12) {
        return Err("de needs targets and at least 12 samples".into());
    }
    if spec.kinds.iter().any(|k| k == "bytematch")
        && spec.budgets.iter().any(|q| q % 5 != 0 || *q > 100)
    {
        return Err("byte budgets are honest-curve qualities that are multiples of 5".into());
    }
    let out = Path::new(out_path);
    fs::create_dir(out).map_err(|e| format!("new output directory required: {e}"))?;
    fs::write(out.join("SPEC.json"), &spec_bytes).map_err(|e| e.to_string())?;
    fs::create_dir(out.join("maps")).map_err(|e| e.to_string())?;

    let gate = Rc::new(Cell::new(false));
    let butter = Rc::new(GatedButteraugli {
        maps: out.join("maps"),
        gate: gate.clone(),
    });
    let cvvdp = Rc::new(CvvdpJudge::new(out.join("maps"), gate.clone()));
    let mut judges: Vec<Box<dyn Judge>> = ["zensim-e", "zensim-e-codec"]
        .into_iter()
        .map(|n| registry.judge(n).ok_or_else(|| format!("judge {n} absent")))
        .collect::<Res<_>>()?;
    for c in &spec.candidates {
        judges.push(Box::new(Research::candidate(&c.label, &c.model, &c.sha256)));
    }
    judges.push(
        registry
            .judge("ssim2-frozen")
            .ok_or("judge ssim2-frozen absent")?,
    );
    judges.push(Box::new(Shared(butter.clone())));
    judges.push(Box::new(Shared(cvvdp.clone())));
    judges.push(Box::new(LumaDiag));
    let model_names: BTreeSet<&str> = judges.iter().map(|j| j.name()).collect();
    for t in &spec.targets {
        if !model_names.contains(t.as_str()) {
            return Err(format!("DE target {t} is not a configured model judge"));
        }
    }
    controls(&judges, &spec, out)?;

    let mut codec_families: BTreeMap<&str, Box<dyn Family>> = BTreeMap::new();
    for tf in &families {
        let name = tf.codec_family();
        if !codec_families.contains_key(name) {
            codec_families.insert(
                name,
                registry
                    .family(name)
                    .ok_or_else(|| format!("family {name} absent; enable possession-svt"))?,
            );
        }
    }
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
                "trade requires unique safe view IDs and TRAIN-only source identities".into(),
            );
        }
    }
    for v in &spec.views {
        if !canonical_sources.contains_key(&v.source) {
            let bytes = fs::read(&v.source_path)
                .map_err(|e| format!("{}: {e}", v.source_path.display()))?;
            if sha(&bytes) != v.source_sha256 {
                return Err(format!("original source hash differs: {}", v.source));
            }
            let pixels = Rgb8Image::from_png(&bytes)?;
            canonical_sources.insert(
                v.source.clone(),
                format!("{}x{}:{}", pixels.width, pixels.height, sha(&pixels.data)),
            );
        }
    }
    save(out.join("SOURCE_IDENTITIES.json"), &canonical_sources)?;
    byte_controls(&spec, &codec_families, out)?;
    let pins: Vec<Value> = judges
        .iter()
        .flat_map(|j| {
            j.keys()
                .into_iter()
                .map(move |k| json!({"judge":j.name(),"key":k}))
        })
        .collect();
    save(out.join("JUDGES.json"), &pins)?;

    let ledger = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("ledger.jsonl"))
        .map_err(|e| e.to_string())?;
    let mut runner = Runner {
        out,
        spec: &spec,
        judges,
        gate,
        butter,
        cvvdp,
        ledger,
        rows: 0,
    };
    let start = Instant::now();
    for (done, v) in spec.views.iter().enumerate() {
        let source = &canonical_sources[&v.source];
        let bytes = fs::read(&v.path).map_err(|e| format!("{}: {e}", v.path.display()))?;
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
        let vx = Vx {
            view: v,
            source,
            partition,
            reference: &reference,
            dir,
        };
        let t_view = Instant::now();
        let timing = process_view(&mut runner, &vx, &codec_families, &families, &ramps)?;
        save(vx.dir.join("timing.json"), &timing)?;
        save(
            out.join("PROGRESS.json"),
            &json!({"pid":std::process::id(),"build_commit":spec.build_commit,"views_done":done+1,
                "views_required":spec.views.len(),"rows":runner.rows,"elapsed_seconds":start.elapsed().as_secs_f64(),
                "last_view":v.id,"last_view_seconds":t_view.elapsed().as_secs_f64(),"stage":"trade"}),
        )?;
        eprintln!(
            "{} done in {:.1}s ({} rows)",
            v.id,
            t_view.elapsed().as_secs_f64(),
            runner.rows
        );
    }
    save(
        out.join("COMPLETE.json"),
        &json!({"views":spec.views.len(),"rows":runner.rows,"wall_seconds":start.elapsed().as_secs_f64()}),
    )
}

fn process_view(
    r: &mut Runner,
    vx: &Vx,
    codec_families: &BTreeMap<&str, Box<dyn Family>>,
    families: &[Tf],
    ramps: &[Ramp],
) -> Res<Value> {
    let spec = r.spec;
    let mut timing = BTreeMap::new();
    let mut baselines: BTreeMap<&str, Honest> = BTreeMap::new();
    // The honest curves (21 qualities) are shared by the trade families of one codec family.
    let t = Instant::now();
    for (name, fam) in codec_families {
        let mut points = Vec::new();
        let mut sizes = Vec::new();
        for q in (0..=100u32).step_by(5) {
            let p = fam.baseline(f64::from(q)).ok_or("no quality baseline")?;
            let enc = fam.encode(&p, vx.reference)?;
            let s = r.score(vx, fam.as_ref(), None, p, &enc, false)?;
            points.push((s.bpp.ok_or("baseline cost absent")?, by_key(&s.scores)));
            sizes.push((q, enc.bytes().ok_or("baseline bytes absent")?));
            let file = r.write_evif(
                vx,
                fam.as_ref(),
                None,
                &s,
                &enc,
                &format!("{name}-baseline-q{q:03}"),
                &json!({"kind":"baseline","codec_family":name,"quality":q}),
            )?;
            r.row(
                vx,
                json!({"kind":"baseline","codec_family":name,"quality":q}),
                &s,
                &enc,
                &file,
            )?;
        }
        baselines.insert(name, (Baseline::from_points(points), sizes));
    }
    timing.insert("baseline", t.elapsed().as_secs_f64());

    for &tf in families {
        let fam = codec_families[tf.codec_family()].as_ref();
        let (baseline, sizes) = &baselines[tf.codec_family()];
        if spec.kinds.iter().any(|k| k == "ladder") {
            let t = Instant::now();
            // Rung 0 of every ramp is one recipe; identical recipes are encoded and scored once.
            let mut seen: HashMap<String, (Sample, String, Encoded)> = HashMap::new();
            for &ramp in ramps {
                for k in 0..tf.rungs() {
                    let params = tf.ladder(fam, ramp, k)?;
                    let head = json!({"kind":"ladder","trade_family":tf.id(),"codec_family":tf.codec_family(),
                        "ramp":ramp.id(),"rung":k,"rungs":tf.rungs(),"chroma":tf.chroma(k)});
                    let key = params.to_string();
                    if let Some((s, file, enc)) = seen.get(&key) {
                        r.row(vx, head, s, enc, file)?;
                        continue;
                    }
                    let enc = match fam.encode(&params, vx.reference) {
                        Ok(e) => e,
                        Err(why) => {
                            r.refused(vx, head, &why)?;
                            continue;
                        }
                    };
                    let name = format!("{}-ladder-{}-r{k:02}", tf.id(), ramp.id());
                    let ends = k == 0 || k + 1 == tf.rungs();
                    let maps = ends || stable_sample(&vx.view.id, &name, spec.map_sample_mod);
                    let s = r.score(vx, fam, Some(baseline), params, &enc, maps)?;
                    let file = r.write_evif(vx, fam, None, &s, &enc, &name, &head)?;
                    r.row(vx, head, &s, &enc, &file)?;
                    seen.insert(key, (s, file, enc));
                }
            }
            timing.insert(
                leak(format!("ladder-{}", tf.id())),
                t.elapsed().as_secs_f64(),
            );
        }
        if spec.kinds.iter().any(|k| k == "bytematch") {
            let t = Instant::now();
            for &bq in &spec.budgets {
                let target = sizes
                    .iter()
                    .find(|(q, _)| *q == bq)
                    .map(|(_, b)| *b)
                    .ok_or("budget quality absent from the honest curve")?;
                for &rho in tf.bm_rhos() {
                    let head = json!({"kind":"bytematch","trade_family":tf.id(),"codec_family":tf.codec_family(),
                        "budget_quality":bq,"budget_bytes":target,"rho":rho});
                    let Some(m) = match_bytes(tf, fam, vx.reference, rho, target)? else {
                        r.refused(vx, head, "no encodable size")?;
                        continue;
                    };
                    let head = merge(
                        head,
                        json!({"matched":m.matched,"rate_quality":m.quality,"evals":m.evals}),
                    );
                    let name = format!("{}-bm-q{bq:03}-rho{rho}", tf.id());
                    let s = r.score(vx, fam, Some(baseline), m.params, &m.enc, true)?;
                    let file = r.write_evif(vx, fam, None, &s, &m.enc, &name, &head)?;
                    r.row(vx, head, &s, &m.enc, &file)?;
                }
            }
            timing.insert(
                leak(format!("bytematch-{}", tf.id())),
                t.elapsed().as_secs_f64(),
            );
        }
        if spec.kinds.iter().any(|k| k == "de") {
            let t = Instant::now();
            for target in &spec.targets {
                // Alternate arm order by partition, as the chroma pilot did.
                let arms = if vx.partition == "gauntlet" {
                    ["de", "random"]
                } else {
                    ["random", "de"]
                };
                for arm in arms {
                    let key = format!("{}:{}:{target}", vx.source, tf.id());
                    let mut proposals = Proposals::new(spec.seed, &key, tf.dims(), arm == "de");
                    let mut finds = 0usize;
                    let mut unique = BTreeSet::new();
                    for i in 0..spec.de_samples {
                        let x = proposals.next(&[]);
                        let params = tf.de(fam, &x)?;
                        let head0 = json!({"kind":"de","trade_family":tf.id(),"codec_family":tf.codec_family(),
                            "target":target,"arm":arm,"index":i,"vector":x});
                        let enc = match fam.encode(&params, vx.reference) {
                            Ok(e) => e,
                            Err(why) => {
                                // A refusal spends budget and is never replaced.
                                r.refused(vx, head0, &why)?;
                                proposals.observe(x, f64::MIN);
                                continue;
                            }
                        };
                        let mut s = r.score(vx, fam, Some(baseline), params, &enc, false)?;
                        let (margins, objective, legacy) = trade_margins(&s.gain, target)?;
                        proposals.observe(x.clone(), objective);
                        let encoded =
                            sha(&enc.bits.as_ref().ok_or("real encode bytes required")?.data);
                        let fresh = unique.insert(encoded);
                        let found = objective > 0.0;
                        if found && fresh {
                            finds += 1;
                        }
                        if found {
                            r.add_maps(vx, &mut s, &enc)?;
                        }
                        let head = merge(
                            head0,
                            json!({"margins":{"ssim2-frozen":margins[0],"butteraugli":margins[1],"cvvdp":margins[2]},
                                "margin":objective,"find":found,"legacy_margin":legacy,"legacy_find":legacy>0.0,
                                "unique_in_arm":fresh}),
                        );
                        s.disagreement = Some(objective);
                        let name = format!("{}-de-{target}-{arm}-{i:04}", tf.id());
                        let file = r.write_evif(vx, fam, Some(target), &s, &enc, &name, &head)?;
                        r.row(vx, head, &s, &enc, &file)?;
                    }
                    let receipt = json!({"view":vx.view.id,"trade_family":tf.id(),"target":target,"arm":arm,
                        "encodes":spec.de_samples,"unique_finds":finds});
                    save(
                        vx.dir.join(format!("{}-de-{target}-{arm}.json", tf.id())),
                        &receipt,
                    )?;
                }
            }
            timing.insert(leak(format!("de-{}", tf.id())), t.elapsed().as_secs_f64());
        }
    }
    if spec.probe_views.contains(&vx.view.id) {
        let t = Instant::now();
        for tf in [Tf::Ycc444, Tf::Ycc420] {
            let Some(fam) = codec_families.get(tf.codec_family()) else {
                continue;
            };
            let baseline = baselines.get(tf.codec_family()).map(|b| &b.0);
            for m in [
                90.509_668,
                128.0,
                181.019_336,
                256.0,
                362.038_672,
                512.0,
                724.077_344,
                1024.0,
            ] {
                let params = tf.expert(fam.as_ref(), 1.0, m, 95.0)?;
                let head = json!({"kind":"probe","trade_family":tf.id(),"codec_family":tf.codec_family(),"chroma":m});
                let enc = match fam.encode(&params, vx.reference) {
                    Ok(e) => e,
                    Err(why) => {
                        r.refused(vx, head, &why)?;
                        continue;
                    }
                };
                let name = format!("{}-probe-m{m}", tf.id());
                let s = r.score(vx, fam.as_ref(), baseline, params, &enc, false)?;
                let file = r.write_evif(vx, fam.as_ref(), None, &s, &enc, &name, &head)?;
                r.row(vx, head, &s, &enc, &file)?;
            }
        }
        timing.insert("probe", t.elapsed().as_secs_f64());
    }
    Ok(json!(timing))
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(w: u32, h: u32) -> Rgb8Image {
        let mut data = Vec::new();
        let mut state = 0x9e37_79b9_u32;
        for y in 0..h {
            for x in 0..w {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let n = (state >> 24) as i32 % 24;
                let px = [
                    ((x * 255 / w) as i32 + n).clamp(0, 255) as u8,
                    ((y * 255 / h) as i32 + n / 2).clamp(0, 255) as u8,
                    (((x ^ y) * 4) as i32).clamp(0, 255) as u8,
                ];
                data.extend(px);
            }
        }
        Rgb8Image::new(w, h, data).unwrap()
    }
    fn expert() -> Box<dyn Family> {
        Box::new(crate::family::zenjpeg_expert::ZenjpegExpert)
    }
    fn color() -> Box<dyn Family> {
        Box::new(crate::family::zenjpeg_color::ZenjpegColor)
    }

    #[test]
    fn ramps_start_at_the_chroma_only_recipe_and_only_luma_moves() {
        let fam = expert();
        for tf in [Tf::Ycc444, Tf::Ycc420, Tf::XybExtended] {
            let none = tf.ladder(fam.as_ref(), Ramp::None, 5).unwrap();
            let large = tf.ladder(fam.as_ref(), Ramp::Large, 5).unwrap();
            let luma = usize::from(tf == Tf::XybExtended);
            let chroma = if luma == 0 { 1 } else { 0 };
            assert_eq!(none["quant_scale"][luma], mul4(1.0));
            assert_eq!(none["quant_scale"][chroma], large["quant_scale"][chroma]);
            assert!(large["quant_scale"][luma][0].as_f64().unwrap() < 1.0);
            // Rung 0 has no luma gain on any ramp, so the recipes coincide.
            assert_eq!(
                tf.ladder(fam.as_ref(), Ramp::Tiny, 0).unwrap(),
                tf.ladder(fam.as_ref(), Ramp::Large, 0).unwrap()
            );
            // Luma never coarsens and chroma never gets finer along a ramp.
            let k = tf.rungs();
            let mut last_luma = f64::INFINITY;
            for r in 0..k {
                let p = tf.ladder(fam.as_ref(), Ramp::Large, r).unwrap();
                let l = p["quant_scale"][luma][0].as_f64().unwrap();
                assert!(l <= last_luma);
                last_luma = l;
                if r > 0 {
                    assert!(tf.chroma(r) > tf.chroma(r - 1));
                }
            }
            assert!((last_luma - 0.25).abs() < 1e-12);
        }
        assert_eq!(Tf::XybExtended.rungs(), 30);
        assert_eq!(Tf::XybExtended.chroma(29), 1024.0);
        let capped = color();
        let p = Tf::XybCapped
            .ladder(capped.as_ref(), Ramp::Large, 13)
            .unwrap();
        assert_eq!(p["tables"]["chroma_scales"], json!([5.0, 5.0]));
        assert!((p["quality"].as_f64().unwrap() - 99.6).abs() < 1e-9);
        assert_eq!(
            Tf::XybCapped
                .ladder(capped.as_ref(), Ramp::None, 13)
                .unwrap()["quality"],
            json!(95.0)
        );
    }

    #[test]
    fn de_vectors_stay_inside_the_registered_ranges() {
        let fam = expert();
        let lo = Tf::Ycc420.de(fam.as_ref(), &[0.0; 9]).unwrap();
        let hi = Tf::Ycc420.de(fam.as_ref(), &[1.0; 9]).unwrap();
        assert_eq!(lo["quant_scale"][0], mul4(1.0));
        assert_eq!(hi["quant_scale"][0], mul4(0.25));
        assert_eq!(lo["quant_scale"][1][0].as_f64().unwrap(), 1.0 / 64.0);
        assert_eq!(hi["quant_scale"][2][3].as_f64().unwrap(), 64.0);
        let x = Tf::XybExtended.de(fam.as_ref(), &[0.0, 0.0, 1.0]).unwrap();
        assert!((x["quant_scale"][0][0].as_f64().unwrap() - 0.1).abs() < 1e-12);
        assert!((x["quant_scale"][2][0].as_f64().unwrap() - 1024.0).abs() < 1e-9);
        assert!(Tf::Ycc444.de(fam.as_ref(), &[0.5; 3]).is_err());
        assert!(
            Tf::XybCapped
                .de(color().as_ref(), &[0.5, 0.5, 1.1])
                .is_err()
        );
    }

    #[test]
    fn registered_margin_is_the_second_largest_and_two_references_make_a_find() {
        let g = |s: f64, b: f64, c: f64| {
            BTreeMap::from([
                ("t".to_string(), 1.0),
                ("ssim2-frozen".into(), 1.0 - s),
                ("butteraugli".into(), 1.0 - b),
                ("cvvdp".into(), 1.0 - c),
            ])
        };
        let (m, second, legacy) = trade_margins(&g(0.5, -0.2, 0.3), "t").unwrap();
        for (got, want) in m.iter().zip([0.5, -0.2, 0.3]) {
            assert!((got - want).abs() < 1e-12);
        }
        assert!((second - 0.3).abs() < 1e-12);
        assert!((legacy + 0.2).abs() < 1e-12);
        // S alone cannot make a find.
        assert!(trade_margins(&g(2.0, -0.1, -0.1), "t").unwrap().1 < 0.0);
        // B and C agreeing is enough even when S dissents.
        assert!(trade_margins(&g(-1.0, 0.2, 0.1), "t").unwrap().1 > 0.0);
        let mut missing = g(1.0, 1.0, 1.0);
        missing.remove("cvvdp");
        assert!(trade_margins(&missing, "t").is_err());
    }

    #[test]
    fn byte_matching_reaches_the_budget_or_stops_at_the_rate_limit() {
        let img = synthetic(160, 128);
        let fam = expert();
        let target = fam
            .encode(&fam.baseline(55.0).unwrap(), &img)
            .unwrap()
            .bytes()
            .unwrap();
        for tf in [Tf::Ycc420, Tf::XybExtended] {
            let (lo, hi) = tf.bm_quality_range();
            for &rho in tf.bm_rhos() {
                let m = match_bytes(tf, fam.as_ref(), &img, rho, target)
                    .unwrap()
                    .expect("encodable");
                let bytes = m.enc.bytes().unwrap();
                let err = (bytes as f64 / target as f64 - 1.0).abs();
                if tf == Tf::Ycc420 && rho <= 4.0 {
                    assert!(
                        m.matched && err <= 0.01,
                        "{} rho {rho}: {bytes} vs {target}",
                        tf.id()
                    );
                } else if !m.matched {
                    // A luma-heavy cell may be out of reach: the search must then sit on a limit.
                    let floor = bytes as f64 > target as f64 && (m.quality - lo).abs() < 1e-9;
                    let ceiling = (bytes as f64) < target as f64 && (m.quality - hi).abs() < 1e-9;
                    assert!(
                        floor || ceiling,
                        "{} rho {rho}: {bytes} vs {target} at q{}",
                        tf.id(),
                        m.quality
                    );
                }
                assert!(m.evals <= 9);
            }
        }
    }

    #[test]
    fn stable_sampling_is_a_pure_function_of_view_and_name() {
        assert_eq!(stable_sample("a", "b", 20), stable_sample("a", "b", 20));
        assert!(!stable_sample("a", "b", 0));
        let hits = (0..4000)
            .filter(|i| stable_sample("v", &format!("n{i}"), 20))
            .count();
        assert!((120..=280).contains(&hits), "{hits}");
    }
}
