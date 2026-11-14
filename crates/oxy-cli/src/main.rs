//! `oxybrush-cli`: headless OxyBrush operations for scripts, build farms and
//! pipelines. No window, no GPU. The `mcp` command speaks to a local agent
//! over stdio, or attaches to the desktop app on 127.0.0.1; it never dials
//! any other address.
//!
//! Inputs: `.oxb` projects (visible pieces), glTF (`.glb`/`.gltf`), OBJ, STL, PLY. Outputs: `.oxb`,
//! glTF (`.glb`/`.gltf`), OBJ, STL, PLY. Output files are written
//! atomically; an input file is never overwritten in place.

use oxy_doc::{Piece, PieceData, SceneData, format};
use oxy_io::ExportPiece;
use oxy_mesh::Mesh;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
oxybrush-cli: headless OxyBrush tools

USAGE:
  oxybrush-cli info     <input>
  oxybrush-cli convert  <input> <output>
  oxybrush-cli remesh   <input> <output> [--resolution N]        (8-320, default 128)
  oxybrush-cli decimate <input> <output> (--keep PERCENT | --triangles N)
  oxybrush-cli lod      <input> <output-prefix> [--levels N] [--format EXT]
  oxybrush-cli split    <input> <output-folder> [--by groups|parts] [--format EXT]
  oxybrush-cli mcp      [file]            stdio MCP server for a local agent (no socket)
  oxybrush-cli mcp      --attach 127.0.0.1:PORT --token TOKEN
  oxybrush-cli addon    pack <plugin-folder> <out.oxyaddon>
  oxybrush-cli addon    inspect <file.oxyaddon>

Inputs:  .oxb project (visible pieces), .glb, .gltf, .obj, .stl, .ply
Outputs: .oxb, .glb, .gltf, .obj, .stl, .ply (by extension; EXT defaults to glb)
Any command writing .glb/.gltf takes --compress meshopt|quantize|none:
  quantize  16-bit positions, 8-bit normals (KHR_mesh_quantization), about half
  meshopt   quantized + EXT_meshopt_compression, usually a fifth or less
Readers must support the extension (three.js, Babylon.js, Blender do).

`mcp` writes the protocol to stdout. Do not mix it with other commands.
It refuses to attach to anything but 127.0.0.1 or localhost. See docs/mcp.md.

Examples:
  oxybrush-cli convert head.oxb head.glb
  oxybrush-cli convert head.oxb head-small.glb --compress meshopt
  oxybrush-cli decimate scan.stl scan-light.obj --keep 10
  oxybrush-cli lod hero.oxb out/hero --levels 4        writes out/hero_LOD0.glb ... out/hero_LOD4.glb
";

