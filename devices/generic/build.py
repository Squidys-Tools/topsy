# Builds the four generic headsets topsy draws a device as when the catalog has no model of its
# own: stem-buds, buds, over-ear and speaker, each as devices/generic/<shape>/model.glb.
# Shapes are metaballs and primitives with plain PBR materials, so they stay editable here.
# usage: blender --background --factory-startup --python devices/generic/build.py
#        (or `cargo devices generic`, which also renders them)
import math
import os

import bpy
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def material(name, colour, roughness, metallic=0.0, coat=0.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = metallic
    if "Coat Weight" in bsdf.inputs:
        bsdf.inputs["Coat Weight"].default_value = coat
    return mat


def finish(obj, mat, smooth=True, bevel=0.0):
    obj.data.materials.clear()
    obj.data.materials.append(mat)
    if bevel:
        mod = obj.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 4
        mod.limit_method = "ANGLE"
    if smooth:
        for poly in obj.data.polygons:
            poly.use_smooth = True
    return obj


def metaball(name, parts, resolution=0.04):
    """`parts` are (type, centre, radius, size, rotation) elements, blended into one mesh."""
    data = bpy.data.metaballs.new(name)
    data.resolution = resolution
    data.render_resolution = resolution
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    for kind, centre, radius, size, rotation in parts:
        el = data.elements.new()
        el.type = kind
        el.co = Vector(centre)
        el.radius = radius
        if size is not None:
            el.size_x, el.size_y, el.size_z = size
        if rotation is not None:
            el.rotation = rotation
    bpy.context.view_layer.objects.active = obj
    for other in bpy.context.selected_objects:
        other.select_set(False)
    obj.select_set(True)
    bpy.ops.object.convert(target="MESH")
    return bpy.context.view_layer.objects.active


def stick(target, part, towards, inset=0.0):
    """Seat `part` on `target`'s surface where a ray from its centre `towards` leaves it, facing
    out: a metaball's surface sits inside its radius, so parts are placed where it really is."""
    direction = Vector(towards).normalized()
    hit, point, normal, _ = target.ray_cast(direction * 10.0, -direction)
    if not hit:
        raise RuntimeError(f"{part.name} misses {target.name}")
    part.location = target.matrix_world @ (point - normal * inset)
    part.rotation_euler = normal.to_track_quat("Z", "Y").to_euler()
    return part


def disc(radius, depth, mat, vertices=48):
    obj = primitive(bpy.ops.mesh.primitive_cylinder_add, radius=radius, depth=depth, vertices=vertices)
    return finish(obj, mat, bevel=depth / 3)


def quat(axis, degrees):
    from mathutils import Quaternion

    return Quaternion(Vector(axis), math.radians(degrees))


def primitive(op, **kwargs):
    op(**kwargs)
    return bpy.context.active_object


def group(name, objects, location=(0, 0, 0), rotation=(0, 0, 0)):
    root = bpy.data.objects.new(name, None)
    bpy.context.collection.objects.link(root)
    for obj in objects:
        obj.parent = root
    root.location = location
    root.rotation_euler = [math.radians(a) for a in rotation]
    return root


def export(shape):
    folder = os.path.join(HERE, shape)
    os.makedirs(folder, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=os.path.join(folder, "model.glb"),
        export_format="GLB",
        export_apply=True,
        export_yup=True,
    )


def stem_bud(side, white, grille, metal):
    """One AirPods-like bud, its stem hanging below; `side` is -1 for the left, 1 for the right."""
    body = metaball(
        f"bud{side}",
        [
            ("ELLIPSOID", (0, 0, 0), 0.62, (1.0, 0.92, 1.0), None),
            ("BALL", (0.02, -0.34, 0.06), 0.34, None, None),
            ("CAPSULE", (0.08, 0.02, -1.0), 0.24, (0.85, 1, 1), quat((0, 1, 0), 84)),
        ],
    )
    finish(body, white)
    mesh = stick(body, disc(0.11, 0.03, grille), (0, -1, 0.25), inset=0.01)
    vent = stick(body, disc(0.07, 0.03, grille), (0.6, 0.3, 0.6), inset=0.01)
    tip = stick(body, disc(0.09, 0.02, metal), (0.06, 0, -1), inset=0.012)
    root = group(f"stem-bud{side}", [body, mesh, vent, tip], location=(0.72 * side, 0, 0.3))
    root.rotation_euler[2] = math.radians(-25 * side)
    # The right bud is the left one mirrored.
    root.scale[0] = side
    return root


def build_stem_buds():
    reset()
    white = material("white plastic", (0.95, 0.95, 0.94), 0.2, coat=0.8)
    grille = material("grille", (0.03, 0.03, 0.03), 0.6)
    metal = material("metal", (0.75, 0.75, 0.76), 0.28, metallic=1.0)
    stem_bud(-1, white, grille, metal)
    stem_bud(1, white, grille, metal)
    export("stem-buds")


def bud(side, shell, tip_mat, accent):
    """One bean-shaped bud, its ear tip pointing back and in."""
    body = metaball(
        f"body{side}",
        [
            ("ELLIPSOID", (0, 0, 0), 0.9, (1.0, 0.72, 0.85), None),
            ("CAPSULE", (0, 0.42, 0), 0.34, (0.32, 1, 1), quat((0, 0, 1), 90)),
        ],
    )
    finish(body, shell)
    tip = primitive(
        bpy.ops.mesh.primitive_uv_sphere_add, radius=0.3, segments=32, ring_count=16
    )
    tip.scale = (1, 0.8, 1)
    finish(tip, tip_mat)
    stick(body, tip, (0, 1, 0), inset=0.12)
    pad = stick(body, disc(0.3, 0.04, accent, vertices=64), (0, -1, 0.1), inset=0.03)
    root = group(f"bud{side}", [body, tip, pad], location=(0.8 * side, 0, 0))
    root.rotation_euler[2] = math.radians(-30 * side)
    root.scale[0] = side
    return root


def build_buds():
    reset()
    shell = material("graphite", (0.06, 0.06, 0.065), 0.32, coat=0.4)
    tip = material("silicone", (0.16, 0.16, 0.17), 0.75)
    accent = material("glass", (0.02, 0.02, 0.02), 0.08, coat=1.0)
    bud(-1, shell, tip, accent)
    bud(1, shell, tip, accent)
    export("buds")


def build_over_ear():
    reset()
    body = material("space grey", (0.11, 0.11, 0.12), 0.38)
    cushion = material("protein leather", (0.035, 0.035, 0.04), 0.55)
    metal = material("aluminium", (0.78, 0.78, 0.8), 0.25, metallic=1.0)
    parts = []
    band = primitive(
        bpy.ops.mesh.primitive_torus_add,
        major_radius=1.25,
        minor_radius=0.075,
        major_segments=96,
        minor_segments=24,
        rotation=(math.radians(90), 0, 0),
    )
    finish(band, metal)
    pad = primitive(
        bpy.ops.mesh.primitive_torus_add,
        major_radius=1.13,
        minor_radius=0.11,
        major_segments=96,
        minor_segments=24,
        rotation=(math.radians(90), 0, 0),
    )
    pad.scale = (1, 1, 1)
    finish(pad, cushion)
    for ring in (band, pad):
        bpy.context.view_layer.objects.active = ring
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="DESELECT")
        bpy.ops.object.mode_set(mode="OBJECT")
        for vert in ring.data.vertices:
            world = ring.matrix_world @ vert.co
            vert.select = world.z < 0.15
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.delete(type="VERT")
        bpy.ops.object.mode_set(mode="OBJECT")
    parts += [band, pad]
    for side in (-1, 1):
        cup = primitive(
            bpy.ops.mesh.primitive_cylinder_add,
            radius=0.58,
            depth=0.34,
            vertices=96,
            location=(1.2 * side, 0, -0.35),
            rotation=(0, math.radians(90), 0),
        )
        cup.scale = (1.15, 1, 1)
        finish(cup, body, bevel=0.07)
        ring = primitive(
            bpy.ops.mesh.primitive_torus_add,
            major_radius=0.46,
            minor_radius=0.15,
            major_segments=96,
            minor_segments=24,
            location=(1.2 * side - 0.2 * side, 0, -0.35),
            rotation=(0, math.radians(90), 0),
        )
        ring.scale = (1.15, 1, 1)
        finish(ring, cushion)
        yoke = primitive(
            bpy.ops.mesh.primitive_cylinder_add,
            radius=0.05,
            depth=0.5,
            location=(1.22 * side, 0, 0.18),
        )
        finish(yoke, metal)
        cap = primitive(
            bpy.ops.mesh.primitive_cylinder_add,
            radius=0.2,
            depth=0.02,
            vertices=64,
            location=(1.2 * side + 0.18 * side, 0, -0.35),
            rotation=(0, math.radians(90), 0),
        )
        finish(cap, metal, bevel=0.005)
        parts += [cup, ring, yoke, cap]
    group("over-ear", parts)
    export("over-ear")


