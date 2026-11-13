"""Read meshes out of a Blender 4.x .blend file, as data.

Used to make OxyBrush's bundled human base meshes from Blender's Human Base
Meshes bundle (CC0 1.0); see crates/oxy-app/assets/base_meshes/README.md.

A .blend file describes its own layout (the "SDNA" block names every struct
and field), so this reads points, faces and face sets without running any
Blender code. Written from the file's own self-description; no Blender
source was consulted. Only 64-bit little-endian files, uncompressed (unpack
a zstd-compressed one first with `zstd -d`).

Usage: python3 -I blend_meshes.py FILE.blend list
       python3 -I blend_meshes.py FILE.blend fields STRUCT...
       python3 -I blend_meshes.py FILE.blend layers OBJECT
       python3 -I blend_meshes.py FILE.blend export OBJECT OUT.obj
         (also writes OUT.groups, one face set per face, when there are any)
"""
import struct
import sys


def field_size(name, base):
    n = 1
    for part in name.split('[')[1:]:
        n *= int(part.split(']')[0])
    if name.startswith('(*'):
        return 8
    if '*' in name.split('[')[0]:
        return 8 * n
    return base * n


def parse_sdna(b):
    assert b[:4] == b'SDNA'
    p = 4

    def align():
        nonlocal p
        p = (p + 3) & ~3

    def strings(tag):
        nonlocal p
        assert b[p:p + 4] == tag, tag
        p += 4
        n, = struct.unpack_from('<i', b, p)
        p += 4
        out = []
        for _ in range(n):
            e = b.index(b'\0', p)
            out.append(b[p:e].decode())
            p = e + 1
        align()
        return out

    names = strings(b'NAME')
    types = strings(b'TYPE')
    assert b[p:p + 4] == b'TLEN'
    p += 4
    tlen = list(struct.unpack_from(f'<{len(types)}h', b, p))
    p += 2 * len(types)
    align()
    assert b[p:p + 4] == b'STRC'
    p += 4
    n, = struct.unpack_from('<i', b, p)
    p += 4
    structs = []
    for _ in range(n):
        t, nf = struct.unpack_from('<hh', b, p)
        p += 4
        fields = []
        for _ in range(nf):
            ft, fn = struct.unpack_from('<hh', b, p)
            p += 4
            fields.append((ft, fn))
        structs.append((t, fields))
    return names, types, tlen, structs


