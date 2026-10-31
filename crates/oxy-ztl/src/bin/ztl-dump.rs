//! `ztl-dump` — characterise one file (Milestone 0 instrument).
//!
//! USAGE: `ztl-dump FILE [--extract DIR] [--strings N]`
//!
//! Prints: size, header hex dump, Layer-1 records (gaps and zlib streams),
//! entropy profile, known signatures, strings, and a verdict with the next
//! step. `--extract DIR` writes each decompressed stream to
//! `DIR/stream_<index>_at_<offset>.bin` for closer inspection.
//!
//! Only run this on files you have the right to analyse
//! (docs/formats/zbrush/LEGAL.md). Share findings, never the files.
//!
//! EXIT: 0 ok · 2 usage or I/O error.

use oxy_ztl::probe::{self, BLOCK, HIGH_ENTROPY};
use oxy_ztl::{Record, ZtlDocument};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut extract: Option<PathBuf> = None;
    let mut max_strings = 40usize;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--extract" => extract = args.next().map(PathBuf::from),
            "--strings" => max_strings = args.next().and_then(|n| n.parse().ok()).unwrap_or(max_strings),
            "-h" | "--help" => {
                println!("Usage: ztl-dump FILE [--extract DIR] [--strings N]");
                return;
            }
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let Some(file) = file else {
        eprintln!("Usage: ztl-dump FILE [--extract DIR] [--strings N]");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ztl-dump: {}: {e}", file.display());
            std::process::exit(2);
        }
    };
    let doc = ZtlDocument::read(&bytes);
    let r = probe::probe(&bytes, &doc);

    println!("file      {}", file.display());
    println!("size      {} bytes", r.len);
    println!("entropy   {:.3} bits/byte overall", r.overall_entropy);
    println!("\n== header (first {} bytes) ==", r.head.len());
    print!("{}", probe::hexdump(&r.head, 0));

    println!("\n== Layer-1 records ({}) ==", doc.records.len());
    println!("{:>4}  {:>10}  {:>10}  {:<6}  detail", "#", "offset", "size", "kind");
    for (i, rec) in doc.records.iter().enumerate() {
        match rec {
            Record::Gap { offset, bytes } => {
                println!("{i:>4}  {offset:>#10x}  {:>10}  gap     entropy {:.2}", bytes.len(), probe::entropy(bytes))
            }
            Record::ZlibStream { offset, raw, decoded, .. } => println!(
                "{i:>4}  {offset:>#10x}  {:>10}  zlib    -> {} bytes decoded (entropy {:.2}), header {:02x} {:02x}",
                raw.len(),
                decoded.len(),
                probe::entropy(decoded),
                raw[0],
                raw[1]
            ),
        }
    }
    println!(
        "coverage  {} streams: {} bytes on disk ({:.1}%), {} decoded; {} gap bytes, {:.0}% of them high-entropy",
        r.stream_count,
        r.stream_bytes,
        100.0 * r.stream_bytes as f64 / r.len.max(1) as f64,
        r.decoded_bytes,
        r.gap_bytes,
        100.0 * r.high_entropy_gap_fraction
    );

    println!("\n== entropy profile ({BLOCK}-byte blocks, ' '=0 … '@'=8 bits/byte; high >= {HIGH_ENTROPY}) ==");
    println!("[{}]", probe::entropy_strip(&r.block_entropy, 96));

    println!("\n== known signatures ({}) ==", r.signatures.len());
    for h in r.signatures.iter().take(50) {
        println!("{:>#10x}  {}", h.offset, h.name);
    }

    println!("\n== strings (first {max_strings} of {}) ==", r.strings.len());
    for s in r.strings.iter().take(max_strings) {
        let text: String = s.text.chars().take(80).collect();
        println!("{:>#10x}  {}  {text}", s.offset, if s.utf16 { "u16" } else { "asc" });
    }

    println!("\n== verdict: {:?} ==\n{}", r.verdict, r.verdict.explain());

    if let Some(dir) = extract {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("ztl-dump: {}: {e}", dir.display());
            std::process::exit(2);
        }
        for (i, off, data) in doc.streams() {
            let p = dir.join(format!("stream_{i:04}_at_{off:#x}.bin"));
            if let Err(e) = std::fs::write(&p, data) {
                eprintln!("ztl-dump: {}: {e}", p.display());
                std::process::exit(2);
            }
        }
        println!("\nextracted {} streams to {}", r.stream_count, dir.display());
    }
}
