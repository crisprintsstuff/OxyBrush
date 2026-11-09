;; Measure: an in-app OxyBrush plugin in WebAssembly text (RFC 0011).
;; Writes "Size W × H × D, volume V" for the active piece to the status
;; line (three decimals, in scene units) and draws its bounding box while
;; the panel is open. The volume is the signed volume of the triangles, so
;; it is meaningful for closed (watertight) pieces.
;;
;; Memory: text at 0..1023 (strings at 0, the message built at 512),
;; positions at 1024, then triangles.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "triangle_count" (func $triangle_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_triangles" (func $read_triangles (param i32 i32 i32) (result i32)))
  (import "oxybrush" "draw_line" (func $draw_line (param f32 f32 f32 f32 f32 f32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Size ")
  (data (i32.const 8) " \c3\97 ")
  (data (i32.const 16) ", volume ")
  (data (i32.const 32) "The piece is too large for a plugin")
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

  ;; Copy `len` bytes from `src` to `at`; returns the end.
  (func $put_str (param $src i32) (param $len i32) (param $at i32) (result i32)
    (memory.copy (local.get $at) (local.get $src) (local.get $len))
    (i32.add (local.get $at) (local.get $len)))

  ;; Decimal digits of `n` at `at`; returns the end.
  (func $put_u (param $n i32) (param $at i32) (result i32)
    (local $len i32) (local $m i32) (local $i i32)
    ;; Count digits.
    (local.set $m (local.get $n))
    (local.set $len (i32.const 1))
    (block $done (loop $count
      (br_if $done (i32.lt_u (local.get $m) (i32.const 10)))
      (local.set $m (i32.div_u (local.get $m) (i32.const 10)))
      (local.set $len (i32.add (local.get $len) (i32.const 1)))
      (br $count)))
    ;; Write them from the end.
    (local.set $i (local.get $len))
    (local.set $m (local.get $n))
    (block $done2 (loop $write
      (br_if $done2 (i32.eqz (local.get $i)))
      (local.set $i (i32.sub (local.get $i) (i32.const 1)))
      (i32.store8 (i32.add (local.get $at) (local.get $i))
        (i32.add (i32.const 48) (i32.rem_u (local.get $m) (i32.const 10))))
      (local.set $m (i32.div_u (local.get $m) (i32.const 10)))
      (br $write)))
    (i32.add (local.get $at) (local.get $len)))

  ;; `v` with three decimals (a leading "-" when negative); returns the end.
  (func $put_f (param $v f32) (param $at i32) (result i32)
    (local $m i32) (local $f i32)
    (if (f32.lt (local.get $v) (f32.const 0))
      (then
        (i32.store8 (local.get $at) (i32.const 45))
        (local.set $at (i32.add (local.get $at) (i32.const 1)))
        (local.set $v (f32.neg (local.get $v)))))
    (local.set $m (i32.trunc_sat_f32_u (f32.nearest (f32.mul (local.get $v) (f32.const 1000)))))
    (local.set $at (call $put_u (i32.div_u (local.get $m) (i32.const 1000)) (local.get $at)))
    (local.set $f (i32.rem_u (local.get $m) (i32.const 1000)))
    (i32.store8 (local.get $at) (i32.const 46))
    (i32.store8 offset=1 (local.get $at) (i32.add (i32.const 48) (i32.div_u (local.get $f) (i32.const 100))))
    (i32.store8 offset=2 (local.get $at)
      (i32.add (i32.const 48) (i32.rem_u (i32.div_u (local.get $f) (i32.const 10)) (i32.const 10))))
    (i32.store8 offset=3 (local.get $at) (i32.add (i32.const 48) (i32.rem_u (local.get $f) (i32.const 10))))
    (i32.add (local.get $at) (i32.const 4)))

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $t i32) (local $tris i32) (local $i i32) (local $p i32)
    (local $a i32) (local $b i32) (local $c i32) (local $at i32) (local $v f32) (local $vol f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (local.set $t (call $triangle_count (local.get $piece)))
    (if (i32.le_s (local.get $n) (i32.const 0)) (then (return)))
    (if (i32.or (i32.gt_u (local.get $n) (i32.const 20000000)) (i32.gt_u (local.get $t) (i32.const 40000000)))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (local.set $tris (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $tris) (i32.mul (local.get $t) (i32.const 12)))))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_triangles (local.get $piece) (local.get $tris) (local.get $t)))
    ;; Bounds.
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
    (global.set $have (i32.const 1))
    ;; Volume: sum of a · (b × c) / 6 over the triangles.
    (local.set $i (i32.const 0))
    (block $done2 (loop $tri
      (br_if $done2 (i32.ge_u (local.get $i) (local.get $t)))
      (local.set $p (i32.add (local.get $tris) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $a (i32.add (i32.const 1024) (i32.mul (i32.load (local.get $p)) (i32.const 12))))
      (local.set $b (i32.add (i32.const 1024) (i32.mul (i32.load offset=4 (local.get $p)) (i32.const 12))))
      (local.set $c (i32.add (i32.const 1024) (i32.mul (i32.load offset=8 (local.get $p)) (i32.const 12))))
      (local.set $vol
        (f32.add (local.get $vol)
          (f32.add
            (f32.add
              (f32.mul (f32.load (local.get $a))
                (f32.sub (f32.mul (f32.load offset=4 (local.get $b)) (f32.load offset=8 (local.get $c)))
                         (f32.mul (f32.load offset=8 (local.get $b)) (f32.load offset=4 (local.get $c)))))
              (f32.mul (f32.load offset=4 (local.get $a))
                (f32.sub (f32.mul (f32.load offset=8 (local.get $b)) (f32.load (local.get $c)))
                         (f32.mul (f32.load (local.get $b)) (f32.load offset=8 (local.get $c))))))
            (f32.mul (f32.load offset=8 (local.get $a))
              (f32.sub (f32.mul (f32.load (local.get $b)) (f32.load offset=4 (local.get $c)))
                       (f32.mul (f32.load offset=4 (local.get $b)) (f32.load (local.get $c))))))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $tri)))
    (local.set $vol (f32.div (local.get $vol) (f32.const 6)))
    ;; "Size W × H × D, volume V"
    (local.set $at (call $put_str (i32.const 0) (i32.const 5) (i32.const 512)))
    (local.set $at (call $put_f (f32.sub (global.get $x1) (global.get $x0)) (local.get $at)))
    (local.set $at (call $put_str (i32.const 8) (i32.const 4) (local.get $at)))
    (local.set $at (call $put_f (f32.sub (global.get $y1) (global.get $y0)) (local.get $at)))
    (local.set $at (call $put_str (i32.const 8) (i32.const 4) (local.get $at)))
    (local.set $at (call $put_f (f32.sub (global.get $z1) (global.get $z0)) (local.get $at)))
    (local.set $at (call $put_str (i32.const 16) (i32.const 9) (local.get $at)))
    (local.set $at (call $put_f (local.get $vol) (local.get $at)))
    (call $log (i32.const 512) (i32.sub (local.get $at) (i32.const 512))))

  (func $edge (param $ax f32) (param $ay f32) (param $az f32) (param $bx f32) (param $by f32) (param $bz f32)
    (drop (call $draw_line (local.get $ax) (local.get $ay) (local.get $az)
                           (local.get $bx) (local.get $by) (local.get $bz) (i32.const 0xffd040ff))))

  ;; The twelve edges of the last measured box.
  (func (export "overlay")
    (local $a f32) (local $b f32) (local $c f32) (local $d f32) (local $e f32) (local $f f32)
    (if (i32.eqz (global.get $have)) (then (return)))
    (local.set $a (global.get $x0)) (local.set $b (global.get $x1))
    (local.set $c (global.get $y0)) (local.set $d (global.get $y1))
    (local.set $e (global.get $z0)) (local.set $f (global.get $z1))
    ;; Along X.
    (call $edge (local.get $a) (local.get $c) (local.get $e) (local.get $b) (local.get $c) (local.get $e))
    (call $edge (local.get $a) (local.get $d) (local.get $e) (local.get $b) (local.get $d) (local.get $e))
    (call $edge (local.get $a) (local.get $c) (local.get $f) (local.get $b) (local.get $c) (local.get $f))
    (call $edge (local.get $a) (local.get $d) (local.get $f) (local.get $b) (local.get $d) (local.get $f))
    ;; Along Y.
    (call $edge (local.get $a) (local.get $c) (local.get $e) (local.get $a) (local.get $d) (local.get $e))
    (call $edge (local.get $b) (local.get $c) (local.get $e) (local.get $b) (local.get $d) (local.get $e))
    (call $edge (local.get $a) (local.get $c) (local.get $f) (local.get $a) (local.get $d) (local.get $f))
    (call $edge (local.get $b) (local.get $c) (local.get $f) (local.get $b) (local.get $d) (local.get $f))
    ;; Along Z.
    (call $edge (local.get $a) (local.get $c) (local.get $e) (local.get $a) (local.get $c) (local.get $f))
    (call $edge (local.get $b) (local.get $c) (local.get $e) (local.get $b) (local.get $c) (local.get $f))
    (call $edge (local.get $a) (local.get $d) (local.get $e) (local.get $a) (local.get $d) (local.get $f))
    (call $edge (local.get $b) (local.get $d) (local.get $e) (local.get $b) (local.get $d) (local.get $f))))
