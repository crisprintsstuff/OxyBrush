//! Real-file corpus tests (Layers 0–2), following SWFormat's harness model.
//!
//! WHAT THIS PINS DOWN: for every corpus file that exists locally,
//! - the Layer-1 parse reconstructs the file byte-for-byte (no orphan bytes,
//!   lazy round-trip), and
//! - re-compressing every zlib stream keeps every decoded stream identical.
//!
//! WHERE FILES COME FROM: `OXY_ZTL_CORPUS` (a private registry with your own
//! absolute paths) or, by default, the committed template
//! `test/corpus/zbrush/corpus.config.json`. Missing paths are skipped, so CI
//! (which has no ZBrush files) passes trivially. See
//! `docs/formats/zbrush/LEGAL.md` §5 for the corpus rules.

use oxy_ztl::{Record, ZtlDocument};
use std::path::PathBuf;

fn registry() -> PathBuf {
    std::env::var_os("OXY_ZTL_CORPUS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/corpus/zbrush/corpus.config.json")
    })
}

fn entries() -> Vec<(String, PathBuf)> {
    let path = registry();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).expect("corpus registry is valid JSON");
    let base = path.parent().map(PathBuf::from).unwrap_or_default();
    json["files"]
        .as_array()
        .expect("`files` array")
        .iter()
        .map(|e| {
            let tag = e["tag"].as_str().unwrap_or("?").to_string();
            let p = PathBuf::from(e["path"].as_str().unwrap_or(""));
            (tag, if p.is_absolute() { p } else { base.join(p) })
        })
        .collect()
}

#[test]
fn registry_is_valid() {
    for e in serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(registry()).unwrap()).unwrap()["files"]
        .as_array()
        .unwrap()
    {
        for key in ["tag", "path", "file_type", "license_status"] {
            assert!(e[key].is_string(), "corpus entry missing `{key}`: {e}");
        }
        let status = e["license_status"].as_str().unwrap();
        assert!(status == "owner" || status == "synthetic", "bad license_status {status}");
    }
}

#[test]
fn corpus_layer1_echo_and_layer2_recompress() {
    let mut ran = 0;
    for (tag, path) in entries() {
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("skip {tag}: {} not present", path.display());
            continue;
        };
        ran += 1;
        let doc = ZtlDocument::read(&bytes);
        assert_eq!(doc.reconstruct(), bytes, "{tag}: lazy echo must be byte-identical");

        let mut re = doc.clone();
        for r in &mut re.records {
            if let Record::ZlibStream { decoded, modified, .. } = r {
                *modified = Some(decoded.clone());
            }
        }
        let again = ZtlDocument::read(&re.reconstruct());
        let a: Vec<&[u8]> = doc.streams().map(|s| s.2).collect();
        let b: Vec<&[u8]> = again.streams().map(|s| s.2).collect();
        assert_eq!(a, b, "{tag}: decoded streams must survive re-compression");
        eprintln!("ok   {tag}: {} bytes, {} streams", bytes.len(), a.len());
    }
    eprintln!("corpus files tested: {ran}");
}
