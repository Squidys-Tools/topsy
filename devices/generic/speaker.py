# A generic portable speaker: a fabric-wrapped cylinder lying on its side, with rubber end caps
# carrying passive radiators, a rubber badge on the front and a rubber foot underneath. Real
# size, in metres; the camera looks along +Y.
from shapes import ellipse_disc, loft, mat

LENGTH = 0.165
RADIUS = 0.034


def materials():
    return {
        "fabric": mat("woven fabric", (0.035, 0.07, 0.11), 0.92, bump=(4000, 0.25)),
        "rubber": mat("rubber", (0.018, 0.018, 0.02), 0.62, bump=(3000, 0.1)),
        "radiator": mat("radiator", (0.03, 0.03, 0.032), 0.35, coat=0.3),
        "badge": mat("badge", (0.75, 0.75, 0.77), 0.25, metallic=1.0),
    }


def build():
    m = materials()
    half = LENGTH / 2
    body = loft(
        "body",
        [
            (-half + 0.002, RADIUS - 0.002, RADIUS - 0.002),
            (-half + 0.004, RADIUS, RADIUS),
            (half - 0.004, RADIUS, RADIUS),
            (half - 0.002, RADIUS - 0.002, RADIUS - 0.002),
        ],
        "X",
        segments=96,
        material=m["fabric"],
        n=2.0,
        closed=True,
    )
    parts = [body]
    for side in (-1, 1):
        cap = loft(
            f"end cap {side}",
            [
                (side * (half - 0.006), RADIUS + 0.0012, RADIUS + 0.0012),
                (side * (half + 0.003), RADIUS + 0.0012, RADIUS + 0.0012),
                (side * (half + 0.006), RADIUS - 0.0012, RADIUS - 0.0012),
                (side * (half + 0.0072), RADIUS - 0.005, RADIUS - 0.005),
            ],
            "X",
            segments=96,
            material=m["rubber"],
            n=2.0,
            closed=True,
        )
        radiator = loft(
            f"radiator {side}",
            [
                (side * (half + 0.0066), 0.021, 0.021),
                (side * (half + 0.0078), 0.020, 0.020),
                (side * (half + 0.0082), 0.017, 0.017),
                (side * (half + 0.0090), 0.012, 0.012),
            ],
            "X",
            segments=96,
            material=m["radiator"],
            n=2.0,
            closed=True,
        )
        logo = loft(
            f"radiator logo {side}",
            [(side * (half + 0.0088), 0.0055, 0.0055), (side * (half + 0.0094), 0.005, 0.005)],
            "X",
            material=m["badge"],
            n=2.0,
            closed=True,
        )
        parts += [cap, radiator, logo]
    badge = ellipse_disc("badge", 0.0075, 0.0045, 0.0012, m["badge"], n=4.0)
    badge.location = (0, -RADIUS - 0.0002, 0.003)
    badge.rotation_euler = (1.5708, 0, 0)
    foot = loft(
        "foot",
        [(-0.002, 0.05, 0.006), (0.002, 0.05, 0.006), (0.004, 0.046, 0.005)],
        "Z",
        material=m["rubber"],
        n=4.0,
        closed=True,
    )
    foot.location = (0, 0, -RADIUS - 0.0012)
    foot.rotation_euler = (3.14159, 0, 0)
    parts += [badge, foot]
    return parts
