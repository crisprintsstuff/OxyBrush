;; Group by Angle: an in-app OxyBrush plugin in WebAssembly text (API 3).
;; Faces that meet more gently than the angle share a face group; a sharper
;; crease starts a new one. Regions are joined with a union-find over edges
;; whose face normals agree, which is the dihedral test (the angle between
;; the normals). "Keep existing splits" refuses to join faces that already
;; carry different non-zero groups.
;;
;; Shows: a stepped slider, a toggle, write_groups, live preview, progress
;; and Stop, and growing memory for the edge table.
;;
;; Memory: strings below 1024. Then positions, triangles, face normals,
;; union-find parent and rank, the root's group id, the groups already on
;; the piece, the groups written back, and an open-addressed edge table.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "triangle_count" (func $triangle_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_triangles" (func $read_triangles (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_groups" (func $read_groups (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_groups" (func $write_groups (param i32 i32 i32) (result i32)))
  (import "oxybrush" "progress" (func $progress (param f32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Grouped by angle")
  (data (i32.const 64) "The piece is too large for a plugin")
  (data (i32.const 128) "Couldn't read the mesh")
  (data (i32.const 192) "Couldn't change the mesh")

  (global $gpos (mut i32) (i32.const 0))
  (global $gtri (mut i32) (i32.const 0))
  (global $gnorm (mut i32) (i32.const 0))
  (global $gparent (mut i32) (i32.const 0))
  (global $grank (mut i32) (i32.const 0))
  (global $groot (mut i32) (i32.const 0))
  (global $gexist (mut i32) (i32.const 0))
  (global $gout (mut i32) (i32.const 0))
  (global $ghash (mut i32) (i32.const 0))
  (global $ghashm (mut i32) (i32.const 0))
  (global $gn (mut i32) (i32.const 0))
  (global $gnt (mut i32) (i32.const 0))

  (func $ensure (param $end i32) (result i32)
    (local $need i32)
    (if (i32.lt_s (local.get $end) (i32.const 0)) (then (return (i32.const 0))))
    (local.set $need (i32.div_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then
        (if (i32.eq (memory.grow (i32.sub (local.get $need) (memory.size))) (i32.const -1))
          (then (return (i32.const 0))))))
    (i32.const 1))

  (func $too_big
    (call $log (i32.const 64) (i32.const 35)))

  (func $bad_read
    (call $log (i32.const 128) (i32.const 22)))

  ;; cos of an angle in degrees, 0..=180. A Taylor series in radians, folded
  ;; with Horner's method; accurate to about 1e-5, which is enough to compare
  ;; with a dot product of unit normals.
  (func $cos_deg (param $deg f32) (result f32)
    (local $x2 f32) (local $y f32)
    (local.set $x2 (f32.mul (local.get $deg) (f32.const 0.017453292)))
    (local.set $x2 (f32.mul (local.get $x2) (local.get $x2)))
    (local.set $y (f32.const -1.1470746e-11))
    (local.set $y (f32.add (f32.const 2.0876757e-9) (f32.mul (local.get $x2) (local.get $y))))
    (local.set $y (f32.add (f32.const -2.7557319e-7) (f32.mul (local.get $x2) (local.get $y))))
    (local.set $y (f32.add (f32.const 2.4801587e-5) (f32.mul (local.get $x2) (local.get $y))))
    (local.set $y (f32.add (f32.const -0.0013888889) (f32.mul (local.get $x2) (local.get $y))))
    (local.set $y (f32.add (f32.const 0.041666668) (f32.mul (local.get $x2) (local.get $y))))
    (local.set $y (f32.add (f32.const -0.5) (f32.mul (local.get $x2) (local.get $y))))
    (f32.add (f32.const 1) (f32.mul (local.get $x2) (local.get $y))))

  (func $px (param $v i32) (result f32)
    (f32.load (i32.add (global.get $gpos) (i32.mul (local.get $v) (i32.const 12)))))
  (func $py (param $v i32) (result f32)
    (f32.load offset=4 (i32.add (global.get $gpos) (i32.mul (local.get $v) (i32.const 12)))))
  (func $pz (param $v i32) (result f32)
    (f32.load offset=8 (i32.add (global.get $gpos) (i32.mul (local.get $v) (i32.const 12)))))
  (func $corner (param $t i32) (param $k i32) (result i32)
    (i32.load (i32.add (global.get $gtri)
      (i32.add (i32.mul (local.get $t) (i32.const 12)) (i32.mul (local.get $k) (i32.const 4))))))

  (func $naddr (param $t i32) (result i32)
    (i32.add (global.get $gnorm) (i32.mul (local.get $t) (i32.const 12))))

  (func $nzero (param $t i32) (result i32)
    (local $p i32)
    (local.set $p (call $naddr (local.get $t)))
    (i32.and
      (i32.and (f32.eq (f32.load (local.get $p)) (f32.const 0))
               (f32.eq (f32.load offset=4 (local.get $p)) (f32.const 0)))
      (f32.eq (f32.load offset=8 (local.get $p)) (f32.const 0))))

  (func $compute_n (param $t i32)
    (local $a i32) (local $b i32) (local $c i32)
    (local $ux f32) (local $uy f32) (local $uz f32)
    (local $vx f32) (local $vy f32) (local $vz f32)
    (local $nx f32) (local $ny f32) (local $nz f32) (local $len f32)
    (local $p i32)
    (local.set $a (call $corner (local.get $t) (i32.const 0)))
    (local.set $b (call $corner (local.get $t) (i32.const 1)))
    (local.set $c (call $corner (local.get $t) (i32.const 2)))
    (local.set $ux (f32.sub (call $px (local.get $b)) (call $px (local.get $a))))
    (local.set $uy (f32.sub (call $py (local.get $b)) (call $py (local.get $a))))
    (local.set $uz (f32.sub (call $pz (local.get $b)) (call $pz (local.get $a))))
    (local.set $vx (f32.sub (call $px (local.get $c)) (call $px (local.get $a))))
    (local.set $vy (f32.sub (call $py (local.get $c)) (call $py (local.get $a))))
    (local.set $vz (f32.sub (call $pz (local.get $c)) (call $pz (local.get $a))))
    (local.set $nx (f32.sub (f32.mul (local.get $uy) (local.get $vz)) (f32.mul (local.get $uz) (local.get $vy))))
    (local.set $ny (f32.sub (f32.mul (local.get $uz) (local.get $vx)) (f32.mul (local.get $ux) (local.get $vz))))
    (local.set $nz (f32.sub (f32.mul (local.get $ux) (local.get $vy)) (f32.mul (local.get $uy) (local.get $vx))))
    (local.set $len (f32.sqrt (f32.add (f32.mul (local.get $nx) (local.get $nx))
      (f32.add (f32.mul (local.get $ny) (local.get $ny)) (f32.mul (local.get $nz) (local.get $nz))))))
    (local.set $p (call $naddr (local.get $t)))
    (if (f32.lt (local.get $len) (f32.const 1e-12))
      (then
        (f32.store (local.get $p) (f32.const 0))
        (f32.store offset=4 (local.get $p) (f32.const 0))
        (f32.store offset=8 (local.get $p) (f32.const 0))
        (return)))
    (f32.store (local.get $p) (f32.div (local.get $nx) (local.get $len)))
    (f32.store offset=4 (local.get $p) (f32.div (local.get $ny) (local.get $len)))
    (f32.store offset=8 (local.get $p) (f32.div (local.get $nz) (local.get $len))))

  ;; 2 when either normal is degenerate, so the caller does not join on it.
  (func $ndot (param $f0 i32) (param $f1 i32) (result f32)
    (local $p0 i32) (local $p1 i32)
    (if (i32.or (call $nzero (local.get $f0)) (call $nzero (local.get $f1)))
      (then (return (f32.const 2))))
    (local.set $p0 (call $naddr (local.get $f0)))
    (local.set $p1 (call $naddr (local.get $f1)))
    (f32.add
      (f32.mul (f32.load (local.get $p0)) (f32.load (local.get $p1)))
      (f32.add
        (f32.mul (f32.load offset=4 (local.get $p0)) (f32.load offset=4 (local.get $p1)))
        (f32.mul (f32.load offset=8 (local.get $p0)) (f32.load offset=8 (local.get $p1))))))

  (func $find (param $x i32) (result i32)
    (local $r i32) (local $p i32) (local $hops i32)
    (local.set $r (local.get $x))
    (block $up (loop $walk
      (local.set $p (i32.load (i32.add (global.get $gparent) (i32.mul (local.get $r) (i32.const 4)))))
      (br_if $up (i32.eq (local.get $p) (local.get $r)))
      (local.set $r (local.get $p))
      (local.set $hops (i32.add (local.get $hops) (i32.const 1)))
      (br_if $up (i32.gt_u (local.get $hops) (global.get $gnt)))
      (br $walk)))
    (block $comp (loop $flat
      (br_if $comp (i32.eq (local.get $x) (local.get $r)))
      (local.set $p (i32.load (i32.add (global.get $gparent) (i32.mul (local.get $x) (i32.const 4)))))
      (i32.store (i32.add (global.get $gparent) (i32.mul (local.get $x) (i32.const 4))) (local.get $r))
      (local.set $x (local.get $p))
      (br $flat)))
    (local.get $r))

  (func $union (param $a i32) (param $b i32)
    (local $ra i32) (local $rb i32)
    (local.set $a (call $find (local.get $a)))
    (local.set $b (call $find (local.get $b)))
    (if (i32.eq (local.get $a) (local.get $b)) (then (return)))
    (local.set $ra (i32.load (i32.add (global.get $grank) (i32.mul (local.get $a) (i32.const 4)))))
    (local.set $rb (i32.load (i32.add (global.get $grank) (i32.mul (local.get $b) (i32.const 4)))))
    (if (i32.lt_u (local.get $ra) (local.get $rb))
      (then
        (local.set $ra (local.get $a))
        (local.set $a (local.get $b))
        (local.set $b (local.get $ra))
        (local.set $ra (i32.load (i32.add (global.get $grank) (i32.mul (local.get $a) (i32.const 4)))))
        (local.set $rb (i32.load (i32.add (global.get $grank) (i32.mul (local.get $b) (i32.const 4)))))))
    (i32.store (i32.add (global.get $gparent) (i32.mul (local.get $b) (i32.const 4))) (local.get $a))
    (if (i32.eq (local.get $ra) (local.get $rb))
      (then
        (i32.store (i32.add (global.get $grank) (i32.mul (local.get $a) (i32.const 4)))
          (i32.add (local.get $ra) (i32.const 1))))))

  (func $maybe_union (param $f0 i32) (param $f1 i32) (param $cos f32) (param $respect i32)
    (local $d f32) (local $g0 i32) (local $g1 i32)
    (if (i32.eq (local.get $f0) (local.get $f1)) (then (return)))
    (local.set $d (call $ndot (local.get $f0) (local.get $f1)))
    (if (f32.gt (local.get $d) (f32.const 1.5)) (then (return)))
    ;; Join when the angle between the normals is within the threshold,
    ;; which is dot >= cos(threshold).
    (if (f32.lt (local.get $d) (local.get $cos)) (then (return)))
    (if (local.get $respect)
      (then
        (local.set $g0 (i32.load (i32.add (global.get $gexist) (i32.mul (local.get $f0) (i32.const 4)))))
        (local.set $g1 (i32.load (i32.add (global.get $gexist) (i32.mul (local.get $f1) (i32.const 4)))))
        (if (i32.and (i32.and (i32.ne (local.get $g0) (i32.const 0)) (i32.ne (local.get $g1) (i32.const 0)))
                     (i32.ne (local.get $g0) (local.get $g1)))
          (then (return)))))
    (call $union (local.get $f0) (local.get $f1)))

  ;; Record edge (va, vb) on `face`. The second triangle of a manifold edge
  ;; is joined with the first when the angle allows; a third is left alone.
  (func $consider (param $va i32) (param $vb i32) (param $face i32) (param $cos f32) (param $respect i32)
    (local $tmp i32) (local $h i32) (local $slot i32) (local $probes i32) (local $other i32)
    (if (i32.gt_u (local.get $va) (local.get $vb))
      (then
        (local.set $tmp (local.get $va))
        (local.set $va (local.get $vb))
        (local.set $vb (local.get $tmp))))
    (local.set $h
      (i32.and
        (i32.xor
          (i32.mul (local.get $va) (i32.const 0x45d9f3b))
          (i32.mul (local.get $vb) (i32.const 0x27d4eb2d)))
        (global.get $ghashm)))
    (local.set $probes (i32.const 0))
    (block $done (loop $probe
      (local.set $slot (i32.add (global.get $ghash) (i32.mul (local.get $h) (i32.const 12))))
      (local.set $tmp (i32.load (local.get $slot)))
      (if (i32.eq (local.get $tmp) (i32.const -1))
        (then
          (i32.store (local.get $slot) (local.get $va))
          (i32.store offset=4 (local.get $slot) (local.get $vb))
          (i32.store offset=8 (local.get $slot) (local.get $face))
          (return)))
      (if (i32.and (i32.eq (local.get $tmp) (local.get $va))
                   (i32.eq (i32.load offset=4 (local.get $slot)) (local.get $vb)))
        (then
          (local.set $other (i32.load offset=8 (local.get $slot)))
          ;; A second, different face closes the edge. A third (or the same
          ;; face twice, a degenerate) must not join anything else.
          (if (i32.and (i32.ge_s (local.get $other) (i32.const 0)) (i32.ne (local.get $other) (local.get $face)))
            (then
              (i32.store offset=8 (local.get $slot) (i32.const -2))
              (call $maybe_union (local.get $face) (local.get $other) (local.get $cos) (local.get $respect))))
          (return)))
      (local.set $h (i32.and (i32.add (local.get $h) (i32.const 1)) (global.get $ghashm)))
      (local.set $probes (i32.add (local.get $probes) (i32.const 1)))
      (br_if $done (i32.gt_u (local.get $probes) (global.get $ghashm)))
      (br $probe))))

  (func $pow2 (param $n i32) (result i32)
    (local $p i32)
    (local.set $p (i32.const 1))
    (block $done (loop $shift
      (br_if $done (i32.ge_u (local.get $p) (local.get $n)))
      (local.set $p (i32.shl (local.get $p) (i32.const 1)))
      (br_if $done (i32.le_s (local.get $p) (i32.const 0)))
      (br $shift)))
    (local.get $p))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $nt i32)
    (local $slots i32) (local $end i32) (local $i i32) (local $k i32)
    (local $deg i32) (local $respect i32) (local $cos f32)
    (local $root i32) (local $id i32) (local $next i32) (local $rc i32)
    (if (call $progress (f32.const 0)) (then (return)))
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (local.set $nt (call $triangle_count (local.get $piece)))
    (if (i32.or (i32.lt_s (local.get $n) (i32.const 0)) (i32.lt_s (local.get $nt) (i32.const 0)))
      (then (call $bad_read) (return)))
    (if (i32.or (i32.le_s (local.get $n) (i32.const 0)) (i32.le_s (local.get $nt) (i32.const 0)))
      (then (return)))
    (if (i32.or (i32.gt_u (local.get $n) (i32.const 2000000)) (i32.gt_u (local.get $nt) (i32.const 2000000)))
      (then (call $too_big) (return)))
    (global.set $gn (local.get $n))
    (global.set $gnt (local.get $nt))
    (global.set $gpos (i32.const 1024))
    (global.set $gtri (i32.add (global.get $gpos) (i32.mul (local.get $n) (i32.const 12))))
    (global.set $gnorm (i32.add (global.get $gtri) (i32.mul (local.get $nt) (i32.const 12))))
    (global.set $gparent (i32.add (global.get $gnorm) (i32.mul (local.get $nt) (i32.const 12))))
    (global.set $grank (i32.add (global.get $gparent) (i32.mul (local.get $nt) (i32.const 4))))
    (global.set $groot (i32.add (global.get $grank) (i32.mul (local.get $nt) (i32.const 4))))
    (global.set $gexist (i32.add (global.get $groot) (i32.mul (local.get $nt) (i32.const 4))))
    (global.set $gout (i32.add (global.get $gexist) (i32.mul (local.get $nt) (i32.const 4))))
    (local.set $slots (call $pow2 (i32.mul (local.get $nt) (i32.const 8))))
    (if (i32.le_s (local.get $slots) (i32.const 0)) (then (call $too_big) (return)))
    (global.set $ghash (i32.add (global.get $gout) (i32.mul (local.get $nt) (i32.const 4))))
    (global.set $ghashm (i32.sub (local.get $slots) (i32.const 1)))
    (local.set $end (i32.add (global.get $ghash) (i32.mul (local.get $slots) (i32.const 12))))
    (if (i32.eqz (call $ensure (local.get $end))) (then (call $too_big) (return)))
    (local.set $rc (call $read_positions (local.get $piece) (global.get $gpos) (local.get $n)))
    (if (i32.ne (local.get $rc) (local.get $n)) (then (call $bad_read) (return)))
    (local.set $rc (call $read_triangles (local.get $piece) (global.get $gtri) (local.get $nt)))
    (if (i32.ne (local.get $rc) (local.get $nt)) (then (call $bad_read) (return)))
    (local.set $rc (call $read_groups (local.get $piece) (global.get $gexist) (local.get $nt)))
    (if (i32.ne (local.get $rc) (local.get $nt)) (then (call $bad_read) (return)))
    (local.set $deg (i32.trunc_sat_f32_s (call $param (i32.const 0))))
    (if (i32.lt_s (local.get $deg) (i32.const 0)) (then (local.set $deg (i32.const 0))))
    (if (i32.gt_s (local.get $deg) (i32.const 180)) (then (local.set $deg (i32.const 180))))
    (local.set $cos (call $cos_deg (f32.convert_i32_s (local.get $deg))))
    (local.set $respect (f32.ge (call $param (i32.const 1)) (f32.const 0.5)))
    ;; Normals, then a fresh union-find, then an empty edge table.
    (local.set $i (i32.const 0))
    (block $dn (loop $normals
      (br_if $dn (i32.ge_u (local.get $i) (local.get $nt)))
      (if (i32.eqz (i32.and (local.get $i) (i32.const 65535)))
        (then
          (if (call $progress (f32.mul (f32.const 0.3)
                (f32.div (f32.convert_i32_u (local.get $i)) (f32.convert_i32_u (local.get $nt)))))
            (then (return)))))
      (call $compute_n (local.get $i))
      (i32.store (i32.add (global.get $gparent) (i32.mul (local.get $i) (i32.const 4))) (local.get $i))
      (i32.store (i32.add (global.get $grank) (i32.mul (local.get $i) (i32.const 4))) (i32.const 0))
      (i32.store (i32.add (global.get $groot) (i32.mul (local.get $i) (i32.const 4))) (i32.const 0))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $normals)))
    (local.set $i (i32.const 0))
    (block $dh (loop $clear
      (br_if $dh (i32.ge_u (local.get $i) (local.get $slots)))
      (if (i32.eqz (i32.and (local.get $i) (i32.const 65535)))
        (then
          (if (call $progress (f32.add (f32.const 0.3) (f32.mul (f32.const 0.2)
                (f32.div (f32.convert_i32_u (local.get $i)) (f32.convert_i32_u (local.get $slots))))))
            (then (return)))))
      (i32.store (i32.add (global.get $ghash) (i32.mul (local.get $i) (i32.const 12))) (i32.const -1))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $clear)))
    (local.set $i (i32.const 0))
    (block $de (loop $edges
      (br_if $de (i32.ge_u (local.get $i) (local.get $nt)))
      (if (i32.eqz (i32.and (local.get $i) (i32.const 65535)))
        (then
          (if (call $progress (f32.add (f32.const 0.5) (f32.mul (f32.const 0.4)
                (f32.div (f32.convert_i32_u (local.get $i)) (f32.convert_i32_u (local.get $nt))))))
            (then (return)))))
      (local.set $k (i32.const 0))
      (block $dk (loop $corn
        (br_if $dk (i32.ge_u (local.get $k) (i32.const 3)))
        (call $consider
          (call $corner (local.get $i) (local.get $k))
          (call $corner (local.get $i) (i32.rem_u (i32.add (local.get $k) (i32.const 1)) (i32.const 3)))
          (local.get $i) (local.get $cos) (local.get $respect))
        (local.set $k (i32.add (local.get $k) (i32.const 1)))
        (br $corn)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $edges)))
    ;; Number the components from 1, in the order their first face appears.
    (local.set $next (i32.const 1))
    (local.set $i (i32.const 0))
    (block $da (loop $assign
      (br_if $da (i32.ge_u (local.get $i) (local.get $nt)))
      (local.set $root (call $find (local.get $i)))
      (local.set $id (i32.load (i32.add (global.get $groot) (i32.mul (local.get $root) (i32.const 4)))))
      (if (i32.eqz (local.get $id))
        (then
          (local.set $id (local.get $next))
          (i32.store (i32.add (global.get $groot) (i32.mul (local.get $root) (i32.const 4))) (local.get $id))
          (local.set $next (i32.add (local.get $next) (i32.const 1)))))
      (i32.store (i32.add (global.get $gout) (i32.mul (local.get $i) (i32.const 4))) (local.get $id))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $assign)))
    (drop (call $progress (f32.const 1)))
    (local.set $rc (call $write_groups (local.get $piece) (global.get $gout) (local.get $nt)))
    (if (i32.lt_s (local.get $rc) (i32.const 0))
      (then (call $log (i32.const 192) (i32.const 24)) (return)))
    (call $log (i32.const 0) (i32.const 16))))
