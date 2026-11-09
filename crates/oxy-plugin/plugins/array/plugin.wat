;; Array: an in-app OxyBrush plugin in WebAssembly text (RFC 0011).
;; Adds `copies` new pieces, each the active piece moved along one axis by
;; its own size plus a gap (a percentage of that size, so 0 makes copies
;; touch and a negative gap overlaps them). The copies are added as one
;; undo step; the last becomes active.
;;
;; Memory: name at 0, positions at 1024, then triangles, then the moved
;; copy of the positions.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "triangle_count" (func $triangle_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_triangles" (func $read_triangles (param i32 i32 i32) (result i32)))
  (import "oxybrush" "add_piece" (func $add_piece (param i32 i32 i32 i32 i32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Copy")
  (data (i32.const 16) "Added copies")
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

  ;; `command` 0, 1, 2 is the axis X, Y, Z (its byte offset in a vertex is 4 × axis).
  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $t i32) (local $tris i32) (local $copy i32)
    (local $i i32) (local $k i32) (local $copies i32) (local $axis i32) (local $p i32)
    (local $lo f32) (local $hi f32) (local $v f32) (local $step f32) (local $shift f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (local.set $t (call $triangle_count (local.get $piece)))
    (if (i32.or (i32.le_s (local.get $n) (i32.const 0)) (i32.le_s (local.get $t) (i32.const 0)))
      (then (return)))
    (if (i32.or (i32.gt_u (local.get $n) (i32.const 10000000)) (i32.gt_u (local.get $t) (i32.const 20000000)))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (local.set $tris (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $copy (i32.add (local.get $tris) (i32.mul (local.get $t) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $copy) (i32.mul (local.get $n) (i32.const 12)))))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_triangles (local.get $piece) (local.get $tris) (local.get $t)))
    (local.set $axis (i32.mul (i32.rem_u (local.get $command) (i32.const 3)) (i32.const 4)))
    ;; Size along the axis.
    (local.set $lo (f32.const inf))
    (local.set $hi (f32.const -inf))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $v
        (f32.load (i32.add (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12))) (local.get $axis))))
      (local.set $lo (f32.min (local.get $lo) (local.get $v)))
      (local.set $hi (f32.max (local.get $hi) (local.get $v)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (local.set $step
      (f32.mul (f32.sub (local.get $hi) (local.get $lo))
        (f32.add (f32.const 1) (f32.mul (call $param (i32.const 1)) (f32.const 0.01)))))
    (local.set $copies (i32.trunc_sat_f32_u (f32.nearest (call $param (i32.const 0)))))
    (local.set $k (i32.const 1))
    (block $all (loop $each_copy
      (br_if $all (i32.gt_u (local.get $k) (local.get $copies)))
      (local.set $shift (f32.mul (local.get $step) (f32.convert_i32_u (local.get $k))))
      (memory.copy (local.get $copy) (i32.const 1024) (i32.mul (local.get $n) (i32.const 12)))
      (local.set $i (i32.const 0))
      (block $moved (loop $move
        (br_if $moved (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $p (i32.add (i32.add (local.get $copy) (i32.mul (local.get $i) (i32.const 12))) (local.get $axis)))
        (f32.store (local.get $p) (f32.add (f32.load (local.get $p)) (local.get $shift)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $move)))
      (drop (call $add_piece (i32.const 0) (i32.const 4) (local.get $copy) (local.get $n) (local.get $tris) (local.get $t)))
      (local.set $k (i32.add (local.get $k) (i32.const 1)))
      (br $each_copy)))
    (call $log (i32.const 16) (i32.const 12))))