def build_speaker():
    reset()
    fabric = material("fabric", (0.045, 0.05, 0.06), 0.95)
    rubber = material("rubber", (0.025, 0.025, 0.03), 0.6)
    metal = material("badge", (0.8, 0.8, 0.82), 0.22, metallic=1.0)
    body = primitive(
        bpy.ops.mesh.primitive_cylinder_add,
        radius=0.62,
        depth=2.3,
        vertices=128,
        rotation=(0, math.radians(90), 0),
    )
    finish(body, fabric, bevel=0.03)
    parts = [body]
    for side in (-1, 1):
        cap = primitive(
            bpy.ops.mesh.primitive_cylinder_add,
            radius=0.64,
            depth=0.16,
            vertices=128,
            location=(1.2 * side, 0, 0),
            rotation=(0, math.radians(90), 0),
        )
        finish(cap, rubber, bevel=0.05)
        radiator = primitive(
            bpy.ops.mesh.primitive_cylinder_add,
            radius=0.38,
            depth=0.02,
            vertices=96,
            location=(1.285 * side, 0, 0),
            rotation=(0, math.radians(90), 0),
        )
        finish(radiator, metal, bevel=0.004)
        parts += [cap, radiator]
    badge = primitive(
        bpy.ops.mesh.primitive_cube_add,
        size=1,
        location=(0, -0.61, 0.05),
    )
    badge.scale = (0.32, 0.03, 0.09)
    finish(badge, metal, bevel=0.02)
    strip = primitive(
        bpy.ops.mesh.primitive_cube_add,
        size=1,
        location=(0, 0, -0.6),
    )
    strip.scale = (1.9, 0.5, 0.06)
    finish(strip, rubber, bevel=0.03)
    parts += [badge, strip]
    group("speaker", parts)
    export("speaker")


build_stem_buds()
build_buds()
build_over_ear()
build_speaker()
