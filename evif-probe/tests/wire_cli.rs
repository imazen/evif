use evif::wire::*;
use evif_probe::{cli, image::Rgb8Image, registry::Registry};
use std::fs;

#[test]
fn pack_and_append_preserve_payloads_and_refuse_overwrites() {
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("wire-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let image = Rgb8Image::new(3, 2, (0..18).collect()).unwrap();
    let png = image.to_png().unwrap();
    fs::write(root.join("input.png"), &png).unwrap();
    let a = Asset {
        id: 1,
        profile: "resolved-rgb/1".into(),
        kind: Kind::Still,
        width: 3,
        height: 2,
        channels: Channels::Rgb,
        samples: Samples::U8,
        primaries: Primaries::Bt709,
        transfer: Transfer::Srgb,
        domain: Domain::DisplayRelative,
        nits_per_unit: None,
        pixel_aspect: Rational::new(1, 1).unwrap(),
        frame_count: U64(1),
        plays: None,
    };
    let s = Segment {
        asset: 1,
        first_frame: U64(0),
        carrier: Carrier::Png,
        frames: vec![Frame {
            duration: None,
            samples: frame_hash(&a, &image.data, 9).unwrap(),
        }],
        storage: Storage::Embedded {
            hash: Some(Hash::of(&png)),
        },
    };
    let plan = serde_json::json!({"assets":[a], "segments":[{"segment":s,"path":"input.png"}],
        "bitstreams":[{"info":{"asset":1,"codec":"png ","format":"image/png",
            "version":{"kind":"unversioned"},"profile":null,"level":null,"parameters":{},
            "bytes":png.len().to_string(),"hash":Hash::of(&png)},"path":"input.png"}]});
    let plan_path = root.join("plan.json");
    fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let output = root.join("out.evif");
    let reg = Registry::builtin();
    let args = [
        "wire".into(),
        "pack".into(),
        plan_path.display().to_string(),
        output.display().to_string(),
    ];
    cli::run(&reg, &args).unwrap();
    let before = fs::read(&output).unwrap();
    assert!(cli::run(&reg, &args).is_err());
    assert_eq!(fs::read(&output).unwrap(), before);
    let index = scan(&before, Limits::default()).unwrap();
    assert_eq!(index.bitstream_info[&1].format, "image/png");
    assert_eq!(index.bitstream_info[&1].version, SyntaxVersion::Unversioned);
    let mut omitted_plan = plan.clone();
    omitted_plan["bitstreams"][0]["path"] = serde_json::Value::Null;
    omitted_plan["bitstreams"][0]["info"]["hash"] = serde_json::Value::Null;
    let omitted_plan_path = root.join("omitted-plan.json");
    fs::write(
        &omitted_plan_path,
        serde_json::to_vec(&omitted_plan).unwrap(),
    )
    .unwrap();
    let omitted_output = root.join("omitted.evif");
    cli::run(
        &reg,
        &[
            "wire".into(),
            "pack".into(),
            omitted_plan_path.display().to_string(),
            omitted_output.display().to_string(),
        ],
    )
    .unwrap();
    let omitted = scan(&fs::read(&omitted_output).unwrap(), Limits::default()).unwrap();
    assert!(omitted.bitstreams.is_empty());
    assert_eq!(omitted.bitstream_info[&1].bytes.0, png.len() as u64);
    assert_eq!(omitted.roots, index.roots);
    let p = index.segments[0].payload.unwrap();
    let embedded = &before[p.offset as usize..(p.offset + p.length) as usize];
    assert_eq!(embedded, png);
    let decoded = Rgb8Image::from_png(embedded).unwrap();
    assert_eq!(
        frame_hash(&a, &decoded.data, 9).unwrap(),
        s.frames[0].samples
    );
    let score = ScoreRecord {
        schema: 1,
        id: "test".into(),
        supersedes: None,
        candidate: index.roots[0].hash.clone(),
        reference: None,
        adapter: "example/1".into(),
        data: serde_json::json!({"status":"not-scored","number":0.000001}),
    };
    let score_json = serde_json::to_string_pretty(&score)
        .unwrap()
        .replace("1e-6", "0.000001");
    let score_path = root.join("score.json");
    fs::write(&score_path, &score_json).unwrap();
    let append = [
        "wire".into(),
        "append".into(),
        output.display().to_string(),
        score_path.display().to_string(),
    ];
    cli::run(&reg, &append).unwrap();
    let after = fs::read(&output).unwrap();
    assert_eq!(&after[..before.len()], before);
    let got = scan(&after, Limits::default()).unwrap();
    let p = got.score_spans[0];
    assert_eq!(
        &after[p.offset as usize..(p.offset + p.length) as usize],
        score_json.as_bytes()
    );
    assert!(cli::run(&reg, &append).is_err());
    assert_eq!(fs::read(&output).unwrap(), after);
    cli::run(
        &reg,
        &[
            "wire".into(),
            "inspect".into(),
            output.display().to_string(),
        ],
    )
    .unwrap();
}
