;; Color Ramp: an in-app OxyBrush plugin in WebAssembly text (RFC 0011).
;; "Color by height" paints a ramp from a deep blue at the bottom of the
;; active piece, through a warm orange, to a pale yellow at the top.
;; "Color by direction" gives each vertex the color of its normal
;; (x, y, z as red, green, blue). Masked parts keep their color; Strength
;; blends with the color that was there.
;;
;; Memory: positions at 1024, then normals, mask, and colors (read, then
;; overwritten in place).
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_normals" (func $read_normals (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_mask" (func $read_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_colors" (func $read_colors (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_colors" (func $write_colors (param i32 i32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Colored by height")
  (data (i32.const 32) "Colored by direction")
  (data (i32.const 64) "The piece is too large for a plugin")
  ;; The ramp: three colors (r, g, b) at 0, 0.5 and 1.
  (global $r0 f32 (f32.const 0.10)) (global $g0 f32 (f32.const 0.16)) (global $b0 f32 (f32.const 0.45))
  (global $r1 f32 (f32.const 0.90)) (global $g1 f32 (f32.const 0.45)) (global $b1 f32 (f32.const 0.20))
  (global $r2 f32 (f32.const 1.00)) (global $g2 f32 (f32.const 0.93)) (global $b2 f32 (f32.const 0.70))

  (func $ensure (param $end i32) (result i32)
    (local $need i32)
    (if (i32.lt_s (local.get $end) (i32.const 0)) (then (return (i32.const 0))))
    (local.set $need (i32.div_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then
        (if (i32.eq (memory.grow (i32.sub (local.get $need) (memory.size))) (i32.const -1))
          (then (return (i32.const 0))))))
    (i32.const 1))

  (func $mix (param $a f32) (param $b f32) (param $t f32) (result f32)
    (f32.add (local.get $a) (f32.mul (f32.sub (local.get $b) (local.get $a)) (local.get $t))))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $i i32) (local $p i32) (local $c i32)
    (local $normals i32) (local $mask i32) (local $colors i32)
    (local $y0 f32) (local $y1 f32) (local $t f32) (local $w f32) (local $u f32)
    (local $r f32) (local $g f32) (local $b f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (if (i32.le_s (local.get $n) (i32.const 0)) (then (return)))
    (if (i32.gt_u (local.get $n) (i32.const 15000000))
      (then (call $log (i32.const 64) (i32.const 35)) (return)))
    (local.set $normals (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $mask (i32.add (local.get $normals) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $colors (i32.add (local.get $mask) (i32.mul (local.get $n) (i32.const 4))))
    (if (i32.eqz (call $ensure (i32.add (local.get $colors) (i32.mul (local.get $n) (i32.const 12)))))
      (then (call $log (i32.const 64) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_normals (local.get $piece) (local.get $normals) (local.get $n)))
    (drop (call $read_mask (local.get $piece) (local.get $mask) (local.get $n)))
    (drop (call $read_colors (local.get $piece) (local.get $colors) (local.get $n)))
    ;; Height range.
    (local.set $y0 (f32.const inf))
    (local.set $y1 (f32.const -inf))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $t (f32.load offset=4 (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12)))))
      (local.set $y0 (f32.min (local.get $y0) (local.get $t)))
      (local.set $y1 (f32.max (local.get $y1) (local.get $t)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (local.set $i (i32.const 0))
    (block $done2 (loop $paint
      (br_if $done2 (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $p (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $c (i32.add (local.get $colors) (i32.mul (local.get $i) (i32.const 12))))
      (if (i32.eqz (local.get $command))
        (then
          ;; t: 0 at the bottom, 1 at the top; two segments of the ramp.
          (local.set $t
            (f32.div (f32.sub (f32.load offset=4 (local.get $p)) (local.get $y0))
              (f32.max (f32.sub (local.get $y1) (local.get $y0)) (f32.const 1e-9))))
          (if (f32.lt (local.get $t) (f32.const 0.5))
            (then
              (local.set $u (f32.mul (local.get $t) (f32.const 2)))
              (local.set $r (call $mix (global.get $r0) (global.get $r1) (local.get $u)))
              (local.set $g (call $mix (global.get $g0) (global.get $g1) (local.get $u)))
              (local.set $b (call $mix (global.get $b0) (global.get $b1) (local.get $u))))
            (else
              (local.set $u (f32.sub (f32.mul (local.get $t) (f32.const 2)) (f32.const 1)))
              (local.set $r (call $mix (global.get $r1) (global.get $r2) (local.get $u)))
              (local.set $g (call $mix (global.get $g1) (global.get $g2) (local.get $u)))
              (local.set $b (call $mix (global.get $b1) (global.get $b2) (local.get $u))))))
        (else
          ;; Normal (-1..1) to color (0..1).
          (local.set $p (i32.add (local.get $normals) (i32.mul (local.get $i) (i32.const 12))))
          (local.set $r (f32.add (f32.mul (f32.load (local.get $p)) (f32.const 0.5)) (f32.const 0.5)))
          (local.set $g (f32.add (f32.mul (f32.load offset=4 (local.get $p)) (f32.const 0.5)) (f32.const 0.5)))
          (local.set $b (f32.add (f32.mul (f32.load offset=8 (local.get $p)) (f32.const 0.5)) (f32.const 0.5)))))
      ;; w: how much of the new color, by Strength and the mask.
      (local.set $w
        (f32.mul (call $param (i32.const 0))
          (f32.sub (f32.const 1) (f32.load (i32.add (local.get $mask) (i32.mul (local.get $i) (i32.const 4)))))))
      (f32.store (local.get $c) (call $mix (f32.load (local.get $c)) (local.get $r) (local.get $w)))
      (f32.store offset=4 (local.get $c) (call $mix (f32.load offset=4 (local.get $c)) (local.get $g) (local.get $w)))
      (f32.store offset=8 (local.get $c) (call $mix (f32.load offset=8 (local.get $c)) (local.get $b) (local.get $w)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $paint)))
    (drop (call $write_colors (local.get $piece) (local.get $colors) (local.get $n)))
    (call $log (i32.mul (local.get $command) (i32.const 32))
      (select (i32.const 20) (i32.const 17) (local.get $command)))))
