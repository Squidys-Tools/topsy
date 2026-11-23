# Stem earbuds after AirPods Pro 3: a pair of glossy white buds, each a smooth pebble of a head
# overhanging a short slender stem with a chrome cap, a soft white silicone tip pressed against
# the head, and a black pill vent on top. Real size, in metres; the camera looks along +Y. Each
# bud is built with its tip along +X; the right bud is the left one mirrored, so the tips face.
import math

from mathutils import Quaternion, Vector

from shapes import ellipse_disc, loft, mat, metaball, stick

WHITE = (0.9, 0.9, 0.885)


def materials():
    return {
        "shell": mat("gloss white", WHITE, 0.14, coat=0.8),
        "silicone": mat("white silicone", (0.86, 0.86, 0.85), 0.62, bump=(2400, 0.12)),
        "grille": mat("black mesh", (0.012, 0.012, 0.013), 0.55, bump=(5000, 0.5)),
        "chrome": mat("chrome", (0.8, 0.8, 0.82), 0.12, metallic=1.0),
    }


def bud(side, m):
    """One bud, built for the left (`side` -1) and mirrored for the right."""
    # The stem leans back a little, its foot further from the tip than its top.
    lean = Quaternion(Vector((0, 1, 0)), math.radians(90 + 8))
    stem_centre = Vector((-0.0056, 0, -0.0136))
    stem_down = lean @ Vector((1, 0, 0))
    body = metaball(
        f"bud {side}",
        [
            # The head: a compact pebble, a little longer toward the tip than it is tall.
            ("ELLIPSOID", (0, 0, 0), 0.0118, (1.08, 0.92, 0.84), None),
            # Fuller toward the tip, where it meets the ear.
            ("BALL", (0.0036, 0, -0.0006), 0.0076, None, None),
            # The stem: short, even and fairly stout, flowing out of the head's back.
            ("BALL", (-0.0046, 0, -0.0064), 0.0068, None, None),
            ("CAPSULE", tuple(stem_centre), 0.0056, (0.0054, 1.0, 1.0), lean),
        ],
        material=m["shell"],
    )
    # Blended shapes leave faint ripples where they meet; a light smoothing irons them out.
    smooth = body.modifiers.new("smooth", "SMOOTH")
    smooth.factor, smooth.iterations = 0.8, 12
    parts = [body]

    # The silicone tip, a soft cone pressed against the head along `aim`: in and a little down.
    aim = Vector((1.0, -0.06, -0.08)).normalized()
    origin = Vector((0.004, 0, -0.0008))
    hit, point, _, _ = body.ray_cast(origin + aim * 0.05, -aim)
    if not hit:
        raise RuntimeError("the ear tip's ray misses the bud")
    base = point - aim * 0.0012
    tip = loft(
        f"ear tip {side}",
        # A short, wide cylinder of silicone with a rounded face.
        [
            (0.0000, 0.0046, 0.0046),
            (0.0010, 0.0053, 0.0053),
            (0.0042, 0.0054, 0.0054),
            (0.0054, 0.0049, 0.0049),
            (0.0060, 0.0038, 0.0038),
            (0.0062, 0.0024, 0.0024),
        ],
        "X",
        material=m["silicone"],
        n=2.0,
        closed=True,
    )
    tip.location = base
    tip.rotation_euler = Vector((1, 0, 0)).rotation_difference(aim).to_euler()
    mesh = ellipse_disc(f"tip mesh {side}", 0.0018, 0.0018, 0.0004, m["grille"])
    mesh.location = base + aim * 0.0061
    mesh.rotation_euler = Vector((0, 0, 1)).rotation_difference(aim).to_euler()
    parts += [tip, mesh]

    # The black oval vent on the head's front, and the small sensor facing the ear.
    vent = stick(body, ellipse_disc(f"front vent {side}", 0.0017, 0.0012, 0.0005, m["grille"]),
                 Vector((-0.001, 0, 0.0035)) + Vector((0, -1, 0.55)).normalized() * 0.05,
                 -Vector((0, -1, 0.55)).normalized(), inset=0.00022)
    sensor = stick(body, ellipse_disc(f"sensor {side}", 0.0011, 0.0011, 0.0004, m["grille"]),
                   (0.002, 0.05, 0.0042), (0, -1, 0), inset=0.0004)
    # The chrome cap on the stem's foot.
    cap = stick(body, ellipse_disc(f"stem cap {side}", 0.0021, 0.0021, 0.0006, m["chrome"]),
                stem_centre + stem_down * 0.05, -stem_down, inset=0.0005)
    parts += [vent, sensor, cap]

    for part in parts:
        if part is not body:
            part.parent = body
    body.location = (0.0185 * side, 0, 0)
    body.scale = (-1, 1, 1) if side > 0 else (1, 1, 1)
    body.rotation_euler = (0, 0, math.radians(-12 * side))
    return parts


def build():
    m = materials()
    return bud(-1, m) + bud(1, m)
