;; Mask by Height: an in-app OxyBrush plugin in WebAssembly text (RFC 0011).
;; Sets the mask of the active piece from each vertex's height: masked
;; below (or above) a level set as a percentage of the piece's height, with
;; a soft edge. Its overlay draws that level as a square ring around the
;; piece, from the last run or the last measured bounds.
;;
;; Memory: positions at 1024, then the new mask.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_mask" (func $write_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "draw_line" (func $draw_line (param f32 f32 f32 f32 f32 f32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Masked below the height")
  (data (i32.const 32) "Masked above the height")
  (data (i32.const 64) "The piece is too large for a plugin")
  ;; Bounds of the piece at the last run; none yet while $have is 0.
  (global $have (mut i32) (i32.const 0))
  (global $x0 (mut f32) (f32.const 0)) (global $x1 (mut f32) (f32.const 0))
  (global $y0 (mut f32) (f32.const 0)) (global $y1 (mut f32) (f32.const 0))
  (global $z0 (mut f32) (f32.const 0)) (global $z1 (mut f32) (f32.const 0))

  (func $ensure (param $end i32) (result i32)
    (local $need i32)
    (if (i32.lt_s (local.get $end) (i32.const 0)) (then (return (i32.const 0))))
    (local.set $need (i32.div_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then
        (if (i32.eq (memory.grow (i32.sub (local.get $need) (memory.size))) (i32.const -1))
          (then (return (i32.const 0))))))
    (i32.const 1))

  ;; Store the bounds of `n` positions at 1024 in the globals.
  (func $bounds (param $n i32)
    (local $i i32) (local $p i32) (local $v f32)
    (global.set $x0 (f32.const inf)) (global.set $y0 (f32.const inf)) (global.set $z0 (f32.const inf))
    (global.set $x1 (f32.const -inf)) (global.set $y1 (f32.const -inf)) (global.set $z1 (f32.const -inf))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $p (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $v (f32.load (local.get $p)))
      (global.set $x0 (f32.min (global.get $x0) (local.get $v)))
      (global.set $x1 (f32.max (global.get $x1) (local.get $v)))
      (local.set $v (f32.load offset=4 (local.get $p)))
      (global.set $y0 (f32.min (global.get $y0) (local.get $v)))
      (global.set $y1 (f32.max (global.get $y1) (local.get $v)))
      (local.set $v (f32.load offset=8 (local.get $p)))
      (global.set $z0 (f32.min (global.get $z0) (local.get $v)))
      (global.set $z1 (f32.max (global.get $z1) (local.get $v)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (global.set $have (i32.const 1)))

  ;; The level, in world units, from the Height param.
  (func $level (result f32)
    (f32.add (global.get $y0)
      (f32.mul (f32.sub (global.get $y1) (global.get $y0))
        (f32.mul (call $param (i32.const 0)) (f32.const 0.01)))))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $i i32) (local $mask i32)
    (local $h f32) (local $soft f32) (local $d f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (if (i32.le_s (local.get $n) (i32.const 0)) (then (return)))
    (if (i32.gt_u (local.get $n) (i32.const 40000000))
      (then (call $log (i32.const 64) (i32.const 35)) (return)))
    (local.set $mask (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $mask) (i32.mul (local.get $n) (i32.const 4)))))
      (then (call $log (i32.const 64) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (call $bounds (local.get $n))
    (local.set $h (call $level))
    (local.set $soft
      (f32.max
        (f32.mul (f32.sub (global.get $y1) (global.get $y0))
          (f32.mul (call $param (i32.const 1)) (f32.const 0.01)))
        (f32.const 1e-6)))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      ;; Distance below the level (above for "Mask above"), in soft edges.
      (local.set $d
        (f32.sub (local.get $h)
          (f32.load offset=4 (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12))))))
      (if (i32.eq (local.get $command) (i32.const 1)) (then (local.set $d (f32.neg (local.get $d)))))
      (f32.store (i32.add (local.get $mask) (i32.mul (local.get $i) (i32.const 4)))
        (f32.min (f32.const 1)
          (f32.max (f32.const 0)
            (f32.add (f32.div (local.get $d) (local.get $soft)) (f32.const 0.5)))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (drop (call $write_mask (local.get $piece) (local.get $mask) (local.get $n)))
    (call $log (i32.mul (local.get $command) (i32.const 32)) (i32.const 23)))

  ;; The level as a ring around the last bounds (follows the Height slider).
  (func (export "overlay")
    (local $h f32) (local $c i32)
    (if (i32.eqz (global.get $have)) (then (return)))
    (local.set $h (call $level))
    (local.set $c (i32.const 0x40c0ffff))
    (drop (call $draw_line (global.get $x0) (local.get $h) (global.get $z0)
                           (global.get $x1) (local.get $h) (global.get $z0) (local.get $c)))
    (drop (call $draw_line (global.get $x1) (local.get $h) (global.get $z0)
                           (global.get $x1) (local.get $h) (global.get $z1) (local.get $c)))
    (drop (call $draw_line (global.get $x1) (local.get $h) (global.get $z1)
                           (global.get $x0) (local.get $h) (global.get $z1) (local.get $c)))
    (drop (call $draw_line (global.get $x0) (local.get $h) (global.get $z1)
                           (global.get $x0) (local.get $h) (global.get $z0) (local.get $c)))))
