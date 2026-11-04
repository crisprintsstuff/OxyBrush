//! Write sample exports (sphere + cube) in every format, e.g. to check them
//! with external validators:
//!
//!     cargo run -p oxy-io --example export_sample -- /tmp/out
use oxy_io::{EXPORT_EXTENSIONS, ExportPiece, export_pieces};
use oxy_mesh::primitives;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).expect("output folder");
    let sphere = primitives::icosphere(3, 1.0);
    let mut cube = primitives::cube(4, 1.0);
    let mut hidden = vec![false; cube.triangle_count()];
    hidden[0] = true;
    cube.set_hidden(hidden);
    let pieces = [ExportPiece { name: "Sphere", mesh: &sphere }, ExportPiece { name: "Cube", mesh: &cube }];
    for ext in EXPORT_EXTENSIONS {
        let path = dir.join(format!("sample.{ext}"));
        let stats = export_pieces(&pieces, &path).expect("export");
        println!("{}: {} pieces, {} triangles", path.display(), stats.pieces, stats.triangles);
    }
}
