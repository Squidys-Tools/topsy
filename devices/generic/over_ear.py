# Generic over-ear headphones: oval domed cups on padded cushions, held by yokes on metal sliders
# under a flat padded headband. Real size, in metres; the wearer faces -Y.
import math

from mathutils import Vector

from shapes import cushion_section, loft, mat, oval, rounded_rect, sweep

# The cup's oval, across the ear: half its depth front to back, and half its height.
CUP_A, CUP_B = 0.034, 0.045
# The cups' outer faces, from the middle of the head.
CUP_X = 0.088
HEADBAND_TOP = 0.150
# Where the band's ends meet the sliders.
HEADBAND_FOOT = 0.074


# A space-grey shell reads on the black notch; an all-black one disappears at the card's size.
SHELL = (0.16, 0.16, 0.17)


def materials(shell=SHELL):
    return {
        "shell": mat("soft-touch plastic", shell, 0.58, bump=(900, 0.05)),
        "leather": mat(
            "protein leather",
            (0.024, 0.024, 0.026),
            0.48,
            coat=0.08,
            bump=(1800, 0.3),
            wrinkle=(140, 0.12),
        ),
        "metal": mat("brushed aluminium", (0.72, 0.72, 0.74), 0.28, metallic=1.0, anisotropic=0.6),
        "mesh": mat("speaker cloth", (0.01, 0.01, 0.011), 0.85, bump=(2600, 0.6)),
    }


def cup(side, m):
    """One cup: `side` is -1 for the left (−X), 1 for the right."""
    def x(offset):
        return side * (CUP_X - 0.032 + offset)

    shell_rings = [
        (0.000, CUP_A * 0.98, CUP_B * 0.98),
        (0.003, CUP_A * 1.02, CUP_B * 1.02),
        (0.012, CUP_A * 1.03, CUP_B * 1.03),
        (0.021, CUP_A * 0.98, CUP_B * 0.98),
        (0.027, CUP_A * 0.88, CUP_B * 0.89),
        (0.0305, CUP_A * 0.70, CUP_B * 0.72),
        (0.032, CUP_A * 0.40, CUP_B * 0.42),
    ]
    shell = loft(
        f"cup shell {side}",
        [(x(o), a, b) for o, a, b in shell_rings],
        "X",
        material=m["shell"],
    )
    parts = [shell]

    # The cushion: a slim block of foam under leather, swept round just inside the cup's edge,
    # narrowing toward the head.
    path = []
    for k in range(96):
        u, v = oval(2 * math.pi * k / 96, CUP_A * 0.78, CUP_B * 0.79)
        path.append(Vector((x(-0.0071), u, v)))
    cushion = sweep(
        f"cushion {side}",
        path,
        cushion_section(0.018, 0.015, taper=0.16, n=3.4),
        up=Vector((-side, 0, 0)),
        closed=True,
        material=m["leather"],
    )
    parts.append(cushion)

    # The cloth inside the cushion, over the driver.
    cloth = loft(
        f"cloth {side}",
        [(x(-0.003), CUP_A * 0.62, CUP_B * 0.62), (x(-0.0035), CUP_A * 0.3, CUP_B * 0.3)],
        "X",
        material=m["mesh"],
    )
    parts.append(cloth)

    # The yoke: a flat fork hugging the top of the cup, pinned to it at its front and back.
    reach = 0.0023
    yoke_path = []
    for k in range(25):
        t = math.pi * k / 24
        u, v = oval(t, CUP_A * 1.03 + reach, CUP_B * 1.03 + reach)
        yoke_path.append(Vector((x(0.012), -u, v)))
    yoke = sweep(
        f"yoke {side}",
        yoke_path,
        rounded_rect(0.0045, 0.010, 0.0018),
        up=Vector((side, 0, 0)),
        material=m["shell"],
    )
    parts.append(yoke)
    # Each arm ends in a solid round boss seated on the cup, with a metal pin head on its outside.
    # A loft caps its last ring, so the rings run from inside the cup outward.
    arm_out = CUP_A * 1.03 + reach + 0.00225
    for sign in (1, -1):
        boss = loft(
            f"hinge {side} {sign}",
            [
                (sign * CUP_A * 0.95, 0.0064, 0.0064),
                (sign * (arm_out - 0.0012), 0.0064, 0.0064),
                (sign * (arm_out - 0.0003), 0.0057, 0.0057),
                (sign * arm_out, 0.0040, 0.0040),
            ],
            "Y",
            material=m["shell"],
            n=2.0,
        )
        boss.location.x = x(0.012)
        pin = loft(
            f"pin {side} {sign}",
            [
                (sign * (arm_out - 0.0004), 0.0026, 0.0026),
                (sign * (arm_out + 0.0003), 0.0025, 0.0025),
                (sign * (arm_out + 0.0006), 0.0016, 0.0016),
            ],
            "Y",
            material=m["metal"],
            n=2.0,
        )
        pin.location.x = x(0.012)
        parts += [boss, pin]

    # The slider: a metal bar rising out of the yoke's top and into the headband's end.
    top = CUP_B * 1.03 + reach
    slider_path = [
        Vector((x(0.012), 0, top - 0.002 + (HEADBAND_FOOT + 0.006 - top + 0.002) * k / 5))
        for k in range(6)
    ]
    slider = sweep(
        f"slider {side}",
        slider_path,
        rounded_rect(0.014, 0.0032, 0.0014),
        up=Vector((side, 0, 0)),
        material=m["metal"],
    )
    parts.append(slider)
    return parts


def headband(m):
    """A flat band arching between the sliders, its padding underneath."""
    def arc(inset, count=41, spread=1.0):
        points = []
        for k in range(count):
            t = math.pi * (0.5 + (k / (count - 1) - 0.5) * spread)
            # Wider than it is tall, and a little flat on top, as a band on a head is.
            px = (CUP_X - 0.020 - inset) * math.cos(t) * -1
            pz = HEADBAND_FOOT + (HEADBAND_TOP - HEADBAND_FOOT - inset) * math.sin(t) ** 0.8
            points.append(Vector((px, 0, pz)))
        return points

    # Sections are (across the band, along Y): thin across, wide front to back.
    band = sweep(
        "headband", arc(0.0), rounded_rect(0.0075, 0.032, 0.0034), up=Vector((0, 1, 0)), material=m["shell"]
    )
    pad = sweep(
        "headband pad",
        arc(0.0085, spread=0.62),
        rounded_rect(0.011, 0.026, 0.0048),
        up=Vector((0, 1, 0)),
        material=m["leather"],
    )
    return [band, pad]


def build(shell=SHELL):
    m = materials(shell)
    parts = headband(m)
    for side in (-1, 1):
        parts += cup(side, m)
    return parts
