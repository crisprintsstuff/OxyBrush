;; Squash: an example OxyBrush plugin in WebAssembly text (RFC 0011).
;; Moves every vertex of the active piece toward the piece's middle height
;; by `amount`, less where it is masked. While its panel is open it draws a
;; cross at that height.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "param" (func $param (param i32) (result f32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_mask" (func $read_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_positions" (func $write_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "draw_line" (func $draw_line (param f32 f32 f32 f32 f32 f32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Squashed toward the middle")
  (global $middle (mut f32) (f32.const 0))

  ;; Positions start at byte 1024; the mask follows them.
  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $i i32) (local $at i32)
    (local $mask_at i32) (local $need i32) (local $sum f32) (local $mid f32)
    (local $amount f32) (local $y f32) (local $keep f32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (if (i32.le_s (local.get $n) (i32.const 0)) (then (return)))
    (local.set $mask_at (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    ;; Grow memory to fit positions and mask.
    (local.set $need
      (i32.div_u
        (i32.add (i32.add (local.get $mask_at) (i32.mul (local.get $n) (i32.const 4))) (i32.const 65535))
        (i32.const 65536)))
    (if (i32.gt_u (local.get $need) (memory.size))
      (then (drop (memory.grow (i32.sub (local.get $need) (memory.size))))))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_mask (local.get $piece) (local.get $mask_at) (local.get $n)))
    ;; Middle height: the mean of y.
    (local.set $i (i32.const 0))
    (block $done (loop $sum_y
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $at (i32.add (i32.const 1028) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $sum (f32.add (local.get $sum) (f32.load (local.get $at))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $sum_y)))
    (local.set $mid (f32.div (local.get $sum) (f32.convert_i32_u (local.get $n))))
    (global.set $middle (local.get $mid))
    (local.set $amount (call $param (i32.const 0)))
    ;; y' = mid + (y - mid) * (1 - amount * (1 - mask))
    (local.set $i (i32.const 0))
    (block $done2 (loop $squash
      (br_if $done2 (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $at (i32.add (i32.const 1028) (i32.mul (local.get $i) (i32.const 12))))
      (local.set $y (f32.load (local.get $at)))
      (local.set $keep
        (f32.sub (f32.const 1)
          (f32.mul (local.get $amount)
            (f32.sub (f32.const 1)
              (f32.load (i32.add (local.get $mask_at) (i32.mul (local.get $i) (i32.const 4))))))))
      (f32.store (local.get $at)
        (f32.add (local.get $mid) (f32.mul (f32.sub (local.get $y) (local.get $mid)) (local.get $keep))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $squash)))
    (drop (call $write_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (call $log (i32.const 0) (i32.const 26)))

  ;; A cross at the middle height of the last squash.
  (func (export "overlay")
    (drop (call $draw_line (f32.const -1.5) (global.get $middle) (f32.const 0)
                           (f32.const 1.5) (global.get $middle) (f32.const 0) (i32.const 0xff8040ff)))
    (drop (call $draw_line (f32.const 0) (global.get $middle) (f32.const -1.5)
                           (f32.const 0) (global.get $middle) (f32.const 1.5) (i32.const 0xff8040ff)))))
