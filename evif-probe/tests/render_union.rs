//! Batched transport must preserve the existing container bytes.
use evif_probe::{image::Rgb8Image, registry::Registry};
use std::fs;
fn run(args: &[String]) {
    evif_probe::cli::run(&Registry::builtin(), args).unwrap();
}
#[test]
fn union_and_bulk_scores_are_byte_identical_to_single_commands() {
    let root = std::env::temp_dir().join(format!("evif-union-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let data = (0..32 * 32 * 3)
        .map(|i| ((i * 37 + i / 19) % 256) as u8)
        .collect();
    let image = Rgb8Image::new(32, 32, data).unwrap();
    let input = root.join("view.png");
    fs::write(&input, image.to_png().unwrap()).unwrap();
    let registry = Registry::builtin();
    let mut union = String::new();
    let mut originals = Vec::new();
    for name in ["zenjpeg-expert", "zenjpeg-color"] {
        let params = registry.family(name).unwrap().baseline(95.).unwrap();
        let file = root.join(format!("{name}.jsonl"));
        fs::write(&file, format!("{params}\n")).unwrap();
        let out = root.join(name);
        run(&[
            "render".into(),
            "--family".into(),
            name.into(),
            "--params".into(),
            file.display().to_string(),
            "--format".into(),
            "evif".into(),
            "--out".into(),
            out.display().to_string(),
            input.display().to_string(),
        ]);
        originals.push(out.join(format!("view-{name}-p0000.evif")));
        union += &format!("{}\n", serde_json::json!({"family":name,"params":params}));
    }
    let params = root.join("union.jsonl");
    fs::write(&params, union).unwrap();
    let out = root.join("union");
    run(&[
        "render".into(),
        "--family".into(),
        "union".into(),
        "--params".into(),
        params.display().to_string(),
        "--format".into(),
        "evif".into(),
        "--emit-reference-rgb8".into(),
        "--out".into(),
        out.display().to_string(),
        input.display().to_string(),
    ]);
    assert_eq!(
        fs::read(out.join("view.reference.rgb8")).unwrap(),
        image.data
    );
    for (i, path) in originals.iter().enumerate() {
        let batched = out.join(format!("view-union-p{i:04}.evif"));
        assert_eq!(fs::read(path).unwrap(), fs::read(&batched).unwrap());
        let file = evif::Evif::from_bytes(&fs::read(path).unwrap()).unwrap();
        let envelope = serde_json::json!({"reference_digest":format!("{:016x}",file.digests.reference.unwrap()),"distorted_digest":format!("{:016x}",file.digests.distorted),"scores":[{"metric":"ssim2","implementation":"fixture@1","value":83.125,"better":"higher"}]});
        let scores = root.join(format!("scores{i}.json"));
        fs::write(&scores, envelope.to_string()).unwrap();
        let single = root.join(format!("single{i}.evif"));
        let bulk = root.join(format!("bulk{i}.evif"));
        run(&[
            "score-import".into(),
            path.display().to_string(),
            scores.display().to_string(),
            single.display().to_string(),
        ]);
        let manifest = root.join(format!("imports{i}.json"));
        fs::write(
            &manifest,
            serde_json::json!([{"input":batched,"scores":scores,"output":bulk}]).to_string(),
        )
        .unwrap();
        run(&["score-import-many".into(), manifest.display().to_string()]);
        assert_eq!(fs::read(single).unwrap(), fs::read(bulk).unwrap());
    }
    fs::remove_dir_all(root).unwrap();
}
