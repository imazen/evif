//! Paired source bootstrap of the G-TRADE tallies (`evif trade-bootstrap`), through the
//! zenmetrics-stats percentile bootstrap. Definitions: `docs/trade-ladders.md`.
use crate::Res;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use zenmetrics_stats::resample::{BootstrapConfig, Design, cluster_bootstrap};

type Counts = BTreeMap<String, f64>;
/// model -> source -> counters
type PerModel = BTreeMap<String, BTreeMap<String, Counts>>;

/// The two counters whose pooled ratio is the rate of a metric.
fn counters(metric: &str) -> (&'static str, &'static str, f64) {
    if metric.starts_with('a') {
        ("inv", "n_worse", 1.0)
    } else {
        ("finds", "encodes", 1000.0)
    }
}

fn pooled(weights: &[f64], rows: &[&Counts], num: &str, den: &str, scale: f64) -> f64 {
    let (mut n, mut d) = (0.0, 0.0);
    for (w, c) in weights.iter().zip(rows) {
        n += w * c.get(num).copied().unwrap_or(0.0);
        d += w * c.get(den).copied().unwrap_or(0.0);
    }
    if d > 0.0 { scale * n / d } else { f64::NAN }
}

fn interval(i: &zenmetrics_stats::resample::Interval) -> Value {
    json!({"estimate":i.estimate,"lo":i.lo,"hi":i.hi,"se":i.se,"n_finite":i.n_finite,
        "p_zero":i.p_zero})
}

pub(crate) fn run(args: &[String]) -> Res<()> {
    let [tallies, out] = args else {
        return Err("usage: evif trade-bootstrap TALLIES.json NEW_OUTPUT.json".into());
    };
    let text = std::fs::read_to_string(tallies).map_err(|e| format!("{tallies}: {e}"))?;
    let root: BTreeMap<String, BTreeMap<String, PerModel>> =
        serde_json::from_str(&text).map_err(|e| format!("tallies: {e}"))?;
    let cfg = BootstrapConfig {
        resamples: 10_000,
        seed: 20_261_030,
        level: 0.95,
    };
    let baseline = "zensim-e";
    let mut result = BTreeMap::new();
    for (partition, metrics) in &root {
        let mut by_metric = BTreeMap::new();
        for (metric, models) in metrics {
            let (num, den, scale) = counters(metric);
            let mut rows = BTreeMap::new();
            for (model, sources) in models {
                // Absolute rate of this model.
                let names: Vec<&String> = sources.keys().collect();
                let labels: Vec<u32> = (0..names.len() as u32).collect();
                let cs: Vec<&Counts> = names.iter().map(|n| &sources[*n]).collect();
                if cs.is_empty() {
                    continue;
                }
                let abs = cluster_bootstrap(cs.len(), &Design::OneWay(&labels), &cfg, |d| {
                    vec![pooled(d.row_weights(), &cs, num, den, scale)]
                })
                .map_err(|e| format!("bootstrap: {e:?}"))?;
                let mut entry = json!({"sources":cs.len(),"absolute":interval(&abs[0]),
                    "numerator":cs.iter().map(|c| c.get(num).copied().unwrap_or(0.0)).sum::<f64>(),
                    "denominator":cs.iter().map(|c| c.get(den).copied().unwrap_or(0.0)).sum::<f64>()});
                // Paired difference against the baseline over the sources both scored.
                if model != baseline
                    && let Some(base) = models.get(baseline)
                {
                    let shared: Vec<&String> = names
                        .iter()
                        .copied()
                        .filter(|n| base.contains_key(*n))
                        .collect();
                    let labels: Vec<u32> = (0..shared.len() as u32).collect();
                    let mine: Vec<&Counts> = shared.iter().map(|n| &sources[*n]).collect();
                    let theirs: Vec<&Counts> = shared.iter().map(|n| &base[*n]).collect();
                    if !shared.is_empty() {
                        let diff =
                            cluster_bootstrap(shared.len(), &Design::OneWay(&labels), &cfg, |d| {
                                let w = d.row_weights();
                                let (a, b) = (
                                    pooled(w, &mine, num, den, scale),
                                    pooled(w, &theirs, num, den, scale),
                                );
                                vec![a - b, a, b]
                            })
                            .map_err(|e| format!("bootstrap: {e:?}"))?;
                        let verdict = if diff[0].lo > 0.0 {
                            "worse than E (interval entirely above zero)"
                        } else if diff[0].hi < 0.0 {
                            "better than E (interval entirely below zero)"
                        } else {
                            "not distinguishable from E"
                        };
                        entry["paired_vs_e"] = json!({"sources":shared.len(),"candidate_minus_e":interval(&diff[0]),
                            "candidate":interval(&diff[1]),"e":interval(&diff[2]),"verdict":verdict});
                    }
                }
                rows.insert(model.clone(), entry);
            }
            by_metric.insert(metric.clone(), rows);
        }
        result.insert(partition.clone(), by_metric);
    }
    let body = json!({"resamples":cfg.resamples,"seed":cfg.seed,"level":cfg.level,"baseline":baseline,
        "design":"one-way bootstrap over source identities, pooled ratio of sums","results":result});
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out)
        .map_err(|e| format!("{out}: {e}"))?;
    std::io::Write::write_all(
        &mut file,
        serde_json::to_string_pretty(&body)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooled_rate_weights_sources_by_multiplicity() {
        let a: Counts = [("inv".into(), 1.0), ("n_worse".into(), 4.0)].into();
        let b: Counts = [("inv".into(), 3.0), ("n_worse".into(), 4.0)].into();
        assert_eq!(pooled(&[1.0, 1.0], &[&a, &b], "inv", "n_worse", 1.0), 0.5);
        assert_eq!(pooled(&[2.0, 0.0], &[&a, &b], "inv", "n_worse", 1.0), 0.25);
        assert!(pooled(&[0.0, 0.0], &[&a, &b], "inv", "n_worse", 1.0).is_nan());
    }

    #[test]
    fn identical_models_have_an_interval_around_zero_and_a_worse_model_is_flagged() {
        let dir = std::env::temp_dir().join(format!("evif-bootstrap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |worse: f64| -> Value {
            let mut sources = serde_json::Map::new();
            for i in 0..30 {
                sources.insert(
                    format!("s{i}"),
                    json!({"inv": ((i % 3) as f64 + worse), "n_worse": 6.0}),
                );
            }
            Value::Object(sources)
        };
        let tallies = json!({"gauntlet":{"a":{"zensim-e":mk(0.0),"same":mk(0.0),"worse":mk(2.0)}}});
        let (input, output) = (dir.join("t.json"), dir.join("o.json"));
        std::fs::write(&input, tallies.to_string()).unwrap();
        run(&[input.display().to_string(), output.display().to_string()]).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        let r = &v["results"]["gauntlet"]["a"];
        assert_eq!(
            r["same"]["paired_vs_e"]["candidate_minus_e"]["estimate"],
            0.0
        );
        assert!(
            r["worse"]["paired_vs_e"]["verdict"]
                .as_str()
                .unwrap()
                .starts_with("worse")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