def main():
    data = open(sys.argv[1], 'rb').read()
    assert data[:7] == b'BLENDER' and data[7:9] == b'-v', "64-bit little-endian files only"
    pos = 12
    blocks = []  # (code, data offset, length, old pointer, sdna index, count)
    while pos + 24 <= len(data):
        code = data[pos:pos + 4].rstrip(b'\0')
        length, = struct.unpack_from('<i', data, pos + 4)
        old, = struct.unpack_from('<Q', data, pos + 8)
        sdna, count = struct.unpack_from('<ii', data, pos + 16)
        blocks.append((code, pos + 24, length, old, sdna, count))
        pos += 24 + length
        if code == b'ENDB':
            break
    dna = next(b for b in blocks if b[0] == b'DNA1')
    names, types, tlen, structs = parse_sdna(data[dna[1]:dna[1] + dna[2]])
    by_ptr = {b[3]: b for b in blocks}
    struct_index = {types[t]: i for i, (t, _) in enumerate(structs)}

    if sys.argv[2] == 'fields':
        for tn in sys.argv[3:]:
            print(tn, [names[fn] for _, fn in structs[struct_index[tn]][1]])
        return

    def layout(tname):
        _, fields = structs[struct_index[tname]]
        off, out = 0, {}
        for ft, fn in fields:
            name = names[fn]
            base = name.split('[')[0].strip('*()')
            out[base] = off
            off += field_size(name, tlen[ft])
        return out

    def ptr(o, lay, f):
        return struct.unpack_from('<Q', data, o + lay[f])[0]

    def i32(o, lay, f):
        return struct.unpack_from('<i', data, o + lay[f])[0]

    id_lay = layout('ID')

    def idname(o):
        s = o + id_lay['name']
        return data[s:s + 258].split(b'\0')[0].decode('utf-8', 'replace')

    me_lay, cd_lay, cdl_lay, ob_lay = layout('Mesh'), layout('CustomData'), layout('CustomDataLayer'), layout('Object')
    cdl_size = tlen[structs[struct_index['CustomDataLayer']][0]]

    def layers(mesh_off, field):
        cd = mesh_off + me_lay[field]
        lp, n = ptr(cd, cd_lay, 'layers'), i32(cd, cd_lay, 'totlayer')
        if not lp or lp not in by_ptr:
            return []
        b = by_ptr[lp]
        out = []
        for k in range(n):
            lo = b[1] + k * cdl_size
            s = lo + cdl_lay['name']
            out.append((i32(lo, cdl_lay, 'type'), data[s:s + 68].split(b'\0')[0].decode(), ptr(lo, cdl_lay, 'data')))
        return out

    meshes = {}
    for b in blocks:
        if b[0] == b'ME':
            mo = b[1]
            meshes[b[3]] = (idname(mo)[2:], mo, *(i32(mo, me_lay, f) for f in ('totvert', 'totpoly', 'totloop')))
    objects = []
    for b in blocks:
        if b[0] == b'OB':
            dp = ptr(b[1], ob_lay, 'data')
            if dp in meshes:
                objects.append((idname(b[1])[2:], meshes[dp]))

    if sys.argv[2] == 'layers':
        want = sys.argv[3]
        oname, (mname, mo, nv, nf, nc) = next(o for o in objects if o[0] == want)
        for f in ('vdata', 'edata', 'pdata', 'ldata'):
            print(f, [(t, n) for t, n, _ in layers(mo, f)])
        return
    if sys.argv[2] == 'list':
        for oname, (mname, mo, nv, nf, nc) in sorted(objects):
            print(f"{oname:42s} mesh {mname:42s} points {nv:7d} faces {nf:7d}")
        return
    want = sys.argv[3]
    oname, (mname, mo, nv, nf, nc) = next(o for o in objects if o[0] == want)
    pos_layer = next(l for l in layers(mo, 'vdata') if l[1] == 'position')
    cv_layer = next(l for l in layers(mo, 'ldata') if l[1] == '.corner_vert')
    P = struct.unpack_from(f'<{nv * 3}f', data, by_ptr[pos_layer[2]][1])
    C = struct.unpack_from(f'<{nc}i', data, by_ptr[cv_layer[2]][1])
    F = struct.unpack_from(f'<{nf + 1}i', data, by_ptr[ptr(mo, me_lay, 'poly_offset_indices')][1])
    assert F[0] == 0 and F[-1] == nc, "face offsets don't add up"
    assert all(0 <= c < nv for c in C), "corner refers to a missing point"
    with open(sys.argv[4], 'w') as out:
        out.write(f"# {oname}, from Blender's Human Base Meshes bundle v1.4.0, CC0 1.0 (blender.org)\n")
        for i in range(nv):
            x, y, z = P[3 * i:3 * i + 3]
            # Blender is Z-up, OxyBrush Y-up.
            out.write(f"v {x:.6f} {z:.6f} {-y:.6f}\n")
        for f in range(nf):
            out.write("f " + " ".join(str(C[k] + 1) for k in range(F[f], F[f + 1])) + "\n")
    sets = [l for l in layers(mo, 'pdata') if l[1] == '.sculpt_face_set']
    if sets:
        G = struct.unpack_from(f'<{nf}i', data, by_ptr[sets[0][2]][1])
        with open(sys.argv[4][:-4] + '.groups', 'w') as out:
            out.write("\n".join(str(g) for g in G) + "\n")
        print(f"  face sets: {len(set(G))}")
    print(f"wrote {oname}: {nv} points, {nf} faces")


main()
