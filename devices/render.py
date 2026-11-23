# Renders a headset into the turntable topsy plays when it connects: FRAMES views a full turn
# apart, starting from the hero angle, on a transparent background, in the studio every model
# shares (studio.py), so they all look like they belong together.
# usage: blender --background --factory-startup --python devices/render.py -- <source> <out dir> [preview]
#   source: a model.glb or model.blend, or a generic shape's module (devices/generic/over_ear.py)
#   preview: render the hero frame alone
#   (or `cargo devices render <folder>` / `cargo devices generic`, which also pack spin.png)
import importlib
import math
import os
import sys

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
for path in (HERE, os.path.join(HERE, "generic")):
    if path not in sys.path:
        sys.path.insert(0, path)

import studio  # noqa: E402

FRAMES = 24


def args():
    rest = sys.argv[sys.argv.index("--") + 1 :]
    if len(rest) not in (2, 3):
        sys.exit("usage: blender -b -P render.py -- <source> <out dir> [preview]")
    return rest[0], rest[1], rest[2:] == ["preview"]


def load(source):
    if source.endswith(".py"):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        module = importlib.import_module(os.path.splitext(os.path.basename(source))[0])
        module.build()
    elif source.endswith(".blend"):
        bpy.ops.wm.open_mainfile(filepath=source)
    else:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        bpy.ops.import_scene.gltf(filepath=source)


def main():
    source, out, preview = args()
    os.makedirs(out, exist_ok=True)
    load(source)
    pivot = studio.setup()
    scene = bpy.context.scene
    for frame in range(1 if preview else FRAMES):
        pivot.rotation_euler = (0, 0, math.radians(studio.HERO + 360.0 * frame / FRAMES))
        scene.render.filepath = os.path.join(out, f"{frame:02}.png")
        bpy.ops.render.render(write_still=True)


main()
