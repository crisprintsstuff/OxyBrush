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
            groups: vec![7],
            hidden: vec![false],
        }],
        vertex_counts: vec![3],
        triangle_counts: vec![1],
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
    assert!(Manifest::parse(r#"{"name":"X","api":4}"#).unwrap_err().contains("API 4"));
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
    PieceView {
        positions,
        triangles,
        mask: vec![0.0; 8],
        normals,
        colors: vec![Vec3::ONE; 8],
        groups: vec![0; 12],
        hidden: vec![false; 12],
    }
}

fn cube_snapshot() -> Snapshot {
    Snapshot {
        pieces: vec![cube()],
        vertex_counts: vec![8],
        triangle_counts: vec![12],
        active: 0,
        scene_json: String::new(),
    }
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

// ---- API 3 ----

#[test]
fn api3_param_types_parse_and_bad_ones_are_refused() {
    let m = Manifest::parse(
        r#"{"name":"P","api":3,"capabilities":["edit_topology"],
            "commands":[{"id":"a","preview":true},{"id":"b"}],
            "params":[
              {"id":"n","min":0,"max":10,"step":1,"default":3.4,"help":"how many"},
              {"id":"t","type":"toggle","default":true},
              {"id":"c","type":"choice","options":["X","Y","Z"],"default":2},
              {"id":"col","type":"color","default":[1,0.5,0]},
              {"id":"l","type":"label","text":"Hello"}
            ]}"#,
    )
    .unwrap();
    assert!(m.capabilities.edit_topology);
    assert!(m.commands[0].preview && !m.commands[1].preview);
    let p = &m.params;
    assert_eq!(p[0].kind, ParamKind::Slider { step: Some(1.0) });
    assert_eq!(p[0].help, "how many");
    assert_eq!((p[1].kind.clone(), p[1].default), (ParamKind::Toggle, 1.0));
    assert_eq!(p[2].default, 2.0);
    assert_eq!(p[3].default_color(), Vec3::new(1.0, 0.5, 0.0));
    assert_eq!(p[4].kind, ParamKind::Label("Hello".into()));
    // Values are made valid: whole steps, 0/1, an option index; labels have none.
    assert_eq!(p[0].sanitize(3.6), Some(4.0));
    assert_eq!(p[0].sanitize(99.0), Some(10.0));
    assert_eq!(p[1].sanitize(0.7), Some(1.0));
    assert_eq!(p[2].sanitize(5.0), Some(2.0));
    assert_eq!(p[0].sanitize(f32::NAN), None);
    assert_eq!(p[4].sanitize(1.0), None);
    for bad in [
        r#"{"id":"x","type":"knob"}"#,
        r#"{"id":"x","type":"choice","options":[]}"#,
        r#"{"id":"x","type":"color","default":[1,2]}"#,
        r#"{"id":"x","step":0}"#,
    ] {
        let json = format!(r#"{{"name":"P","api":3,"params":[{bad}]}}"#);
        assert!(Manifest::parse(&json).is_err(), "{bad}");
    }
}

#[test]
fn group_by_height_makes_bands_along_the_chosen_axis() {
    let mut p = bundled("group_height");
    assert!(p.manifest.commands[0].preview);
    p.params[0] = 1.0; // Y
    p.params[1] = 2.0; // two bands
    let r = p.run(0, cube_snapshot()).unwrap();
    let (piece, groups) = &r.groups[0];
    assert_eq!((*piece, groups.len()), (0, 12));
    let cube = cube();
    for (t, tri) in cube.triangles.iter().enumerate() {
        let y = tri.iter().map(|&v| cube.positions[v as usize].y).sum::<f32>() / 3.0;
        let want = if y < 0.5 { 1 } else { 2 };
        assert_eq!(groups[t], want, "triangle {t} at height {y}");
    }
    assert_eq!(r.log, vec!["Grouped into bands"]);
    assert_eq!(p.progress().fraction(), Some(1.0), "it reported progress");
}

#[test]
fn flip_faces_rewrites_the_mesh_and_keeps_its_attributes() {
    let mut p = bundled("flip_faces");
    let mut snap = cube_snapshot();
    snap.pieces[0].mask[3] = 0.75;
    snap.pieces[0].groups[5] = 9;
    let r = p.run(0, snap).unwrap();
    let (piece, mesh) = &r.meshes[0];
    assert_eq!(*piece, 0);
    let cube = cube();
    assert_eq!(mesh.positions, cube.positions);
    for (a, b) in mesh.triangles.iter().zip(&cube.triangles) {
        assert_eq!(*a, [b[0], b[2], b[1]], "corners swapped");
    }
    assert_eq!(r.masks[0].1[3], 0.75, "mask written back for the new mesh");
    assert_eq!(r.groups[0].1[5], 9);
    assert_eq!(r.colors[0].1.len(), 8);
}

/// A plugin whose `run` calls `body` and logs every i32 result left on
/// its stack via $r (one log line each, as a decimal number).
fn probe(caps: &str, imports: &str, body: &str) -> Plugin {
    let json = format!(r#"{{"name":"Probe","api":3,"capabilities":[{caps}],"commands":[{{"id":"go"}}]}}"#);
    let wat = format!(
        r#"(module
        (import "oxybrush" "log" (func $log (param i32 i32)))
        {imports}
        (memory (export "memory") 2)
        ;; Log v as text: '-' and a digit for errors, '0' for ok, 'n' for counts > 9.
        (func $r (param $v i32)
          (if (i32.lt_s (local.get $v) (i32.const 0))
            (then (i32.store8 (i32.const 0) (i32.const 45))
                  (i32.store8 (i32.const 1) (i32.sub (i32.const 48) (local.get $v)))
                  (call $log (i32.const 0) (i32.const 2)))
            (else (if (i32.gt_s (local.get $v) (i32.const 9))
              (then (i32.store8 (i32.const 0) (i32.const 110)))
              (else (i32.store8 (i32.const 0) (i32.add (i32.const 48) (local.get $v)))))
                  (call $log (i32.const 0) (i32.const 1)))))
        (func (export "run") (param i32) {body}))"#
    );
    plugin(&json, &wat).unwrap()
}

const FACE_IMPORTS: &str = r#"
    (import "oxybrush" "read_groups" (func $rg (param i32 i32 i32) (result i32)))
    (import "oxybrush" "read_hidden" (func $rh (param i32 i32 i32) (result i32)))
    (import "oxybrush" "write_groups" (func $wg (param i32 i32 i32) (result i32)))
    (import "oxybrush" "write_hidden" (func $wh (param i32 i32 i32) (result i32)))
    (import "oxybrush" "write_mesh" (func $wm (param i32 i32 i32 i32 i32) (result i32)))
    (import "oxybrush" "write_positions" (func $wp (param i32 i32 i32) (result i32)))
    (import "oxybrush" "write_mask" (func $wk (param i32 i32 i32) (result i32)))"#;

#[test]
fn face_and_topology_writes_need_their_capabilities() {
    let body = r#"
        (call $r (call $rg (i32.const 0) (i32.const 4096) (i32.const 1)))
        (call $r (call $rh (i32.const 0) (i32.const 4096) (i32.const 1)))
        (call $r (call $wg (i32.const 0) (i32.const 4096) (i32.const 1)))
        (call $r (call $wh (i32.const 0) (i32.const 4096) (i32.const 1)))
        (call $r (call $wm (i32.const 0) (i32.const 4096) (i32.const 3) (i32.const 8192) (i32.const 1)))"#;
    let denied = format!("-{}", -ERR_DENIED);
    let r = probe("", FACE_IMPORTS, body).run(0, snapshot()).unwrap();
    assert_eq!(r.log, vec![denied.clone(); 5]);
    // read_mesh + edit_mesh: everything but write_mesh.
    let r = probe(r#""read_mesh","edit_mesh""#, FACE_IMPORTS, body).run(0, snapshot()).unwrap();
    assert_eq!(r.log, vec!["1", "1", "0", "0", denied.as_str()]);
    assert_eq!(r.groups, vec![(0, vec![0])], "zeroed memory: group 0");
    assert_eq!(r.hidden, vec![(0, vec![false])]);
    assert!(r.meshes.is_empty());
}

#[test]
fn after_write_mesh_counts_follow_the_new_mesh() {
    // A new mesh of 4 vertices and 2 triangles (indices 0..3) for piece 0,
    // which had 3 vertices and 1 triangle.
    let body = r#"
        (i32.store (i32.const 8196) (i32.const 1)) (i32.store (i32.const 8200) (i32.const 2))
        (i32.store (i32.const 8204) (i32.const 0)) (i32.store (i32.const 8208) (i32.const 3))
        (i32.store (i32.const 8212) (i32.const 2))
        (call $r (call $wk (i32.const 0) (i32.const 4096) (i32.const 3)))
        (call $r (call $wm (i32.const 0) (i32.const 4096) (i32.const 4) (i32.const 8192) (i32.const 2)))
        (call $r (call $wp (i32.const 0) (i32.const 4096) (i32.const 4)))
        (call $r (call $wk (i32.const 0) (i32.const 4096) (i32.const 3)))
        (call $r (call $wk (i32.const 0) (i32.const 4096) (i32.const 4)))
        (call $r (call $wg (i32.const 0) (i32.const 4096) (i32.const 1)))
        (call $r (call $wg (i32.const 0) (i32.const 4096) (i32.const 2)))
        ;; Indices out of range are refused.
        (i32.store (i32.const 8192) (i32.const 9))
        (call $r (call $wm (i32.const 0) (i32.const 4096) (i32.const 4) (i32.const 8192) (i32.const 2)))"#;
    let caps = r#""read_mesh","edit_mesh","edit_topology""#;
    let r = probe(caps, FACE_IMPORTS, body).run(0, snapshot()).unwrap();
    let count = format!("-{}", -ERR_COUNT);
    let context = format!("-{}", -ERR_CONTEXT);
    assert_eq!(r.log, vec!["0", "0", &context, &count, "0", &count, "0", &count]);
    let (piece, mesh) = &r.meshes[0];
    assert_eq!((*piece, mesh.positions.len(), mesh.triangles.len()), (0, 4, 2));
    assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 3, 2]]);
    // The mask written before write_mesh was for the old mesh: dropped.
    assert_eq!(r.masks.len(), 1);
    assert_eq!(r.masks[0].1.len(), 4);
    assert_eq!(r.groups[0].1.len(), 2);
}

