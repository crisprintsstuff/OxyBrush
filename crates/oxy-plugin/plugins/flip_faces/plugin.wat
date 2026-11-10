;; Flip Faces: an in-app OxyBrush plugin in WebAssembly text (API 3).
;; Swaps two corners of every triangle of the active piece, so it faces the
;; other way, with write_mesh. After write_mesh the piece starts unmasked,
;; white and in group 0, so the plugin writes its mask, colors and groups
;; back: an example of the counts referring to the new mesh.
;;
;; Memory: positions at 1024, then triangles, mask, colors, groups.
(module
  (import "oxybrush" "log" (func $log (param i32 i32)))
  (import "oxybrush" "active_piece" (func $active (result i32)))
  (import "oxybrush" "vertex_count" (func $vertex_count (param i32) (result i32)))
  (import "oxybrush" "triangle_count" (func $triangle_count (param i32) (result i32)))
  (import "oxybrush" "read_positions" (func $read_positions (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_triangles" (func $read_triangles (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_mask" (func $read_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_colors" (func $read_colors (param i32 i32 i32) (result i32)))
  (import "oxybrush" "read_groups" (func $read_groups (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_mesh" (func $write_mesh (param i32 i32 i32 i32 i32) (result i32)))
  (import "oxybrush" "write_mask" (func $write_mask (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_colors" (func $write_colors (param i32 i32 i32) (result i32)))
  (import "oxybrush" "write_groups" (func $write_groups (param i32 i32 i32) (result i32)))

  (memory (export "memory") 1)
  (data (i32.const 0) "Flipped the faces")
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

  (func (export "run") (param $command i32)
    (local $piece i32) (local $n i32) (local $nt i32)
    (local $tris i32) (local $mask i32) (local $colors i32) (local $groups i32)
    (local $t i32) (local $p i32) (local $b i32)
    (local.set $piece (call $active))
    (local.set $n (call $vertex_count (local.get $piece)))
    (local.set $nt (call $triangle_count (local.get $piece)))
    (if (i32.or (i32.le_s (local.get $n) (i32.const 0)) (i32.le_s (local.get $nt) (i32.const 0))) (then (return)))
    (if (i32.or (i32.gt_u (local.get $n) (i32.const 4000000)) (i32.gt_u (local.get $nt) (i32.const 4000000)))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (local.set $tris (i32.add (i32.const 1024) (i32.mul (local.get $n) (i32.const 12))))
    (local.set $mask (i32.add (local.get $tris) (i32.mul (local.get $nt) (i32.const 12))))
    (local.set $colors (i32.add (local.get $mask) (i32.mul (local.get $n) (i32.const 4))))
    (local.set $groups (i32.add (local.get $colors) (i32.mul (local.get $n) (i32.const 12))))
    (if (i32.eqz (call $ensure (i32.add (local.get $groups) (i32.mul (local.get $nt) (i32.const 4)))))
      (then (call $log (i32.const 32) (i32.const 35)) (return)))
    (drop (call $read_positions (local.get $piece) (i32.const 1024) (local.get $n)))
    (drop (call $read_triangles (local.get $piece) (local.get $tris) (local.get $nt)))
    (drop (call $read_mask (local.get $piece) (local.get $mask) (local.get $n)))
    (drop (call $read_colors (local.get $piece) (local.get $colors) (local.get $n)))
    (drop (call $read_groups (local.get $piece) (local.get $groups) (local.get $nt)))
    ;; Swap the second and third corner of each triangle.
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $t) (local.get $nt)))
      (local.set $p (i32.add (local.get $tris) (i32.mul (local.get $t) (i32.const 12))))
      (local.set $b (i32.load offset=4 (local.get $p)))
      (i32.store offset=4 (local.get $p) (i32.load offset=8 (local.get $p)))
      (i32.store offset=8 (local.get $p) (local.get $b))
      (local.set $t (i32.add (local.get $t) (i32.const 1)))
      (br $each)))
    (drop (call $write_mesh (local.get $piece) (i32.const 1024) (local.get $n) (local.get $tris) (local.get $nt)))
    ;; Same vertices and triangles as before, so the same counts.
    (drop (call $write_mask (local.get $piece) (local.get $mask) (local.get $n)))
    (drop (call $write_colors (local.get $piece) (local.get $colors) (local.get $n)))
    (drop (call $write_groups (local.get $piece) (local.get $groups) (local.get $nt)))
    (call $log (i32.const 0) (i32.const 17))))
