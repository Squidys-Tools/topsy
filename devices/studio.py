# The studio every headset is rendered in: the camera, the lights and a dark world for glossy
# parts to reflect, shared by render.py and by a live Blender session while modelling, so what
# is judged there is what ships. `setup()` frames everything visible in the scene.
import math

import bpy
from mathutils import Vector

SIZE = 256
SAMPLES = 96
# The angle a turntable starts and ends at: a three-quarter view from the front.
HERO = 25.0
ELEVATION = 18.0
LENS = 85.0
# How much of the frame the model's bounding sphere fills: below 1, it crops nothing at any turn.
FILL = 0.92


def _area(name, location, size, power, colour=(1.0, 1.0, 1.0)):
    data = bpy.data.lights.new(name, "AREA")
    data.shape = "RECTANGLE"
    data.size, data.size_y = size
    data.energy = power
    data.color = colour
    light = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(light)
    light.location = location
    light.rotation_euler = (Vector((0, 0, 0)) - light.location).to_track_quat("-Z", "Y").to_euler()
    light["studio"] = True
    return light


def _world():
    sky = bpy.data.worlds.get("studio") or bpy.data.worlds.new("studio")
    tree = sky.node_tree
    nodes, links = tree.nodes, tree.links
    for node in list(nodes):
        if node.type not in ("BACKGROUND", "OUTPUT_WORLD"):
            nodes.remove(node)
    background = nodes["Background"]
    coords = nodes.new("ShaderNodeTexCoord")
    mapping = nodes.new("ShaderNodeMapping")
    gradient = nodes.new("ShaderNodeTexGradient")
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


def models():
    """The scene's top-level objects that are the model, not the studio."""
    return [
        o
        for o in bpy.context.scene.objects
        if o.parent is None and "studio" not in o and o.type not in ("CAMERA", "LIGHT")
    ]


def setup():
    """Light and frame the scene's model; returns the pivot it turns on."""
    scene = bpy.context.scene
    for obj in [o for o in scene.objects if "studio" in o]:
        bpy.data.objects.remove(obj)
    # Parts just moved still report their old place until the scene is updated.
    bpy.context.view_layer.update()
    meshes = [o for o in scene.objects if o.type in ("MESH", "CURVE") and o.visible_get()]
    corners = [o.matrix_world @ Vector(c) for o in meshes for c in o.bound_box]
    low = Vector([min(c[i] for c in corners) for i in range(3)])
    high = Vector([max(c[i] for c in corners) for i in range(3)])
    centre = (low + high) / 2
    # The farthest point from the centre, not the box's corner, so compact shapes fill the frame.
    deps = bpy.context.evaluated_depsgraph_get()
    radius = 0.0
    for obj in meshes:
        evaluated = obj.evaluated_get(deps)
        mesh = evaluated.to_mesh()
        for vert in mesh.vertices:
            radius = max(radius, (evaluated.matrix_world @ vert.co - centre).length)
        evaluated.to_mesh_clear()

    pivot = bpy.data.objects.new("pivot", None)
    scene.collection.objects.link(pivot)
    pivot["studio"] = True
    for obj in models():
        if obj is not pivot:
            obj.parent = pivot
            obj.matrix_parent_inverse = pivot.matrix_world.inverted()
    pivot.location = centre
    for obj in pivot.children:
        obj.location -= centre
    pivot.location = (0, 0, 0)
    pivot.scale = (1 / radius,) * 3
    pivot.rotation_euler = (0, 0, math.radians(HERO))

    scene.render.engine = "CYCLES"
    scene.cycles.samples = SAMPLES
    scene.cycles.use_denoising = True
    scene.render.film_transparent = True
    scene.render.resolution_x = scene.render.resolution_y = SIZE
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.view_settings.view_transform = "AgX"
    _world()

    data = bpy.data.cameras.new("camera")
    data.lens = LENS
    camera = bpy.data.objects.new("camera", data)
    camera["studio"] = True
    scene.collection.objects.link(camera)
    scene.camera = camera
    distance = 1 / (FILL * math.sin(math.atan(18 / data.lens)))
    pitch = math.radians(ELEVATION)
    camera.location = (0, -distance * math.cos(pitch), distance * math.sin(pitch))
    camera.rotation_euler = (math.radians(90) - pitch, 0, 0)

    _area("key", (-3.5, -4.0, 4.0), (4.0, 4.0), 380)
    _area("fill", (4.5, -3.0, 1.0), (5.0, 3.0), 90, (0.92, 0.95, 1.0))
    _area("rim left", (-4.0, 4.0, 2.5), (1.5, 5.0), 600)
    _area("rim right", (4.0, 4.0, 2.5), (1.5, 5.0), 450, (0.9, 0.95, 1.0))
    _area("top", (0, 0, 6.0), (6.0, 6.0), 120)
    return pivot
