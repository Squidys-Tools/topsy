# Generic stemless earbuds: a pair of rounded beans in glossy graphite, each with a glassy touch
# pad on its outer face ringed in metal, and a silicone tip on a short nozzle. Real size, in
# metres; the camera looks along +Y. The tips point in and back, as a pair lies on a table.
import math

from mathutils import Vector

from shapes import ellipse_disc, loft, mat, metaball, stick


def materials():
    return {
        "shell": mat("gloss graphite", (0.12, 0.12, 0.13), 0.2, coat=0.6),
        "pad": mat("touch glass", (0.03, 0.03, 0.035), 0.06, coat=1.0),
        "ring": mat("satin silver", (0.7, 0.7, 0.72), 0.3, metallic=1.0),
        "silicone": mat("silicone", (0.08, 0.08, 0.085), 0.8, bump=(2200, 0.15)),
        "grille": mat("black mesh", (0.012, 0.012, 0.013), 0.55, bump=(5000, 0.5)),
    }


def bud(side, m):
    """One bud, built for the left (`side` -1) and mirrored for the right."""
    body = metaball(
        f"bud {side}",
        [
            # The bean: a wide rounded shell, fuller at its outer face.
            ("ELLIPSOID", (0, 0, 0), 0.0116, (1.0, 0.86, 0.9), None),
            # A dome on the outer face, toward the camera.
            ("BALL", (-0.0008, -0.0026, 0.0004), 0.0088, None, None),
            # The nozzle the tip sits on, toward the ear: in and back.
            ("BALL", (0.0036, 0.0046, -0.0008), 0.0058, None, None),
        ],
        material=m["shell"],
    )
    parts = [body]

    # The touch pad on the outer face, ringed in metal.
    ring = stick(body, ellipse_disc(f"pad ring {side}", 0.0043, 0.0037, 0.0004, m["ring"]),
                 (-0.0012, -0.05, 0.0004), (0, 1, 0), inset=0.00042)
    pad = stick(body, ellipse_disc(f"pad {side}", 0.0039, 0.0033, 0.0004, m["pad"]),
                (-0.0012, -0.05, 0.0004), (0, 1, 0), inset=0.00034)
    mic = stick(body, ellipse_disc(f"mic {side}", 0.0006, 0.0006, 0.0004, m["grille"]),
                (-0.003, -0.002, 0.05), (0, 0, -1), inset=0.0003)
    parts += [ring, pad, mic]

    aim = Vector((0.42, 0.88, -0.2)).normalized()
    nozzle = Vector((0.0036, 0.0046, -0.0008))
    hit, point, _, _ = body.ray_cast(nozzle + aim * 0.05, -aim)
    if not hit:
        raise RuntimeError("the ear tip's ray misses the bud")
    # Sunk a little into the nozzle, so it sits on it rather than touching it.
    base = point - aim * 0.0009
    tip = loft(
        f"ear tip {side}",
        [
            (0.0000, 0.0030, 0.0030),
            (0.0010, 0.0043, 0.0043),
            (0.0026, 0.0052, 0.0052),
            (0.0040, 0.0049, 0.0049),
            (0.0050, 0.0040, 0.0040),
            (0.0055, 0.0026, 0.0026),
        ],
        "X",
        material=m["silicone"],
        n=2.0,
        closed=True,
    )
    tip.location = base
    tip.rotation_euler = Vector((1, 0, 0)).rotation_difference(aim).to_euler()
    parts.append(tip)

    for part in parts:
        if part is not body:
            part.parent = body
    body.location = (0.0165 * side, 0, 0)
    body.scale = (-1, 1, 1) if side > 0 else (1, 1, 1)
    body.rotation_euler = (math.radians(-8), 0, math.radians(14 * side))
    return parts


def build():
    m = materials()
    return bud(-1, m) + bud(1, m)
