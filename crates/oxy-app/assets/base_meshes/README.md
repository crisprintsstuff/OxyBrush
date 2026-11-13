# Human base meshes

`human_female.oxb` and `human_male.oxb` are the realistic female and male
bodies (`GEO-body_female_realistic`, `GEO-body_male_realistic`) from
Blender's **Human Base Meshes** bundle v1.4.0, released by the Blender
Foundation under **CC0 1.0** (public domain):
<https://www.blender.org/download/demo/asset-bundles/human-base-meshes/>.

Each is 10,582 points and 10,590 faces (10,570 quads, 20 triangles), life
size in scene units (about 1.65 m tall), standing on the floor, Y up, facing
+Z. The bundle's 102 face sets are kept as face groups. Nothing else was
changed. OxyBrush offers them under File > New and Pieces > + Add.

To rebuild them:

```bash
# The bundle's .blend is zstd-compressed; Blender's download mirror serves
# it without the browser check www.blender.org has.
curl -LO https://download.blender.org/demo/asset-bundles/human-base-meshes/human-base-meshes-bundle-v1.4.0.zip
unzip human-base-meshes-bundle-v1.4.0.zip
zstd -d human_base_meshes_bundle_v1.4.0/human_base_meshes_bundle.blend -o bundle.blend
for g in female male; do
  python3 -I scripts/blend_meshes.py bundle.blend export GEO-body_${g}_realistic $g.obj
  cargo run -p oxy-cli --example base_mesh -- $g.obj $g.groups "Human ($g)" \
    crates/oxy-app/assets/base_meshes/human_$g.oxb
done
```
