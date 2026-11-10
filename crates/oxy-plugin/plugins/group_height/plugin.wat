;; Group by Height: an in-app OxyBrush plugin in WebAssembly text (API 3).
;; Gives each triangle of the active piece a face group from where its
;; centre lies along an axis: the piece's extent is cut into equal bands.
;; Shows: a choice param (the axis), a whole-number slider (the bands),
;; write_groups, and progress (which also tells it to stop early).
;;
;; Memory: positions at 1024, then the triangles, then the groups.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "triangle_count" (func $triangle_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_triangles" (func $read_triangles (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_groups" (func $write_groups (param i32 i32 i32) (result i32)))
  (import "oxybrush" "progress" (func $progress (param f32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Grouped into bands")
  (data (i32.const 32) "The piece is too large for a plugin")

  (func $ensure (param $end i32) (result i32)
    (local $need i32)
    (if (i32.lt_s (local.get $end) (i32.const 0)) (then (return (i32.const 0))))
    (local.set $need (i32.div_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then
        (if (i32.eq (memory.grow (i32.sub (local.get $need) (memory.size))) (i32.const -1))
          (then (return (i32.const 0))))))
    (i32.const 1))

  ;; Coordinate `axis` (0, 1, 2) of vertex `v`.
  (func $coord (param $v i32) (param $axis i32) (result f32)
    (f32.load (i32.add (i32.const 1024)
      (i32.add (i32.mul (local.get $v) (i32.const 12)) (i32.mul (local.get $axis) (i32.const 4))))))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $nt i32) (local $tris i32) (local $groups i32)
    (local $axis i32) (local $bands i32) (local $i i32) (local $t i32) (local $band i32)
    (local $lo f32) (local $hi f32) (local $c f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (local.set $nt (call $triangle_count (local.get $piece)))
    (if (i32.or (i32.le_s (local.get $n) (i32.const 0)) (i32.le_s (local.get $nt) (i32.const 0))) (then (return)))
    (if (i32.or (i32.gt_u (local.get $n) (i32.const 20000000)) (i32.gt_u (local.get $nt) (i32.const 20000000)))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (local.set $tris (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $groups (i32.add (local.get $tris) (i32.mul (local.get $nt) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $groups) (i32.mul (local.get $nt) (i32.const 4)))))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_triangles (local.get $piece) (local.get $tris) (local.get $nt)))
    (local.set $axis (i32.trunc_f32_s (call $param (i32.const 0))))
    (local.set $bands (i32.trunc_f32_s (call $param (i32.const 1))))
    (if (i32.lt_s (local.get $bands) (i32.const 1)) (then (local.set $bands (i32.const 1))))
    ;; The extent along the axis.
    (local.set $lo (f32.const inf))
    (local.set $hi (f32.const -inf))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $c (call $coord (local.get $i) (local.get $axis)))
      (local.set $lo (f32.min (local.get $lo) (local.get $c)))
      (local.set $hi (f32.max (local.get $hi) (local.get $c)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (local.set $hi (f32.max (f32.sub (local.get $hi) (local.get $lo)) (f32.const 1e-9)))
    ;; Each triangle's band, from its centre.
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $t) (local.get $nt)))
      ;; Every 65536 triangles: report progress, and stop if asked to.
      (if (i32.eqz (i32.and (local.get $t) (i32.const 65535)))
        (then
          (if (call $progress (f32.div (f32.convert_i32_u (local.get $t)) (f32.convert_i32_u (local.get $nt))))
            (then (return)))))
      (local.set $i (i32.add (local.get $tris) (i32.mul (local.get $t) (i32.const 12))))
      (local.set $c
        (f32.mul (f32.const 0.33333334)
          (f32.add (call $coord (i32.load (local.get $i)) (local.get $axis))
            (f32.add (call $coord (i32.load offset=4 (local.get $i)) (local.get $axis))
                     (call $coord (i32.load offset=8 (local.get $i)) (local.get $axis))))))
      (local.set $band
        (i32.trunc_sat_f32_s
          (f32.mul (f32.div (f32.sub (local.get $c) (local.get $lo)) (local.get $hi))
                   (f32.convert_i32_s (local.get $bands)))))
      (if (i32.ge_s (local.get $band) (local.get $bands))
        (then (local.set $band (i32.sub (local.get $bands) (i32.const 1)))))
      (if (i32.lt_s (local.get $band) (i32.const 0)) (then (local.set $band (i32.const 0))))
      ;; Groups 1..=bands (0 is the usual "no group").
      (i32.store (i32.add (local.get $groups) (i32.mul (local.get $t) (i32.const 4)))
        (i32.add (local.get $band) (i32.const 1)))
      (local.set $t (i32.add (local.get $t) (i32.const 1)))
      (br $each)))
    (drop (call $progress (f32.const 1)))
    (drop (call $write_groups (local.get $piece) (local.get $groups) (local.get $nt)))
    (call $log (i32.const 0) (i32.const 18))))
