//! `ztl-diff` — compare a diff pair at every layer
//! (docs/formats/zbrush/REVERSE_ENGINEERING.md §1).
//!
//! USAGE: `ztl-diff A B [--max N]`
//!
//! Layer 0: changed raw-byte regions (hex excerpts of both sides).
//! Layer 1/2: zlib streams paired by order; for changed streams, the
//! changed regions inside the decompressed content.
//!
//! EXIT: 0 identical · 1 different · 2 usage or I/O error.

use oxy_ztl::diff::{StreamDiff, diff};
use oxy_ztl::probe::hexdump;

fn excerpt(bytes: &[u8], off: usize, len: usize) -> String {
    let end = (off + len.min(32)).min(bytes.len());
    hexdump(&bytes[off.min(bytes.len())..end], off)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut max = 30usize;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--max" {
            max = it.next().and_then(|n| n.parse().ok()).unwrap_or(max);
        } else {
            files.push(a.clone());
        }
    }
    if files.len() != 2 {
        eprintln!("Usage: ztl-diff A B [--max N]");
        std::process::exit(2);
    }
    let read = |p: &str| {
        std::fs::read(p).unwrap_or_else(|e| {
            eprintln!("ztl-diff: {p}: {e}");
            std::process::exit(2)
        })
    };
    let (a, b) = (read(&files[0]), read(&files[1]));
    let r = diff(&a, &b);

    println!("A {} ({} bytes)\nB {} ({} bytes)", files[0], r.len_a, files[1], r.len_b);
    if r.identical() {
        println!("identical");
        return;
    }

    println!("\n== Layer 0: {} changed raw region(s) ==", r.regions.len());
    for g in r.regions.iter().take(max) {
        println!("A[{:#x}..+{}]  B[{:#x}..+{}]", g.a_offset, g.a_len, g.b_offset, g.b_len);
        print!("  A: {}", excerpt(&a, g.a_offset, g.a_len).replace('\n', "\n     "));
        println!();
        print!("  B: {}", excerpt(&b, g.b_offset, g.b_len).replace('\n', "\n     "));
        println!();
    }
    if r.regions.len() > max {
        println!("… {} more (use --max)", r.regions.len() - max);
    }

    println!("\n== Layer 1/2: zlib streams ==");
    if r.streams.is_empty() {
        println!("no zlib streams in either file");
    }
    for s in &r.streams {
        match s {
            StreamDiff::Same { index, a_offset, b_offset, len } => {
                println!("#{index:<3} same      {len} bytes  (A@{a_offset:#x}, B@{b_offset:#x})")
            }
            StreamDiff::Changed { index, a_offset, b_offset, a_len, b_len, regions } => {
                println!(
                    "#{index:<3} CHANGED   {a_len} -> {b_len} bytes  (A@{a_offset:#x}, B@{b_offset:#x}); {} region(s) in decoded data:",
                    regions.len()
                );
                for g in regions.iter().take(max) {
                    println!("       decoded A[{:#x}..+{}]  B[{:#x}..+{}]", g.a_offset, g.a_len, g.b_offset, g.b_len);
                }
            }
            StreamDiff::OnlyInA { index, offset, len } => println!("#{index:<3} only in A {len} bytes @ {offset:#x}"),
            StreamDiff::OnlyInB { index, offset, len } => println!("#{index:<3} only in B {len} bytes @ {offset:#x}"),
        }
    }
    std::process::exit(1);
}