fn describe_addon(info: &oxy_plugin::addon::AddonInfo) -> String {
    let kind = match info.kind {
        oxy_plugin::addon::AddonKind::Wasm => "in-app (WebAssembly) plugin".to_owned(),
        oxy_plugin::addon::AddonKind::Python => {
            format!("Python plugin, runs {}", info.entry.as_deref().unwrap_or("main.py"))
        }
    };
    format!("{} {} [{}]: {kind}; installs to plugins/{}/", info.name, info.version, info.id, info.id)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(msg) => {
            // `mcp` speaks the protocol on stdout itself and returns nothing to print.
            if !msg.is_empty() {
                println!("{msg}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("oxybrush-cli: {e}");
            if e.contains("usage") {
                eprintln!("\n{USAGE}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Run one command; returns the message to print.
fn run(args: &[String]) -> Result<String, String> {
    let Some(cmd) = args.first() else { return Ok(USAGE.to_string()) };
    let pos: Vec<&str> = positional(&args[1..]);
    let export = export_options(args)?;
    let need = |n: usize| -> Result<(), String> {
        if pos.len() == n { Ok(()) } else { Err(format!("`{cmd}` takes {n} file argument(s) (see usage)")) }
    };
    match cmd.as_str() {
        "-h" | "--help" | "help" => Ok(USAGE.to_string()),
        "-V" | "--version" => Ok(format!("oxybrush-cli {}", env!("CARGO_PKG_VERSION"))),
        "info" => {
            need(1)?;
            let pieces = load(Path::new(pos[0]))?;
            let mut out = format!("{}: {} piece(s)\n", pos[0], pieces.len());
            for p in &pieces {
                let b = p.mesh.bounds();
                let size = b.max - b.min;
                out += &format!(
                    "  {}: {} triangles, {} vertices, size {:.3} x {:.3} x {:.3}{}\n",
                    p.name,
                    p.mesh.triangle_count(),
                    p.mesh.vertex_count(),
                    size.x,
                    size.y,
                    size.z,
                    p.multires.as_ref().map_or(String::new(), |m| format!(", {} subdivision levels", m.level_count()))
                );
            }
            Ok(out.trim_end().to_string())
        }
        "convert" => {
            need(2)?;
            let pieces = load(Path::new(pos[0]))?;
            save(&pieces, Path::new(pos[1]), Path::new(pos[0]), export)
        }
        "remesh" => {
            need(2)?;
            let resolution = flag_num(args, "--resolution")?.unwrap_or(128.0) as u32;
            let adaptivity = flag_num(args, "--adaptivity")?.unwrap_or(0.0) as f32;
            let opts = oxy_topo::RemeshOptions { resolution, smooth_passes: 1, adaptivity, ..Default::default() };
            let pieces = map_meshes(load(Path::new(pos[0]))?, |m| {
                oxy_topo::voxel_remesh(&[m], opts).map_err(|e| e.to_string())
            })?;
            save(&pieces, Path::new(pos[1]), Path::new(pos[0]), export)
        }
        "decimate" => {
            need(2)?;
            let keep = flag_num(args, "--keep")?;
            let tris = flag_num(args, "--triangles")?;
            let target = |m: &Mesh| -> usize {
                match (keep, tris) {
                    (_, Some(t)) => t as usize,
                    (Some(k), None) => (m.triangle_count() as f64 * k.clamp(0.1, 100.0) / 100.0) as usize,
                    (None, None) => m.triangle_count() / 4,
                }
                .max(4)
            };
            let pieces = map_meshes(load(Path::new(pos[0]))?, |m| Ok(oxy_topo::decimate(m, target(m))))?;
            save(&pieces, Path::new(pos[1]), Path::new(pos[0]), export)
        }
        "lod" => {
            need(2)?;
            let levels = flag_num(args, "--levels")?.unwrap_or(3.0).clamp(1.0, 10.0) as usize;
            let ext = flag_str(args, "--format").unwrap_or("glb");
            let input = Path::new(pos[0]);
            let mut current = load(input)?;
            let prefix = pos[1];
            let mut written = Vec::new();
            for level in 0..=levels {
                if level > 0 {
                    current = map_meshes(current, |m| Ok(oxy_topo::decimate(m, (m.triangle_count() / 2).max(32))))?;
                }
                let out = PathBuf::from(format!("{prefix}_LOD{level}.{ext}"));
                save(&current, &out, input, export)?;
                let tris: usize = current.iter().map(|p| p.mesh.triangle_count()).sum();
                written.push(format!("  {} ({tris} triangles)", out.display()));
            }
            Ok(format!("wrote {} LOD files:\n{}", levels + 1, written.join("\n")))
        }
        "mcp" => mcp(&args[1..]),
        "addon" => match pos.first().copied().unwrap_or("") {
            "pack" => {
                need(3)?;
                let bytes = oxy_plugin::addon::pack(Path::new(pos[1]))?;
                let out = Path::new(pos[2]);
                std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
                let (info, files) = oxy_plugin::addon::inspect(&bytes)?;
                Ok(format!(
                    "wrote {} ({} files, {} bytes): {}",
                    out.display(),
                    files.len(),
                    bytes.len(),
                    describe_addon(&info)
                ))
            }
            "inspect" => {
                need(2)?;
                let path = Path::new(pos[1]);
                let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let (info, files) = oxy_plugin::addon::inspect(&bytes)?;
                let list: Vec<String> = files.iter().map(|(n, c)| format!("  {n} ({} bytes)", c.len())).collect();
                Ok(format!("{}\n{}", describe_addon(&info), list.join("\n")))
            }
            other => Err(format!("addon: unknown action `{other}` (use pack or inspect); usage")),
        },
        "split" => {
            need(2)?;
            let by_parts = match flag_str(args, "--by").unwrap_or("parts") {
                "parts" => true,
                "groups" => false,
                other => return Err(format!("--by must be `groups` or `parts`, not `{other}`")),
            };
            let ext = flag_str(args, "--format").unwrap_or("glb");
            let input = Path::new(pos[0]);
            let dir = Path::new(pos[1]);
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let mut written = Vec::new();
            for p in load(input)? {
                let keys = if by_parts { p.mesh.island_ids() } else { p.mesh.groups().to_vec() };
                for (k, (key, data)) in p.mesh.split_by(&keys).into_iter().enumerate() {
                    let label =
                        if by_parts { format!("{} part {}", p.name, k + 1) } else { format!("{} group {key}", p.name) };
                    let out = dir.join(format!("{}.{ext}", file_stem(&label)));
                    let piece = Piece::new(label, Mesh::from_data(data));
                    save(std::slice::from_ref(&piece), &out, input, export)?;
                    written.push(format!("  {}", out.display()));
                }
            }
            Ok(format!("wrote {} files:\n{}", written.len(), written.join("\n")))
        }
        other => Err(format!("unknown command `{other}` (usage below)")),
    }
}

/// Arguments that aren't `--flag value` pairs.
fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.as_str());
        }
    }
    out
}

fn flag_str<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn flag_num(args: &[String], name: &str) -> Result<Option<f64>, String> {
    match flag_str(args, name) {
        None => Ok(None),
        Some(v) => v.trim_end_matches('%').parse().map(Some).map_err(|_| format!("{name} needs a number, not `{v}`")),
    }
}

/// `mcp` is the local agent protocol. It does not print a summary: stdout is the protocol.
fn mcp(args: &[String]) -> Result<String, String> {
    let attach = flag_str(args, "--attach");
    let token = flag_str(args, "--token");
    let files = positional(args);
    match (attach, token, files.as_slice()) {
        (Some(addr), Some(token), []) => {
            oxy_mcp::serve_stdio_attach(addr, token)?;
            Ok(String::new())
        }
        (Some(_), None, _) => Err("--attach needs --token (see usage)".into()),
        (None, Some(_), _) => Err("--token is only used with --attach (see usage)".into()),
        (Some(_), Some(_), _) => Err("mcp --attach does not take a file (see usage)".into()),
        (None, None, []) => {
            let mut session = oxy_mcp::HeadlessSession::new();
            oxy_mcp::serve_stdio(oxy_mcp::Identity::cli(), &mut session).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        (None, None, [path]) => {
            let mut session = oxy_mcp::HeadlessSession::from_file(Path::new(path))?;
            oxy_mcp::serve_stdio(oxy_mcp::Identity::cli(), &mut session).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        (None, None, _) => Err("mcp takes one file (see usage)".into()),
    }
}

fn file_stem(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn load(path: &Path) -> Result<Vec<Piece>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == format::EXTENSION {
        let scene = format::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let pieces: Vec<Piece> = scene.pieces.into_iter().filter(|p| p.visible).map(PieceData::into_piece).collect();
        if pieces.is_empty() {
            return Err(format!("{}: no visible pieces", path.display()));
        }
        Ok(pieces)
    } else {
        let mesh = oxy_io::import(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_stem().map_or("Mesh".into(), |s| s.to_string_lossy().into_owned());
        Ok(vec![Piece::new(name, mesh)])
    }
}

fn map_meshes(pieces: Vec<Piece>, f: impl Fn(&Mesh) -> Result<Mesh, String>) -> Result<Vec<Piece>, String> {
    pieces.into_iter().map(|p| Ok(Piece::new(p.name.clone(), f(&p.mesh)?))).collect()
}

/// `--compress meshopt|quantize|none`: how glTF/GLB outputs are compressed.
fn export_options(args: &[String]) -> Result<oxy_io::ExportOptions, String> {
    let gltf = match flag_str(args, "--compress") {
        None => oxy_io::GltfCompression::None,
        Some(v) => oxy_io::GltfCompression::from_key(v)
            .ok_or_else(|| format!("--compress takes `meshopt`, `quantize` or `none`, not `{v}`"))?,
    };
    if args.iter().any(|a| a == "--compress") && flag_str(args, "--compress").is_none() {
        return Err("--compress needs `meshopt`, `quantize` or `none`".into());
    }
    Ok(oxy_io::ExportOptions { gltf })
}

fn save(pieces: &[Piece], out: &Path, input: &Path, opts: oxy_io::ExportOptions) -> Result<String, String> {
    if same_file(out, input) {
        return Err(format!("refusing to overwrite the input file {}", input.display()));
    }
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let tris: usize = pieces.iter().map(|p| p.mesh.triangle_count()).sum();
    if ext == format::EXTENSION {
        let scene = SceneData { pieces: pieces.iter().map(Piece::to_data).collect(), active: 0, alphas: Vec::new() };
        format::save(&scene, out).map_err(|e| format!("{}: {e}", out.display()))?;
    } else {
        let list: Vec<ExportPiece> = pieces.iter().map(|p| ExportPiece::new(&p.name, &p.mesh)).collect();
        oxy_io::export_pieces_with(&list, out, opts).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(format!("wrote {} ({} piece(s), {tris} triangles)", out.display(), pieces.len()))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn end_to_end_on_files() {
        let dir = std::env::temp_dir().join(format!("oxy-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let d = |n: &str| dir.join(n).to_string_lossy().into_owned();
        // A two-piece project: a sphere and a cube.
        let scene = SceneData {
            pieces: vec![
                Piece::new("Head", oxy_mesh::primitives::icosphere(3, 1.0)).to_data(),
                Piece::new("Box", oxy_mesh::primitives::cube(2, 1.0)).to_data(),
            ],
            active: 0,
            alphas: Vec::new(),
        };
        format::save(&scene, &dir.join("in.oxb")).unwrap();

        let info = run(&args(&["info", &d("in.oxb")])).unwrap();
        assert!(info.contains("2 piece(s)") && info.contains("Head: 1280 triangles"), "{info}");
        assert!(run(&args(&["convert", &d("in.oxb"), &d("out.glb")])).unwrap().contains("2 piece(s)"));
        assert!(run(&args(&["convert", &d("in.oxb"), &d("out.obj")])).is_ok());
        // Compressed glTF: smaller, and it says which extension it needs.
        assert!(run(&args(&["convert", &d("in.oxb"), &d("small.glb"), "--compress", "quantize"])).is_ok());
        let (plain, small) = (std::fs::read(d("out.glb")).unwrap(), std::fs::read(d("small.glb")).unwrap());
        assert!(small.len() * 10 < plain.len() * 6, "{} vs {}", small.len(), plain.len());
        assert!(small.windows(21).any(|w| w == b"KHR_mesh_quantization"));
        assert!(!plain.windows(21).any(|w| w == b"KHR_mesh_quantization"));
        let e = run(&args(&["convert", &d("in.oxb"), &d("x.glb"), "--compress", "zip"])).unwrap_err();
        assert!(e.contains("quantize"), "{e}");
        assert!(run(&args(&["convert", &d("in.oxb"), &d("tiny.glb"), "--compress", "meshopt"])).is_ok());
        let tiny = std::fs::read(d("tiny.glb")).unwrap();
        assert!(tiny.len() < small.len(), "{} vs {}", tiny.len(), small.len());
        assert!(tiny.windows(23).any(|w| w == b"EXT_meshopt_compression"));
        // OBJ round trip, then decimate to a percentage.
        let msg = run(&args(&["decimate", &d("out.obj"), &d("light.stl"), "--keep", "25%"])).unwrap();
        assert!(msg.contains("light.stl"), "{msg}");
        assert!(run(&args(&["remesh", &d("in.oxb"), &d("remeshed.oxb"), "--resolution", "48"])).is_ok());
        assert!(run(&args(&["info", &d("remeshed.oxb")])).unwrap().contains("2 piece(s)"));
        // LOD files.
        let lod = run(&args(&["lod", &d("in.oxb"), &d("lods/hero"), "--levels", "2", "--format", "ply"])).unwrap();
        for l in 0..=2 {
            assert!(dir.join(format!("lods/hero_LOD{l}.ply")).exists(), "{lod}");
        }
        // Split the merged OBJ (one mesh of two loose parts) into parts.
        let split = run(&args(&["split", &d("out.obj"), &d("parts"), "--by", "parts"])).unwrap();
        assert!(split.contains("wrote 2 files"), "{split}");
        // Safety and errors.
        assert!(run(&args(&["convert", &d("in.oxb"), &d("in.oxb")])).unwrap_err().contains("refusing"));
        assert!(run(&args(&["convert", &d("missing.obj"), &d("x.glb")])).is_err());
        assert!(run(&args(&["decimate", &d("in.oxb"), &d("x.glb"), "--keep", "lots"])).unwrap_err().contains("number"));
        assert!(run(&args(&["mcp", "--attach", "10.0.0.1:9", "--token", "abc"])).unwrap_err().contains("refusing"));
        assert!(run(&args(&["mcp", "--token", "abc"])).unwrap_err().contains("--attach"));
        assert!(run(&args(&["frobnicate"])).unwrap_err().contains("unknown command"));
        assert!(run(&args(&["info"])).unwrap_err().contains("takes 1"));
        assert!(run(&[]).unwrap().contains("USAGE"));
        std::fs::remove_dir_all(dir).ok();
    }
}
