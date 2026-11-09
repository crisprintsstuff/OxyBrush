;; Displace: an in-app OxyBrush plugin in WebAssembly text (RFC 0011).
;; Moves each vertex of the active piece along its normal: out (Inflate),
;; in (Deflate) or by a random amount each way (Roughen). The distance is a
;; percentage of the piece's largest side, less where it is masked.
;;
;; Memory: positions at 1024, then normals, then the mask.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_normals" (func $read_normals (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_mask" (func $read_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_positions" (func $write_positions (param i32 i32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Inflated")
  (data (i32.const 16) "Deflated")
  (data (i32.const 32) "Roughened")
  (data (i32.const 48) "The piece is too large for a plugin")
  (global $rng (mut i32) (i32.const 1))

  ;; Grow memory so bytes [0, end) exist. Returns 0 if it could not.
  (func $ensure (param $end i32) (result i32)
    (local $need i32)
    (if (i32.lt_s (local.get $end) (i32.const 0)) (then (return (i32.const 0))))
    (local.set $need (i32.div_u (i32.add (local.get $end) (i32.const 65535)) (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then
        (if (i32.eq (memory.grow (i32.sub (local.get $need) (memory.size))) (i32.const -1))
          (then (return (i32.const 0))))))
    (i32.const 1))

  ;; Largest side of the bounding box of `n` positions at `at`.
  (func $extent (param $at i32) (param $n i32) (result f32)
    (local $i i32) (local $p i32) (local $k i32) (local $v f32)
    (local $x0 f32) (local $x1 f32) (local $y0 f32) (local $y1 f32) (local $z0 f32) (local $z1 f32)
    (local.set $x0 (f32.const inf)) (local.set $y0 (f32.const inf)) (local.set $z0 (f32.const inf))
    (local.set $x1 (f32.const -inf)) (local.set $y1 (f32.const -inf)) (local.set $z1 (f32.const -inf))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $p (i32.add (local.get $at) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $v (f32.load (local.get $p)))
      (local.set $x0 (f32.min (local.get $x0) (local.get $v)))
      (local.set $x1 (f32.max (local.get $x1) (local.get $v)))
      (local.set $v (f32.load offset=4 (local.get $p)))
      (local.set $y0 (f32.min (local.get $y0) (local.get $v)))
      (local.set $y1 (f32.max (local.get $y1) (local.get $v)))
      (local.set $v (f32.load offset=8 (local.get $p)))
      (local.set $z0 (f32.min (local.get $z0) (local.get $v)))
      (local.set $z1 (f32.max (local.get $z1) (local.get $v)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (f32.max (f32.sub (local.get $x1) (local.get $x0))
      (f32.max (f32.sub (local.get $y1) (local.get $y0)) (f32.sub (local.get $z1) (local.get $z0)))))

  ;; A random number in -1..1 (xorshift32).
  (func $random (result f32)
    (local $s i32)
    (local.set $s (global.get $rng))
    (local.set $s (i32.xor (local.get $s) (i32.shl (local.get $s) (i32.const 13))))
    (local.set $s (i32.xor (local.get $s) (i32.shr_u (local.get $s) (i32.const 17))))
    (local.set $s (i32.xor (local.get $s) (i32.shl (local.get $s) (i32.const 5))))
    (global.set $rng (local.get $s))
    (f32.sub
      (f32.mul (f32.convert_i32_u (i32.shr_u (local.get $s) (i32.const 8))) (f32.const 1.1920929e-7))
      (f32.const 1)))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $i i32) (local $p i32) (local $q i32)
    (local $normals i32) (local $mask i32) (local $d f32) (local $amount f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (if (i32.le_s (local.get $n) (i32.const 0)) (then (return)))
    (if (i32.gt_u (local.get $n) (i32.const 20000000))
      (then (call $log (i32.const 48) (i32.const 35)) (return)))
    (local.set $normals (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $mask (i32.add (local.get $normals) (i32.mul (local.get $n) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $mask) (i32.mul (local.get $n) (i32.const 4)))))
      (then (call $log (i32.const 48) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_normals (local.get $piece) (local.get $normals) (local.get $n)))
    (drop (call $read_mask (local.get $piece) (local.get $mask) (local.get $n)))
    (local.set $amount
      (f32.mul (call $extent (i32.const 1024) (local.get $n))
        (f32.mul (call $param (i32.const 0)) (f32.const 0.01))))
    (if (i32.eq (local.get $command) (i32.const 1))
      (then (local.set $amount (f32.neg (local.get $amount)))))
    ;; Seed: never 0 (xorshift would stay at 0).
    (global.set $rng
      (i32.or
        (i32.mul (i32.trunc_sat_f32_u (call $param (i32.const 1))) (i32.const 0x9e3779b1))
        (i32.const 1)))
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $p (i32.add (i32.const 1024) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $q (i32.add (local.get $normals) (i32.mul (local.get $i) (i32.const 12))))
      ;; d = amount * (1 - mask), times a random -1..1 for Roughen.
      (local.set $d
        (f32.mul (local.get $amount)
          (f32.sub (f32.const 1)
            (f32.load (i32.add (local.get $mask) (i32.mul (local.get $i) (i32.const 4)))))))
      (if (i32.eq (local.get $command) (i32.const 2))
        (then (local.set $d (f32.mul (local.get $d) (call $random)))))
      (f32.store (local.get $p)
        (f32.add (f32.load (local.get $p)) (f32.mul (f32.load (local.get $q)) (local.get $d))))
      (f32.store offset=4 (local.get $p)
        (f32.add (f32.load offset=4 (local.get $p)) (f32.mul (f32.load offset=4 (local.get $q)) (local.get $d))))
      (f32.store offset=8 (local.get $p)
        (f32.add (f32.load offset=8 (local.get $p)) (f32.mul (f32.load offset=8 (local.get $q)) (local.get $d))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    (drop (call $write_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (call $log (i32.mul (local.get $command) (i32.const 16))
      (select (i32.const 9) (i32.const 8) (i32.eq (local.get $command) (i32.const 2))))))
