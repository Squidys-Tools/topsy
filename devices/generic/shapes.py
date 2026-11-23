# Modelling helpers for the generic headsets: shells lofted through rounded oval rings, parts swept
# along a path, and materials with procedural detail (grain, creases, roughness variation). Every
# part gets a subdivision surface, so it is smooth where it should be and holds its edges where a
# ring or a section says so. Distances are metres, at real size.
import math

import bpy
from mathutils import Matrix, Vector


def oval(t, a, b, n=2.4):
    """A point on a superellipse of half-width `a` and half-height `b`; `n` above 2 squares it."""
    c, s = math.cos(t), math.sin(t)
    return (
        a * math.copysign(abs(c) ** (2 / n), c),
        b * math.copysign(abs(s) ** (2 / n), s),
    )


def mesh_object(name, verts, faces, material=None, levels=2, smooth=True):
    data = bpy.data.meshes.new(name)
    data.from_pydata([tuple(v) for v in verts], [], faces)
    data.validate()
    obj = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(obj)
    if material:
        data.materials.append(material)
    if smooth:
        data.shade_smooth()
    if levels:
        mod = obj.modifiers.new("subdivision", "SUBSURF")
        mod.levels = mod.render_levels = levels
    return obj


def loft(name, rings, axis, segments=48, cap=True, material=None, n=2.4, closed=False):
    """A shell through `rings` of (offset along `axis`, half-width, half-height), in order,
    closed by a cap after the last, and before the first too if `closed`. `axis` is 'X', 'Y' or
    'Z': the oval lies across it."""
    verts, faces = [], []
    for offset, a, b in rings:
        for k in range(segments):
            u, v = oval(2 * math.pi * k / segments, a, b, n)
            verts.append(_place(axis, offset, u, v))
    for r in range(len(rings) - 1):
        for k in range(segments):
            i, j = r * segments + k, r * segments + (k + 1) % segments
            faces.append((i, j, j + segments, i + segments))
    if cap:
        last = len(rings) - 1
        verts.append(_place(axis, rings[-1][0], 0, 0))
        centre = len(verts) - 1
        for k in range(segments):
            faces.append((last * segments + k, last * segments + (k + 1) % segments, centre))
    if closed:
        verts.append(_place(axis, rings[0][0], 0, 0))
        start = len(verts) - 1
        for k in range(segments):
            faces.append(((k + 1) % segments, k, start))
    return mesh_object(name, verts, faces, material)


def _place(axis, offset, u, v):
    if axis == "X":
        return Vector((offset, u, v))
    if axis == "Y":
        return Vector((u, offset, v))
    return Vector((u, v, offset))


def sweep(name, path, section, up=Vector((0, 0, 1)), closed=False, material=None, levels=2):
    """`section`, a closed loop of (across, along `up`) points, carried along `path`. A closed
    path joins its ends."""
    verts, faces = [], []
    count = len(section)
    for i, point in enumerate(path):
        before = path[i - 1] if (closed or i > 0) else point
        after = path[(i + 1) % len(path)] if (closed or i < len(path) - 1) else point
        tangent = (Vector(after) - Vector(before)).normalized()
        across = tangent.cross(up).normalized()
        if across.length < 0.5:
            across = tangent.cross(Vector((1, 0, 0))).normalized()
        normal = across.cross(tangent).normalized()
        for x, y in section:
            verts.append(Vector(point) + across * x + normal * y)
    rows = len(path) if closed else len(path) - 1
    for i in range(rows):
        for k in range(count):
            a, b = i * count + k, i * count + (k + 1) % count
            c, d = ((i + 1) % len(path)) * count + (k + 1) % count, ((i + 1) % len(path)) * count + k
            faces.append((a, b, c, d))
    if not closed:
        faces.append(tuple(range(count))[::-1])
        last = (len(path) - 1) * count
        faces.append(tuple(range(last, last + count)))
    return mesh_object(name, verts, faces, material, levels)


def rounded_rect(width, height, radius, steps=4):
    """A closed loop: a rectangle with rounded corners, centred on the origin."""
    w, h, r = width / 2 - radius, height / 2 - radius, radius
    points = []
    for cx, cy, start in ((w, h, 0), (-w, h, 90), (-w, -h, 180), (w, -h, 270)):
        for s in range(steps + 1):
            angle = math.radians(start + 90 * s / steps)
            points.append((cx + r * math.cos(angle), cy + r * math.sin(angle)))
    return points


def cushion_section(width, thickness, taper=0.22, n=2.6, steps=24):
    """A cushion's cross-section: a rounded block, `width` across and `thickness` deep, narrowing
    by `taper` toward its face (+y), as foam under leather does."""
    points = []
    for k in range(steps):
        u, v = oval(2 * math.pi * k / steps, width / 2, thickness / 2, n)
        squeeze = 1 - taper * max(0.0, v / (thickness / 2))
        points.append((u * squeeze, v))
    return points


