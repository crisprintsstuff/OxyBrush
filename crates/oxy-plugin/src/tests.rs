use super::*;

const SQUASH_JSON: &str = include_str!("../plugins/squash/plugin.json");
const SQUASH_WAT: &str = include_str!("../plugins/squash/plugin.wat");

fn snapshot() -> Snapshot {
    let positions = vec![Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
    Snapshot {
        pieces: vec![PieceView {
            positions: positions.clone(),
            triangles: vec![[0, 1, 2]],
            mask: vec![0.0, 1.0, 0.0],
            normals: vec![Vec3::Z; 3],
            colors: vec![Vec3::ONE; 3],
        }],
        vertex_counts: vec![3],
        active: 0,
        scene_json: "{\"pieces\":1}".into(),
    }
}

fn plugin(manifest: &str, wat: &str) -> Result<Plugin, String> {
    Plugin::from_parts(manifest, wat.as_bytes(), Path::new("."))
}

const BASIC: &str = r#"{"name":"T","api":1,"capabilities":[],"commands":[{"id":"go"}]}"#;

#[test]
fn manifest_parses_and_rejects_bad_ones() {
    let m = Manifest::parse(SQUASH_JSON).unwrap();
    assert_eq!(m.name, "Squash");
    assert!(m.capabilities.read_mesh && m.capabilities.edit_mesh && m.capabilities.overlay);
    assert!(!m.capabilities.read_scene);
    assert_eq!(m.commands[0].label, "Squash");
    assert_eq!(m.params[0].default, 0.3);
    assert!(Manifest::parse(r#"{"name":"X","api":3}"#).unwrap_err().contains("API 3"));
    assert!(Manifest::parse(r#"{"name":"X","api":0}"#).is_err());
    assert!(Manifest::parse(r#"{"name":"X","api":2,"capabilities":["add_pieces"]}"#).unwrap().capabilities.add_pieces);
    assert!(Manifest::parse(r#"{"name":"X","api":1,"capabilities":["network"]}"#).unwrap_err().contains("unknown"));
    assert!(Manifest::parse(r#"{"api":1}"#).is_err());
    assert!(Manifest::parse(r#"{"name":"X","api":1,"params":[{"id":"a","min":1,"max":0}]}"#).is_err());
}

#[test]
fn squash_example_edits_only_through_its_result() {
    let mut p = plugin(SQUASH_JSON, SQUASH_WAT).unwrap();
    p.params[0] = 0.5;
    let snap = snapshot();
    let before = snap.pieces[0].positions.clone();
    let r = p.run(0, snap).unwrap();
    assert_eq!(r.log, vec!["Squashed toward the middle".to_string()]);
    assert_eq!(r.edits.len(), 1);
    let (piece, after) = &r.edits[0];
    assert_eq!(*piece, 0);
    // Middle height is 0: y halves where unmasked, stays where masked; x, z untouched.
    assert_eq!(after[0], Vec3::new(0.0, -0.5, 0.0));
    assert_eq!(after[1], before[1], "masked vertex stays");
    assert_eq!(after[2], before[2]);
    // Its overlay draws two lines at the middle height.
    let lines = p.overlay().unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.a.y == 0.0 && l.rgba == 0xff8040ff));
}

#[test]
fn capabilities_are_enforced() {
    // No capabilities: reading a mesh, writing, scene state and drawing all refuse.
    let wat = r#"(module
        (import "oxybrush" "vertex_count" (func $vc (param i32) (result i32)))
        (import "oxybrush" "write_positions" (func $wp (param i32 i32 i32) (result i32)))
        (import "oxybrush" "scene_state" (func $ss (param i32 i32) (result i32)))
        (import "oxybrush" "draw_line" (func $dl (param f32 f32 f32 f32 f32 f32 i32) (result i32)))
        (import "oxybrush" "log" (func $log (param i32 i32)))
        (memory (export "memory") 1)
        (func $report (param $v i32)
          ;; log one byte: '0' + (-v)
          (i32.store8 (i32.const 0) (i32.sub (i32.const 48) (local.get $v)))
          (call $log (i32.const 0) (i32.const 1)))
        (func (export "run") (param i32)
          (call $report (call $vc (i32.const 0)))
          (call $report (call $wp (i32.const 0) (i32.const 0) (i32.const 3)))
          (call $report (call $ss (i32.const 0) (i32.const 10)))
          (call $report (call $dl (f32.const 0) (f32.const 0) (f32.const 0) (f32.const 1) (f32.const 1) (f32.const 1) (i32.const 0)))))"#;
    let mut p = plugin(BASIC, wat).unwrap();
    let r = p.run(0, snapshot()).unwrap();
    assert!(r.edits.is_empty());
    // denied, denied, denied, and draw_line outside overlay is a context error.
    assert_eq!(r.log, vec!["1", "1", "1", "5"]);
}

#[test]
fn bad_writes_are_refused() {
    let manifest = r#"{"name":"T","api":1,"capabilities":["read_mesh","edit_mesh"],"commands":[{"id":"go"}]}"#;
    let wat = r#"(module
        (import "oxybrush" "write_positions" (func $wp (param i32 i32 i32) (result i32)))
        (import "oxybrush" "read_positions" (func $rp (param i32 i32 i32) (result i32)))
        (import "oxybrush" "log" (func $log (param i32 i32)))
        (memory (export "memory") 1)
        (func $report (param $v i32)
          (i32.store8 (i32.const 0) (i32.sub (i32.const 48) (local.get $v)))
          (call $log (i32.const 0) (i32.const 1)))
        (func (export "run") (param i32)
          (call $report (call $wp (i32.const 0) (i32.const 16) (i32.const 2)))
          (call $report (call $wp (i32.const 7) (i32.const 16) (i32.const 3)))
          (call $report (call $wp (i32.const 0) (i32.const 65530) (i32.const 3)))
          (call $report (call $rp (i32.const 0) (i32.const 65530) (i32.const 3)))))"#;
    let mut p = plugin(manifest, wat).unwrap();
    let r = p.run(0, snapshot()).unwrap();
    assert!(r.edits.is_empty());
    // wrong count, no such piece, out of memory bounds (write and read).
    assert_eq!(r.log, vec!["4", "2", "3", "3"]);
}

#[test]
fn a_runaway_plugin_is_stopped_and_stays_usable() {
    let wat = r#"(module
        (import "oxybrush" "param" (func $param (param i32) (result f32)))
        (memory (export "memory") 1)
        (func (export "run") (param $c i32)
          (if (i32.eqz (local.get $c)) (then (loop $forever (br $forever))))))"#;
    let manifest = r#"{"name":"Loop","api":1,"commands":[{"id":"hang"},{"id":"fine"}]}"#;
    let mut p = plugin(manifest, wat).unwrap();
    let t = std::time::Instant::now();
    let err = p.run(0, Snapshot::default()).unwrap_err();
    assert!(err.contains("ran too long"), "{err}");
    assert!(t.elapsed().as_secs() < 30);
    // The next call gets fresh fuel.
    assert!(p.run(1, Snapshot::default()).is_ok());
    assert!(p.run(5, Snapshot::default()).is_err(), "no such command");
}

#[test]
fn memory_is_capped_and_modules_are_checked() {
    let grow = r#"(module (memory (export "memory") 1)
        (func (export "run") (param i32)
          (if (i32.eq (memory.grow (i32.const 20000)) (i32.const -1)) (then unreachable))))"#;
    let mut p = plugin(BASIC, grow).unwrap();
    assert!(p.run(0, Snapshot::default()).is_err(), "growing past the cap fails");
    let no_run = r#"(module (memory (export "memory") 1))"#;
    assert!(plugin(BASIC, no_run).unwrap_err().contains("run"));
    let no_mem = r#"(module (func (export "run") (param i32)))"#;
    assert!(plugin(BASIC, no_mem).unwrap_err().contains("memory"));
    // Importing anything outside the host API fails to load.
    let wasi = r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1) (func (export "run") (param i32)))"#;
    assert!(plugin(BASIC, wasi).is_err());
    assert!(plugin(BASIC, "not wasm").is_err());
}

#[test]
fn discover_loads_folders_and_reports_failures() {
    let dir = std::env::temp_dir().join(format!("oxy-plugin-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("squash")).unwrap();
    std::fs::write(dir.join("squash/plugin.json"), SQUASH_JSON).unwrap();
    std::fs::write(dir.join("squash/plugin.wat"), SQUASH_WAT).unwrap();
    std::fs::create_dir_all(dir.join("broken")).unwrap();
    std::fs::write(dir.join("broken/plugin.json"), BASIC).unwrap();
    std::fs::create_dir_all(dir.join("not-a-plugin")).unwrap();
    let found = discover(&dir);
    assert_eq!(found.len(), 2);
    assert!(found[0].as_ref().is_err_and(|e| e.contains("no plugin.wasm")));
    assert_eq!(found[1].as_ref().unwrap().manifest.name, "Squash");
    let _ = std::fs::remove_dir_all(&dir);
}

const V2_WAT: &str = r#"(module
    (import "oxybrush" "write_mask" (func $wm (param i32 i32 i32) (result i32)))
    (import "oxybrush" "write_colors" (func $wc (param i32 i32 i32) (result i32)))
    (import "oxybrush" "read_normals" (func $rn (param i32 i32 i32) (result i32)))
    (import "oxybrush" "add_piece" (func $ap (param i32 i32 i32 i32 i32 i32) (result i32)))
    (import "oxybrush" "log" (func $log (param i32 i32)))
    (memory (export "memory") 1)
    ;; mask: 0.25, 0.5, 2.0 (clamped to 1)
    (data (i32.const 0) "\00\00\80\3e\00\00\00\3f\00\00\00\40")
    ;; colors: red, green, blue
    (data (i32.const 16) "\00\00\80\3f\00\00\00\00\00\00\00\00\00\00\00\00\00\00\80\3f\00\00\00\00\00\00\00\00\00\00\00\00\00\00\80\3f")
    ;; new piece: 3 vertices (0,0,0) (1,0,0) (0,1,0), one triangle 0 1 2, name "Tri"
    (data (i32.const 64) "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\80\3f\00\00\00\00\00\00\00\00\00\00\00\00\00\00\80\3f\00\00\00\00")
    (data (i32.const 112) "\00\00\00\00\01\00\00\00\02\00\00\00")
    (data (i32.const 128) "Tri")
    (func $report (param $v i32)
      (i32.store8 (i32.const 200) (i32.add (i32.const 48) (local.get $v)))
      (call $log (i32.const 200) (i32.const 1)))
    (func (export "run") (param i32)
      (call $report (call $wm (i32.const 0) (i32.const 0) (i32.const 3)))
      (call $report (call $wc (i32.const 0) (i32.const 16) (i32.const 3)))
      (call $report (call $rn (i32.const 0) (i32.const 300) (i32.const 3)))
      (call $report (call $ap (i32.const 128) (i32.const 3) (i32.const 64) (i32.const 3) (i32.const 112) (i32.const 1)))))"#;

#[test]
fn api2_mask_color_and_new_pieces() {
    let manifest =
        r#"{"name":"V2","api":2,"capabilities":["read_mesh","edit_mesh","add_pieces"],"commands":[{"id":"go"}]}"#;
    let mut p = plugin(manifest, V2_WAT).unwrap();
    let r = p.run(0, snapshot()).unwrap();
    // write_mask ok, write_colors ok, 3 normals read, first new piece is index 0.
    assert_eq!(r.log, vec!["0", "0", "3", "0"]);
    assert_eq!(r.masks, vec![(0, vec![0.25, 0.5, 1.0])]);
    assert_eq!(r.colors, vec![(0, vec![Vec3::X, Vec3::Y, Vec3::Z])]);
    assert_eq!(r.new_pieces.len(), 1);
    let np = &r.new_pieces[0];
    assert_eq!(np.name, "Tri");
    assert_eq!(np.positions, vec![Vec3::ZERO, Vec3::X, Vec3::Y]);
    assert_eq!(np.triangles, vec![[0, 1, 2]]);

    // Without the capabilities every one of them is refused (-1 -> '/').
    let bare = r#"{"name":"V2","api":2,"capabilities":[],"commands":[{"id":"go"}]}"#;
    let mut p = plugin(bare, V2_WAT).unwrap();
    let r = p.run(0, snapshot()).unwrap();
    assert_eq!(r.log, vec!["/", "/", "/", "/"]);
    assert!(r.masks.is_empty() && r.colors.is_empty() && r.new_pieces.is_empty());
}

/// A unit cube from (0, 0, 0) to (1, 1, 1), faces pointing out.
fn cube() -> PieceView {
    let positions: Vec<Vec3> =
        (0..8).map(|i| Vec3::new((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32)).collect();
    let triangles = vec![
        [0, 2, 3],
        [0, 3, 1], // z = 0
        [4, 5, 7],
        [4, 7, 6], // z = 1
        [0, 1, 5],
        [0, 5, 4], // y = 0
        [2, 6, 7],
        [2, 7, 3], // y = 1
        [0, 4, 6],
        [0, 6, 2], // x = 0
        [1, 3, 7],
        [1, 7, 5], // x = 1
    ];
    let normals = positions.iter().map(|p| (*p - Vec3::splat(0.5)).normalize()).collect();
    PieceView { positions, triangles, mask: vec![0.0; 8], normals, colors: vec![Vec3::ONE; 8] }
}

fn cube_snapshot() -> Snapshot {
    Snapshot { pieces: vec![cube()], vertex_counts: vec![8], active: 0, scene_json: String::new() }
}

fn bundled(name: &str) -> Plugin {
    let (_, json, wat) = BUNDLED.iter().find(|(n, _, _)| *n == name).expect("bundled plugin");
    plugin(json, wat).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn every_bundled_plugin_loads() {
    for (name, json, wat) in BUNDLED {
        let p = plugin(json, wat).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!p.manifest.commands.is_empty(), "{name}");
        assert!(!p.manifest.description.is_empty(), "{name}");
    }
}

#[test]
#[allow(clippy::needless_range_loop)]
fn displace_moves_along_normals_except_where_masked() {
    let mut p = bundled("displace");
    p.params[0] = 10.0; // 10 % of the size (1): 0.1
    let mut snap = cube_snapshot();
    snap.pieces[0].mask[7] = 1.0;
    let r = p.run(0, snap.clone()).unwrap();
    assert_eq!(r.log, vec!["Inflated".to_string()]);
    let after = &r.edits[0].1;
    let c = cube();
    for v in 0..7 {
        let moved = after[v] - c.positions[v];
        assert!((moved - c.normals[v] * 0.1).length() < 1e-5, "{v}: {moved}");
    }
    assert_eq!(after[7], c.positions[7], "masked vertex stays");
    // Deflate goes the other way.
    let r = p.run(1, snap.clone()).unwrap();
    assert!((r.edits[0].1[0] - c.positions[0] + c.normals[0] * 0.1).length() < 1e-5);
    // Roughen: along the normal, no further than the amount, not all the same,
    // and the same again for the same seed.
    let a = p.run(2, snap.clone()).unwrap().edits[0].1.clone();
    let b = p.run(2, snap).unwrap().edits[0].1.clone();
    assert_eq!(a, b);
    let d: Vec<f32> = (0..7).map(|v| (a[v] - c.positions[v]).dot(c.normals[v])).collect();
    assert!(d.iter().all(|x| x.abs() <= 0.1 + 1e-6), "{d:?}");
    assert!(d.iter().any(|x| (x - d[0]).abs() > 1e-4), "{d:?}");
}

#[test]
#[allow(clippy::needless_range_loop)]
fn mask_by_height_masks_below_and_above_and_draws_the_level() {
    let mut p = bundled("mask_height");
    assert!(p.overlay().unwrap().is_empty(), "nothing to draw before a run");
    p.params[0] = 50.0;
    p.params[1] = 0.0;
    let r = p.run(0, cube_snapshot()).unwrap();
    let (piece, mask) = &r.masks[0];
    assert_eq!(*piece, 0);
    let c = cube();
    for v in 0..8 {
        let want = if c.positions[v].y < 0.5 { 1.0 } else { 0.0 };
        assert_eq!(mask[v], want, "{v}");
    }
    let r = p.run(1, cube_snapshot()).unwrap();
    assert!((0..8).all(|v| r.masks[0].1[v] == if c.positions[v].y > 0.5 { 1.0 } else { 0.0 }));
    assert!(r.edits.is_empty() && r.colors.is_empty());
    let lines = p.overlay().unwrap();
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|l| l.a.y == 0.5 && l.b.y == 0.5));
}

#[test]
fn color_ramp_paints_by_height_and_direction_and_respects_the_mask() {
    let mut p = bundled("color_ramp");
    let mut snap = cube_snapshot();
    snap.pieces[0].mask[0] = 1.0;
    let r = p.run(0, snap.clone()).unwrap();
    let colors = &r.colors[0].1;
    assert_eq!(colors[0], Vec3::ONE, "masked vertex keeps its color");
    // Bottom is blue-ish, top is pale yellow.
    assert!(colors[1].z > colors[1].x, "{}", colors[1]);
    assert!((colors[2] - Vec3::new(1.0, 0.93, 0.70)).length() < 1e-5, "{}", colors[2]);
    // By direction: the normal as a color.
    let r = p.run(1, snap.clone()).unwrap();
    let n = cube().normals[7];
    assert!((r.colors[0].1[7] - (n * 0.5 + Vec3::splat(0.5))).length() < 1e-5);
    // Half strength: halfway from white.
    p.params[0] = 0.5;
    let r = p.run(1, snap).unwrap();
    let want = Vec3::ONE.lerp(n * 0.5 + Vec3::splat(0.5), 0.5);
    assert!((r.colors[0].1[7] - want).length() < 1e-5);
}

#[test]
fn array_adds_moved_copies() {
    let mut p = bundled("array");
    p.params[0] = 3.0;
    p.params[1] = 50.0;
    let r = p.run(1, cube_snapshot()).unwrap();
    assert_eq!(r.new_pieces.len(), 3);
    for (k, np) in r.new_pieces.iter().enumerate() {
        assert_eq!(np.name, "Copy");
        assert_eq!(np.triangles, cube().triangles);
        let shift = 1.5 * (k + 1) as f32;
        for (a, b) in np.positions.iter().zip(&cube().positions) {
            assert!((*a - *b - Vec3::new(0.0, shift, 0.0)).length() < 1e-5);
        }
    }
    assert!(r.edits.is_empty());
}

#[test]
fn measure_reports_size_and_volume_and_draws_the_box() {
    let mut p = bundled("measure");
    let mut snap = cube_snapshot();
    for v in &mut snap.pieces[0].positions {
        *v *= Vec3::new(2.0, 1.0, 0.5);
    }
    let r = p.run(0, snap).unwrap();
    assert_eq!(r.log, vec!["Size 2.000 × 1.000 × 0.500, volume 1.000".to_string()]);
    let lines = p.overlay().unwrap();
    assert_eq!(lines.len(), 12);
}