#[test]
fn param_color_and_a_stopped_command() {
    let json = r#"{"name":"C","api":3,"capabilities":["edit_mesh"],"commands":[{"id":"go"}],
        "params":[{"id":"s"},{"id":"tint","type":"color","default":[0.25,0.5,1]}]}"#;
    let wat = r#"(module
        (import "oxybrush" "param_color" (func $pc (param i32 i32) (result i32)))
        (import "oxybrush" "progress" (func $pr (param f32) (result i32)))
        (import "oxybrush" "write_positions" (func $wp (param i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "run") (param $c i32)
          (if (i32.eqz (local.get $c)) (then
            ;; Write the tint as the first vertex's position, to read it back.
            (drop (call $pc (i32.const 1) (i32.const 0)))
            (drop (call $wp (i32.const 0) (i32.const 0) (i32.const 3)))
            ;; Then wait until asked to stop.
            (block $stop (loop $wait
              (br_if $stop (call $pr (f32.const 0.5)))
              (br $wait)))))))"#;
    let mut p = Plugin::from_parts(json, wat.as_bytes(), Path::new(".")).unwrap();
    p.param_colors[1] = Vec3::new(0.25, 0.5, 1.0);
    let progress = p.progress();
    let stopper = std::thread::spawn(move || {
        while progress.fraction().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        progress.cancel();
    });
    let r = p.run(0, snapshot()).unwrap();
    stopper.join().unwrap();
    assert!(r.cancelled, "stopped on request");
    assert!(r.edits.is_empty(), "a stopped command's edits are dropped");
    // Run again without stopping: the plugin loops until fuel runs out, so
    // check param_color with a fresh, simpler plugin instead.
    let wat2 = r#"(module
        (import "oxybrush" "param_color" (func $pc (param i32 i32) (result i32)))
        (import "oxybrush" "write_positions" (func $wp (param i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "run") (param i32)
          (drop (call $pc (i32.const 1) (i32.const 0)))
          (drop (call $wp (i32.const 0) (i32.const 0) (i32.const 3)))))"#;
    let mut p = Plugin::from_parts(json, wat2.as_bytes(), Path::new(".")).unwrap();
    assert_eq!(p.param_colors[1], Vec3::new(0.25, 0.5, 1.0), "defaults from the manifest");
    let r = p.run(0, snapshot()).unwrap();
    assert!(!r.cancelled);
    assert_eq!(r.edits[0].1[0], Vec3::new(0.25, 0.5, 1.0));
}

