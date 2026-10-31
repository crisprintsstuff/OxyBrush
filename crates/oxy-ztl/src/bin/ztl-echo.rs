//! `ztl-echo` — Layer-1 self-consistency check (SWFormat's M1 instrument,
//! adapted).
//!
//! USAGE: `ztl-echo FILE [--recompress] [--out PATH]`
//!
//! - default (lazy): read → write unmodified. MUST be byte-identical;
//!   a failure is a P0 bug in the record model.
//! - `--recompress`: re-compress every zlib stream from its own decoded
//!   content. Raw bytes may differ (encoder differences); the decoded
//!   streams MUST be identical. Writing the result with `--out` produces a
//!   file for a manual Layer-3 check (does ZBrush accept our DEFLATE
//!   output?). Never overwrite the original.
//!
//! EXIT: 0 faithful · 1 mismatch · 2 usage or I/O error.

use oxy_ztl::{Record, ZtlDocument};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut file, mut out, mut recompress) = (None, None, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--recompress" => recompress = true,
            "--out" => out = args.next().map(PathBuf::from),
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let Some(file) = file else {
        eprintln!("Usage: ztl-echo FILE [--recompress] [--out PATH]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&file).unwrap_or_else(|e| {
        eprintln!("ztl-echo: {}: {e}", file.display());
        std::process::exit(2)
    });
    let mut doc = ZtlDocument::read(&bytes);
    if recompress {
        for r in &mut doc.records {
            if let Record::ZlibStream { decoded, modified, .. } = r {
                *modified = Some(decoded.clone());
            }
        }
    }
    let written = doc.reconstruct();
    if let Some(p) = &out {
        if p.canonicalize().ok() == file.canonicalize().ok() {
            eprintln!("ztl-echo: refusing to overwrite the input file");
            std::process::exit(2);
        }
        if let Err(e) = std::fs::write(p, &written) {
            eprintln!("ztl-echo: {}: {e}", p.display());
            std::process::exit(2);
        }
    }
    let ok = if recompress {
        let again = ZtlDocument::read(&written);
        let a: Vec<&[u8]> = doc.streams().map(|s| s.2).collect();
        let b: Vec<&[u8]> = again.streams().map(|s| s.2).collect();
        let same = a == b;
        println!(
            "recompress: {} streams, raw {} -> {} bytes, decoded streams {}",
            a.len(),
            bytes.len(),
            written.len(),
            if same { "identical" } else { "DIFFER" }
        );
        same
    } else {
        let same = written == bytes;
        println!("lazy echo: {} records, {}", doc.records.len(), if same { "byte-identical" } else { "MISMATCH" });
        same
    };
    std::process::exit(if ok { 0 } else { 1 });
}
