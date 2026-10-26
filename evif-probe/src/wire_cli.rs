//! Major-1 transport tools. Codec/sample verification is a separate operation.
use crate::Res;
use evif::wire::{self, Asset, Limits, Segment, Storage, Writer};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

const HELP: &str = "evif wire inspect FILE | pack PLAN.json OUTPUT.evif | append FILE SCORE.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    assets: Vec<Asset>,
    segments: Vec<PlannedSegment>,
    #[serde(default)]
    bitstreams: Vec<PlannedBits>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedSegment {
    segment: Segment,
    path: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedBits {
    asset: u32,
    codec: String,
    path: PathBuf,
}

fn small_file(path: &Path, limit: u64) -> Res<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("JSON file exceeds configured limit".into());
    }
    Ok(bytes)
}

pub(crate) fn run(args: &[String]) -> Res<()> {
    match args {
        [cmd, file] if cmd == "inspect" => {
            let mut source = BufReader::new(File::open(file).map_err(|e| e.to_string())?);
            let index = wire::read(&mut source, Limits::default()).map_err(|e| e.to_string())?;
            println!(
                "wire 1: {} assets, {} segments, {} original bitstreams, {} evaluations",
                index.assets.len(),
                index.segments.len(),
                index.bitstreams.len(),
                index.scores.len()
            );
            for root in &index.roots {
                let a = &index.assets[&root.asset];
                println!(
                    "  asset {}: {}x{} {:?} {:?} {:?}/{:?}, {} frames, media {}",
                    a.id,
                    a.width,
                    a.height,
                    a.channels,
                    a.samples,
                    a.primaries,
                    a.transfer,
                    a.frame_count.0,
                    root.hash
                );
            }
            println!(
                "container and declared embedded payload hashes checked; decoded samples not verified"
            );
            Ok(())
        }
        [cmd, plan, output] if cmd == "pack" => pack(Path::new(plan), Path::new(output)),
        [cmd, file, score] if cmd == "append" => {
            let bytes = small_file(Path::new(score), Limits::default().max_json_bytes as u64)?;
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(file)
                .map_err(|e| e.to_string())?;
            wire::append_record(&mut file, *b"scor", &bytes, Limits::default())
                .map_err(|e| e.to_string())
        }
        _ => Err(HELP.into()),
    }
}

fn pack(plan_path: &Path, output: &Path) -> Res<()> {
    let bytes = small_file(plan_path, Limits::default().max_total_json_bytes)?;
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let base = plan_path.parent().unwrap_or(Path::new("."));
    // Avoid creating output for missing inputs or ambiguous embedded/external plans.
    for p in &plan.segments {
        match (&p.segment.storage, &p.path) {
            (Storage::Embedded { .. } | Storage::EmbeddedPages { .. }, Some(path)) => {
                fs::metadata(base.join(path)).map_err(|e| e.to_string())?;
            }
            (Storage::External { .. } | Storage::SharedPages { .. }, None) => {}
            _ => {
                return Err("embedded segments need path; external segments must omit path".into());
            }
        }
    }
    for b in &plan.bitstreams {
        if b.codec.len() != 4 || !b.codec.is_ascii() {
            return Err("codec must be four ASCII bytes".into());
        }
        fs::metadata(base.join(&b.path)).map_err(|e| e.to_string())?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| format!("{}: {e}", output.display()))?;
    let result = (|| -> Result<(), wire::Error> {
        let mut writer = Writer::new(file, Limits::default())?;
        for asset in &plan.assets {
            writer.asset(asset)?;
        }
        for p in &plan.segments {
            if let Some(path) = &p.path {
                let mut source = File::open(base.join(path))?;
                let len = source.metadata()?.len();
                writer.segment(&p.segment, &mut source, len)?;
            } else {
                writer.segment(&p.segment, &mut std::io::empty(), 0)?;
            }
        }
        for bits in &plan.bitstreams {
            let mut source = File::open(base.join(&bits.path))?;
            let len = source.metadata()?.len();
            writer.bitstream(
                bits.asset,
                bits.codec.as_bytes().try_into().unwrap(),
                &mut source,
                len,
            )?;
        }
        let roots = writer.finish_media()?;
        writer.finish()?;
        for root in roots {
            println!("asset {} media {}", root.asset, root.hash);
        }
        Ok(())
    })();
    result.map_err(|e| format!("{e}; partial output retained at {}", output.display()))
}