def mat(
    name,
    colour,
    roughness,
    metallic=0.0,
    coat=0.0,
    bump=None,
    sheen=0.0,
    anisotropic=0.0,
    wrinkle=None,
):
    """A material; `bump` is (scale, strength) of a fine noise, for grain or texture, and
    `wrinkle` a coarser, stretched one for the creases leather takes."""
    material = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    tree = material.node_tree
    nodes, links = tree.nodes, tree.links
    for node in list(nodes):
        if node.type not in ("BSDF_PRINCIPLED", "OUTPUT_MATERIAL"):
            nodes.remove(node)
    bsdf = nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Coat Weight"].default_value = coat
    bsdf.inputs["Sheen Weight"].default_value = sheen
    bsdf.inputs["Anisotropic"].default_value = anisotropic
    # A little variation in shine, as real surfaces have.
    vary = nodes.new("ShaderNodeTexNoise")
    vary.inputs["Scale"].default_value = 60.0
    ramp = nodes.new("ShaderNodeMapRange")
    ramp.inputs["To Min"].default_value = roughness * 0.85
    ramp.inputs["To Max"].default_value = min(1.0, roughness * 1.15)
    links.new(vary.outputs["Fac"], ramp.inputs["Value"])
    links.new(ramp.outputs["Result"], bsdf.inputs["Roughness"])
    normal = None
    for layer, distortion, distance in ((wrinkle, 6.0, 0.0012), (bump, 0.0, 0.0004)):
        if not layer:
            continue
        scale, strength = layer
        noise = nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = scale
        noise.inputs["Detail"].default_value = 8.0
        noise.inputs["Distortion"].default_value = distortion
        bumper = nodes.new("ShaderNodeBump")
        bumper.inputs["Strength"].default_value = strength
        bumper.inputs["Distance"].default_value = distance
        links.new(noise.outputs["Fac"], bumper.inputs["Height"])
        if normal:
            links.new(normal, bumper.inputs["Normal"])
        normal = bumper.outputs["Normal"]
    if normal:
        links.new(normal, bsdf.inputs["Normal"])
    return material


def clear():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj)
    for data in (bpy.data.meshes, bpy.data.materials, bpy.data.cameras, bpy.data.lights):
        for block in list(data):
            data.remove(block)


# Blender won't polygonise a metaball finer than this, which is coarse at an earbud's size, so
# bodies are built this many times larger and scaled back down.
METABALL_SCALE = 100.0


def metaball(name, parts, resolution=0.0003, material=None):
    """One smooth body blended from `parts`: (type, centre, radius, size or None, rotation
    quaternion or None). Its surface sits inside the radii, so place details with `stick`."""
    k = METABALL_SCALE
    data = bpy.data.metaballs.new(name)
    data.resolution = data.render_resolution = resolution * k
    obj = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(obj)
    for kind, centre, radius, size, rotation in parts:
        element = data.elements.new()
        element.type = kind
        element.co = Vector(centre) * k
        element.radius = radius * k
        if size is not None:
            # An ellipsoid's sizes stretch its radius; the others' are lengths.
            scale = 1.0 if kind == "ELLIPSOID" else k
            element.size_x, element.size_y, element.size_z = (s * scale for s in size)
        if rotation is not None:
            element.rotation = rotation
    for other in bpy.context.selected_objects:
        other.select_set(False)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.convert(target="MESH")
    mesh = bpy.context.view_layer.objects.active
    mesh.name = name
    mesh.data.transform(Matrix.Scale(1 / k, 4))
    mesh.data.shade_smooth()
    if material:
        mesh.data.materials.append(material)
    return mesh


def stick(target, part, origin, direction, inset=0.0):
    """Seat `part` where a ray from `origin` along `direction` meets `target`'s surface, its
    local +Z along the surface normal, sunk `inset` into it."""
    direction = Vector(direction).normalized()
    hit, point, normal, _ = target.ray_cast(Vector(origin), direction)
    if not hit:
        raise RuntimeError(f"{part.name} misses {target.name}")
    part.location = target.matrix_world @ (point - normal * inset)
    part.rotation_euler = normal.to_track_quat("Z", "Y").to_euler()
    return part


def ellipse_disc(name, a, b, depth, material, segments=48, n=2.0):
    """A thin rounded oval plate, `a` by `b` across, lying in XY, for vents and grilles; `n`
    above 2 squares its corners."""
    obj = loft(
        name,
        [(-depth / 2, a, b), (depth / 2, a, b), (depth / 2 + depth * 0.4, a * 0.85, b * 0.85)],
        "Z",
        segments=segments,
        material=material,
        n=n,
        closed=True,
    )
    return obj


def open_edges():
    """Each part's count of open edges after its modifiers: a visible open edge is a hole."""
    deps = bpy.context.evaluated_depsgraph_get()
    report = {}
    for obj in bpy.context.scene.objects:
        if obj.type != "MESH":
            continue
        mesh = obj.evaluated_get(deps).to_mesh()
        counts = {}
        for poly in mesh.polygons:
            for key in poly.edge_keys:
                counts[key] = counts.get(key, 0) + 1
        boundary = sum(1 for c in counts.values() if c == 1)
        obj.evaluated_get(deps).to_mesh_clear()
        if boundary:
            report[obj.name] = boundary
    return report
