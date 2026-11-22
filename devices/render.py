# Renders a headset's model into the turntable topsy plays when it connects: FRAMES views a
# full turn apart, starting from the hero angle, on a transparent background, lit by one studio
# rig shared by every model so they all look like they belong together.
# usage: blender --background --factory-startup --python devices/render.py -- <model.glb> <out dir> [preview]
#        (preview renders the hero frame alone)
#        (or `cargo devices render <folder>`, which also packs the frames into spin.png)
import math
import os
import sys

import bpy
from mathutils import Vector

FRAMES = 24
SIZE = 256
SAMPLES = 96
# The angle the turn starts and ends at: a three-quarter view from the front.
HERO = 25.0
ELEVATION = 18.0


def args():
    rest = sys.argv[sys.argv.index("--") + 1 :]
    if len(rest) not in (2, 3):
        sys.exit("usage: blender -b -P render.py -- <model.glb> <out dir> [preview]")
    return rest[0], rest[1], rest[2:] == ["preview"]


def area(name, location, size, power, colour=(1.0, 1.0, 1.0)):
    data = bpy.data.lights.new(name, "AREA")
    data.shape = "RECTANGLE"
    data.size, data.size_y = size
    data.energy = power
    data.color = colour
    light = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(light)
    light.location = location
    direction = Vector((0, 0, 0)) - light.location
    light.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    return light


def world():
    sky = bpy.data.worlds.new("studio")
    nodes, links = sky.node_tree.nodes, sky.node_tree.links
    background = nodes["Background"]
    gradient = nodes.new("ShaderNodeTexGradient")
    coords = nodes.new("ShaderNodeTexCoord")
    mapping = nodes.new("ShaderNodeMapping")
    ramp = nodes.new("ShaderNodeValToRGB")
    # A dark floor and a brighter sky, for something to reflect.
    mapping.inputs["Rotation"].default_value = (0, math.radians(-90), 0)
    ramp.color_ramp.elements[0].color = (0.004, 0.004, 0.005, 1)
    ramp.color_ramp.elements[1].position = 0.75
    ramp.color_ramp.elements[1].color = (0.08, 0.085, 0.09, 1)
    links.new(coords.outputs["Generated"], mapping.inputs["Vector"])
    links.new(mapping.outputs["Vector"], gradient.inputs["Vector"])
    links.new(gradient.outputs["Fac"], ramp.inputs["Fac"])
    links.new(ramp.outputs["Color"], background.inputs["Color"])
    background.inputs["Strength"].default_value = 1.0
    bpy.context.scene.world = sky


def main():
    model, out, preview = args()
    os.makedirs(out, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=model)

    # Everything turns on one pivot at the model's centre, scaled to a unit sphere.
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    corners = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
    low = Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
    high = Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
    centre, radius = (low + high) / 2, (high - low).length / 2
    pivot = bpy.data.objects.new("pivot", None)
    bpy.context.collection.objects.link(pivot)
    for obj in bpy.context.scene.objects:
        if obj.parent is None and obj is not pivot:
            obj.location -= centre
            obj.parent = pivot
    pivot.scale = (1 / radius,) * 3

    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = SAMPLES
    scene.cycles.use_denoising = True
    scene.render.film_transparent = True
    scene.render.resolution_x = scene.render.resolution_y = SIZE
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.view_settings.view_transform = "AgX"
    world()

    data = bpy.data.cameras.new("camera")
    data.lens = 85
    camera = bpy.data.objects.new("camera", data)
    scene.collection.objects.link(camera)
    scene.camera = camera
    # Far enough that the unit sphere fills the frame with a little room.
    distance = 0.94 / math.sin(math.atan(18 / data.lens))
    pitch = math.radians(ELEVATION)
    camera.location = (0, -distance * math.cos(pitch), distance * math.sin(pitch))
    camera.rotation_euler = (math.radians(90) - pitch, 0, 0)

    area("key", (-3.5, -4.0, 4.0), (4.0, 4.0), 380)
    area("fill", (4.5, -3.0, 1.0), (5.0, 3.0), 90, (0.92, 0.95, 1.0))
    area("rim left", (-4.0, 4.0, 2.5), (1.5, 5.0), 600)
    area("rim right", (4.0, 4.0, 2.5), (1.5, 5.0), 450, (0.9, 0.95, 1.0))
    area("top", (0, 0, 6.0), (6.0, 6.0), 120)

    for frame in range(1 if preview else FRAMES):
        pivot.rotation_euler = (0, 0, math.radians(HERO + 360.0 * frame / FRAMES))
        scene.render.filepath = os.path.join(out, f"{frame:02}.png")
        bpy.ops.render.render(write_still=True)


main()