#[test]
fn a_stop_asked_for_before_the_run_starts_still_counts() {
    // The app takes the progress handle, then the worker runs the command:
    // a Stop clicked in between must not be lost (it was, which made the
    // test above fail now and then).
    let json = r#"{"name":"Loop","api":3,"commands":[{"id":"go"}]}"#;
    let wat = r#"(module
        (import "oxybrush" "progress" (func $pr (param f32) (result i32)))
        (memory (export "memory") 1)
        (func (export "run") (param i32)
          (block $stop (loop $wait
            (br_if $stop (call $pr (f32.const 0.5)))
            (br $wait)))))"#;
    let mut p = Plugin::from_parts(json, wat.as_bytes(), Path::new(".")).unwrap();
    assert_eq!(p.progress().fraction(), None, "nothing reported yet");
    p.progress().cancel();
    let r = p.run(0, snapshot()).unwrap();
    assert!(r.cancelled);
    // The stop was used up: the next run isn't stopped before it starts
    // (this plugin would then loop until its fuel ran out, an error).
    assert!(!p.progress().cancelled());
}

fn boundary_edges(tris: &[[u32; 3]]) -> usize {
    let mut count = std::collections::HashMap::<(u32, u32), u32>::new();
    for t in tris {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    count.values().filter(|&&c| c == 1).count()
}

fn signed_volume(pos: &[Vec3], tris: &[[u32; 3]]) -> f32 {
    tris.iter()
        .map(|t| {
            let a = pos[t[0] as usize];
            let b = pos[t[1] as usize];
            let c = pos[t[2] as usize];
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

fn assert_mesh_ok(pos: &[Vec3], tris: &[[u32; 3]]) {
    assert!(pos.len() >= 3 && !tris.is_empty());
    assert!(pos.iter().all(|p| p.is_finite()), "non-finite position");
    for t in tris {
        assert!(t.iter().all(|&i| (i as usize) < pos.len()), "index out of range {t:?}");
        assert!(t[0] != t[1] && t[1] != t[2] && t[0] != t[2], "repeated index {t:?}");
        let a = pos[t[0] as usize];
        let b = pos[t[1] as usize];
        let c = pos[t[2] as usize];
        let cross = (b - a).cross(c - a);
        assert!(cross.is_finite() && cross.length() > 1e-8, "degenerate {t:?}");
    }
}

fn face_normal(pos: &[Vec3], t: [u32; 3]) -> Vec3 {
    let a = pos[t[0] as usize];
    let b = pos[t[1] as usize];
    let c = pos[t[2] as usize];
    (b - a).cross(c - a).normalize_or_zero()
}

/// A flat grid in the XZ plane, `n` vertices on a side.
fn grid(n: usize) -> PieceView {
    let mut positions = Vec::with_capacity(n * n);
    let mut colors = Vec::with_capacity(n * n);
    for z in 0..n {
        for x in 0..n {
            positions.push(Vec3::new(x as f32, 0.0, z as f32));
            colors.push(Vec3::new(x as f32 / n as f32, 0.2, z as f32 / n as f32));
        }
    }
    let mut triangles = Vec::new();
    for z in 0..n - 1 {
        for x in 0..n - 1 {
            let i = (z * n + x) as u32;
            let row = n as u32;
            triangles.push([i, i + row, i + 1]);
            triangles.push([i + 1, i + row, i + row + 1]);
        }
    }
    let nt = triangles.len();
    PieceView {
        positions,
        triangles,
        mask: vec![0.0; n * n],
        normals: vec![Vec3::Y; n * n],
        colors,
        groups: vec![3; nt],
        hidden: vec![false; nt],
    }
}

fn grid_snapshot(n: usize) -> Snapshot {
    let g = grid(n);
    let (nv, nt) = (g.positions.len(), g.triangles.len());
    Snapshot {
        pieces: vec![g],
        vertex_counts: vec![nv],
        triangle_counts: vec![nt],
        active: 0,
        scene_json: String::new(),
    }
}

/// A regular octahedron, closed and pointing out.
fn octahedron_snapshot() -> Snapshot {
    let positions = vec![Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
    let triangles = vec![[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [0, 3, 5], [3, 1, 5], [1, 2, 5], [2, 0, 5]];
    let piece = PieceView {
        positions,
        triangles,
        mask: vec![0.0; 6],
        normals: vec![Vec3::Y; 6],
        colors: vec![Vec3::ONE; 6],
        groups: vec![0; 8],
        hidden: vec![false; 8],
    };
    Snapshot {
        pieces: vec![piece],
        vertex_counts: vec![6],
        triangle_counts: vec![8],
        active: 0,
        scene_json: String::new(),
    }
}

#[test]
fn group_by_angle_splits_a_cube_on_creases() {
    let mut p = bundled("group_angle");
    assert!(p.manifest.commands[0].preview);
    assert!(p.manifest.capabilities.read_mesh && p.manifest.capabilities.edit_mesh);
    assert!(!p.manifest.capabilities.edit_topology);
    // 40 degrees: coplanar triangles share a group, the cube's 90 degree
    // corners do not. Six faces.
    p.params[0] = 40.0;
    p.params[1] = 0.0;
    let cube = cube();
    let r = p.run(0, cube_snapshot()).unwrap();
    let groups = &r.groups[0].1;
    assert_eq!(groups.len(), 12);
    let mut pairs = 0;
    for i in 0..12 {
        for j in (i + 1)..12 {
            let dot =
                face_normal(&cube.positions, cube.triangles[i]).dot(face_normal(&cube.positions, cube.triangles[j]));
            if dot > 0.99 {
                assert_eq!(groups[i], groups[j], "coplanar {i} and {j}");
                pairs += 1;
            } else if dot.abs() < 0.01 {
                assert_ne!(groups[i], groups[j], "a crease between {i} and {j}");
            }
        }
    }
    assert_eq!(pairs, 6, "one pair per face");
    assert_eq!(groups.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 6);
    assert_eq!(r.log, vec!["Grouped by angle"]);
    assert_eq!(p.progress().fraction(), Some(1.0));
    // Wide open: the whole cube is one smooth-enough piece.
    p.params[0] = 180.0;
    let r = p.run(0, cube_snapshot()).unwrap();
    assert!(r.groups[0].1.iter().all(|&g| g == r.groups[0].1[0]));
    // Existing different groups are an extra wall when asked.
    let mut snap = cube_snapshot();
    snap.pieces[0].groups[0] = 1;
    snap.pieces[0].groups[1] = 2;
    p.params[0] = 40.0;
    p.params[1] = 1.0;
    let r = p.run(0, snap.clone()).unwrap();
    assert_ne!(r.groups[0].1[0], r.groups[0].1[1], "the two triangles of one face stay apart");
    p.params[1] = 0.0;
    let r = p.run(0, snap).unwrap();
    assert_eq!(r.groups[0].1[0], r.groups[0].1[1]);
}

#[test]
fn hide_faces_by_angle_uses_the_axis_and_can_protect_a_mask() {
    let mut p = bundled("hide_angle");
    assert!(p.manifest.commands[0].preview);
    let cube = cube();
    p.params[0] = 1.0; // +Y
    p.params[1] = 45.0;
    p.params[2] = 0.0; // not inverted
    p.params[3] = 0.0; // don't protect the mask
    let r = p.run(0, cube_snapshot()).unwrap();
    let hidden = &r.hidden[0].1;
    assert_eq!(hidden.len(), 12);
    for (i, t) in cube.triangles.iter().enumerate() {
        let dot = face_normal(&cube.positions, *t).dot(Vec3::Y);
        let want = dot < (45.0f32).to_radians().cos();
        assert_eq!(hidden[i], want, "triangle {i} dot {dot}");
    }
    assert_eq!(r.log, vec!["Hid faces by angle"]);
    // Invert keeps the faces that were hidden.
    p.params[2] = 1.0;
    let r = p.run(0, cube_snapshot()).unwrap();
    for (i, t) in cube.triangles.iter().enumerate() {
        let dot = face_normal(&cube.positions, *t).dot(Vec3::Y);
        let away = dot < (45.0f32).to_radians().cos();
        assert_eq!(r.hidden[0].1[i], !away, "inverted triangle {i}");
    }
    // A fully masked piece keeps the visibility it had.
    p.params[2] = 0.0;
    p.params[3] = 1.0;
    let mut snap = cube_snapshot();
    snap.pieces[0].mask.fill(1.0);
    snap.pieces[0].hidden[4] = true;
    let r = p.run(0, snap).unwrap();
    assert!(r.hidden[0].1[4], "the face that was hidden stays hidden");
    assert_eq!(r.hidden[0].1.iter().filter(|h| **h).count(), 1);
}

#[test]
fn color_by_group_paints_one_group_or_every_group_and_respects_the_mask() {
    let mut p = bundled("color_group");
    assert!(p.manifest.commands[0].preview);
    assert!(matches!(p.manifest.params[2].kind, ParamKind::Color(_)));
    let mut snap = cube_snapshot();
    // The -Z face (triangles 0 and 1) is group 1; vertices 0, 1, 2 and 3.
    snap.pieces[0].groups[0] = 1;
    snap.pieces[0].groups[1] = 1;
    snap.pieces[0].mask[0] = 1.0;
    p.params[0] = 0.0; // selected group
    p.params[1] = 1.0;
    p.param_colors[2] = Vec3::new(0.0, 1.0, 0.0);
    p.params[3] = 1.0;
    let r = p.run(0, snap.clone()).unwrap();
    let colors = &r.colors[0].1;
    assert_eq!(colors[0], Vec3::ONE, "masked vertex keeps white");
    for (v, color) in colors.iter().enumerate().take(4).skip(1) {
        assert!((*color - Vec3::Y).length() < 1e-5, "vertex {v} is the chosen green, got {color}");
    }
    for (v, color) in colors.iter().enumerate().skip(4) {
        assert_eq!(*color, Vec3::ONE, "vertex {v} is not in the group");
    }
    assert_eq!(r.log, vec!["Colored by face group"]);
    // Half strength blends from the color that was there.
    p.params[3] = 0.5;
    let r = p.run(0, snap.clone()).unwrap();
    assert!((r.colors[0].1[1] - Vec3::new(0.5, 1.0, 0.5)).length() < 1e-4);
    // Every group: group 2 is not the selected one, so it gets its own hue.
    snap.pieces[0].groups.fill(2);
    p.params[0] = 1.0;
    p.params[1] = 1.0;
    p.params[3] = 1.0;
    p.param_colors[2] = Vec3::X;
    let r = p.run(0, snap).unwrap();
    let painted = r.colors[0].1[1];
    assert!(painted.distance(Vec3::ONE) > 0.2, "not left white: {painted}");
    assert!(painted.distance(Vec3::X) > 0.2, "not the picked color: {painted}");
    assert_eq!(r.colors[0].1[0], Vec3::ONE, "masked vertex still stays");
}

#[test]
fn fill_holes_caps_an_opening_without_moving_the_surface() {
    let mut p = bundled("fill_holes");
    assert!(!p.manifest.commands[0].preview);
    assert!(p.manifest.capabilities.edit_topology && p.manifest.capabilities.edit_mesh);
    let whole = cube();
    let whole_volume = signed_volume(&whole.positions, &whole.triangles);
    // Drop the -Z face. The opening is a quad.
    let mut open = whole.clone();
    open.triangles.drain(0..2);
    open.groups.drain(0..2);
    open.hidden.drain(0..2);
    open.groups[3] = 9;
    open.hidden[2] = true;
    open.mask[6] = 0.4;
    open.colors[4] = Vec3::X;
    let before_tris = open.triangles.clone();
    let snap = Snapshot {
        pieces: vec![open.clone()],
        vertex_counts: vec![8],
        triangle_counts: vec![before_tris.len()],
        active: 0,
        scene_json: String::new(),
    };
    p.params[0] = 128.0;
    p.params[1] = 1.0;
    let r = p.run(0, snap.clone()).unwrap();
    assert_eq!(r.log, vec!["Filled 1 hole"]);
    let mesh = &r.meshes[0].1;
    assert_eq!(mesh.positions, open.positions, "existing vertices stay put");
    assert_eq!(&mesh.triangles[..before_tris.len()], before_tris.as_slice());
    assert_eq!(
        boundary_edges(&mesh.triangles),
        0,
        "the opening is closed\nlog {:?}\nbefore {:?}\nafter {:?}",
        r.log,
        before_tris,
        mesh.triangles
    );
    assert_mesh_ok(&mesh.positions, &mesh.triangles);
    assert!((signed_volume(&mesh.positions, &mesh.triangles) - whole_volume).abs() < 1e-3);
    assert_eq!(r.masks[0].1[6], 0.4);
    assert_eq!(r.colors[0].1[4], Vec3::X);
    assert_eq!(r.groups[0].1[3], 9, "a kept face keeps its group");
    assert!(r.hidden[0].1[2]);
    let patch: Vec<u32> = r.groups[0].1[before_tris.len()..].to_vec();
    assert!(patch.iter().all(|&g| g == patch[0] && g > 9), "the patch gets a new group {patch:?}");
    assert!(r.hidden[0].1[before_tris.len()..].iter().all(|h| !h));
    assert_eq!(p.progress().fraction(), Some(1.0));
    // A closed piece has nothing to do, and nothing is written.
    let r = p.run(0, cube_snapshot()).unwrap();
    assert!(r.meshes.is_empty());
    assert_eq!(r.log, vec!["No holes to fill"]);
    // The quad is larger than a 3-edge limit, so it is left open.
    p.params[0] = 3.0;
    let r = p.run(0, snap).unwrap();
    assert!(r.meshes.is_empty(), "{:?}", r.log);
    assert_eq!(r.log, vec!["Holes are larger than the limit"]);
}

#[test]
fn simplify_reduces_a_mesh_and_keeps_open_edges() {
    let mut p = bundled("simplify");
    assert!(!p.manifest.commands[0].preview);
    assert!(p.manifest.capabilities.edit_topology);
    // A closed octahedron can lose triangles; it stays closed and finite.
    p.params[0] = 50.0;
    let r = p.run(0, octahedron_snapshot()).unwrap();
    assert!(r.log.iter().any(|l| l.starts_with("Removed ")), "{:?}", r.log);
    let mesh = &r.meshes[0].1;
    assert!(mesh.triangles.len() < 8, "got {} triangles: {}", mesh.triangles.len(), r.log[0]);
    assert_eq!(boundary_edges(&mesh.triangles), 0);
    assert_mesh_ok(&mesh.positions, &mesh.triangles);
    assert!(mesh.positions.iter().all(|v| v.length() <= 1.0 + 1e-4));
    assert_eq!(r.groups[0].1.len(), mesh.triangles.len());
    assert_eq!(r.masks[0].1.len(), mesh.positions.len());
    assert_eq!(r.colors[0].1.len(), mesh.positions.len());
    assert_eq!(r.hidden[0].1.len(), mesh.triangles.len());
    assert_eq!(p.progress().fraction(), Some(1.0));
    // A grid keeps its outline. Fully masked interior vertices stay put,
    // and the face group and a corner's color survive.
    let mut snap = grid_snapshot(6);
    let n = 6usize;
    let corner = |x, z| z * n + x;
    snap.pieces[0].mask[corner(2, 2)] = 1.0;
    let boundary_before = boundary_edges(&snap.pieces[0].triangles);
    let tris_before = snap.pieces[0].triangles.len();
    p.params[0] = 40.0;
    let r = p.run(0, snap).unwrap();
    let mesh = &r.meshes[0].1;
    assert!(mesh.triangles.len() < tris_before, "{} -> {}", tris_before, mesh.triangles.len());
    assert_eq!(boundary_edges(&mesh.triangles), boundary_before, "open edges stay: {:?}", r.log);
    // Every vertex of the outline was locked, so it is still here, unmoved.
    for z in 0..n {
        for x in 0..n {
            if x == 0 || z == 0 || x + 1 == n || z + 1 == n {
                let want = Vec3::new(x as f32, 0.0, z as f32);
                assert!(
                    mesh.positions.iter().any(|v| (*v - want).length() < 1e-4),
                    "boundary vertex {want} gone ({:?})",
                    r.log
                );
            }
        }
    }
    assert_mesh_ok(&mesh.positions, &mesh.triangles);
    for (x, z) in [(0, 0), (5, 0), (0, 5), (5, 5)] {
        let want = Vec3::new(x as f32, 0.0, z as f32);
        assert!(mesh.positions.iter().any(|v| (*v - want).length() < 1e-4), "corner {want} gone");
    }
    let protected = Vec3::new(2.0, 0.0, 2.0);
    let kept = mesh.positions.iter().position(|v| (*v - protected).length() < 1e-4).expect("masked vertex moved");
    assert!((r.masks[0].1[kept] - 1.0).abs() < 1e-5);
    assert!(r.groups[0].1.iter().all(|&g| g == 3));
    let corner_color = snap_color_at(&mesh.positions, &r.colors[0].1, Vec3::new(0.0, 0.0, 0.0));
    assert!(corner_color.x < 0.05 && corner_color.z < 0.05, "{corner_color}");
    // Nothing asked for: no rewrite.
    p.params[0] = 0.0;
    let r = p.run(0, octahedron_snapshot()).unwrap();
    assert!(r.meshes.is_empty());
    assert_eq!(r.log, vec!["Nothing to remove"]);
}

fn snap_color_at(pos: &[Vec3], colors: &[Vec3], at: Vec3) -> Vec3 {
    let i = pos.iter().position(|v| (*v - at).length() < 1e-4).expect("vertex");
    colors[i]
}

#[test]
fn showcase_plugins_stop_before_writing() {
    for name in ["group_angle", "hide_angle", "color_group", "fill_holes", "simplify"] {
        let mut p = bundled(name);
        p.progress().cancel();
        let snap = if name == "simplify" { octahedron_snapshot() } else { cube_snapshot() };
        let r = p.run(0, snap).unwrap();
        assert!(r.cancelled, "{name}");
        assert!(r.groups.is_empty() && r.hidden.is_empty() && r.colors.is_empty() && r.meshes.is_empty(), "{name}");
        assert!(
            r.log.iter().all(|l| !l.contains("Grouped")
                && !l.contains("Hid")
                && !l.contains("Colored")
                && !l.contains("Filled")
                && !l.contains("Removed")),
            "{name}: {l:?}",
            l = r.log
        );
    }
}
