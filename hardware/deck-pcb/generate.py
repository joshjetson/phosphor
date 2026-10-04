"""Generate the Phosphor Deck's controller board as a KiCad PCB.

Run with KiCad's own Python, which carries the `pcbnew` module:

    <KiCad.app>/Contents/Frameworks/Python.framework/Versions/Current/bin/python3 \
        hardware/deck-pcb/generate.py <KiCad footprint library folder>

Everything comes from two places. Which control is wired to which input,
and which ones light, comes from `firmware/deck-layout.json` — Phosphor's own
control table, by the same rule the firmware's build uses. Where each control
sits comes from the panel drawing, transcribed below in millimetres.

Controls go on the top side (all through-hole), everything else on the
bottom (all surface-mount), so a board house assembles one side only. The
LEDs are reverse-mount parts on the bottom that shine up through a hole.
Parts on the bottom are placed by searching outward from where they want
to be (beside the controls they serve) for space that touches no pin.

The result is unrouted; `route.sh` routes it.
"""
import json
import math
import os
import sys

import pcbnew

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
LIB = sys.argv[1]
OUT = os.path.join(HERE, "deck.kicad_pcb")

MM = 1_000_000


def mm(v):
    return int(round(v * MM))


def vec(x, y):
    return pcbnew.VECTOR2I(mm(x), mm(y))


# ── The control table ────────────────────────────────────────────────────
layout = json.load(open(os.path.join(ROOT, "firmware", "deck-layout.json")))
controls = layout["controls"]

# Shift-register inputs in table order: a button is one, an encoder three
# (push, A, B), and the sustain pedal comes last. The firmware's build.rs and
# the schematics follow the same rule.
digital = []
for c in controls:
    if c["kind"] == "Button":
        digital.append((c["id"], "switch"))
    elif c["kind"] == "Encoder":
        digital += [(c["id"], "push"), (c["id"], "A"), (c["id"], "B")]
digital.append(("Sustain", "pedal"))
CHIPS = 12
INPUTS = CHIPS * 8
input_of = {(cid, role): i for i, (cid, role) in enumerate(digital)}

# ── The panel, in millimetres (the design page's top view) ───────────────
# Each control: centre (x, y) and, for buttons, its cap's width and height.
panel = {}


def button(cid, x, y, w, h):
    panel[cid] = ("button", x + w / 2, y + h / 2, w, h)


for i, cid in enumerate(["Play", "Stop", "Rec", "Overdub", "Loop", "Click", "CountIn", "Tap"]):
    button(cid, 28 + (i % 4) * 15.5, 46 + (i // 4) * 17, 14, 11)
panel["Function"] = ("encoder", 107, 66, 0, 0)
for i, cid in enumerate(["FnTempo", "FnSwing", "FnGrid", "FnMaster", "FnLoop", "FnLast"]):
    button(cid, 120 + (i % 3) * 13, 54 + (i // 3) * 14, 12, 10)
panel["Navigate"] = ("encoder", 181, 58, 0, 0)
for i, cid in enumerate(["Lock", "Back", "Browse"]):
    button(cid, 198, 41 + i * 9.5, 28, 8)
button("Shift", 229, 41, 23, 8)
button("Menu", 229, 50.5, 23, 8)
button("Part", 229, 60, 23, 8)
for i, cid in enumerate(["Left", "Up", "Down", "Right"]):
    button(cid, 198 + i * 13.5, 69.5, 12, 8)
for i, cid in enumerate(["Undo", "Redo", "Copy", "Paste", "Delete", "Duplicate", "Save", "New"]):
    button(cid, 261 + (i % 4) * 13.5, 46 + (i // 4) * 17, 12.5, 11)
for n in range(8):
    x = 39 + n * 30
    panel[f"Knob({n})"] = ("encoder", x, 103, 0, 0)
    button(f"Action({n})", x - 11, 115, 22, 7)
    button(f"TrackButton({n})", x - 11, 128, 22, 8)
    # A PTA4543's body is 60 mm long, more than the 48 mm slot: centred at
    # 167 it clears the track buttons above and the pads below, and the
    # top plate's slot is 143-191 mm.
    panel[f"Fader({n})"] = ("fader", x, 167, 0, 0)
for n in range(16):
    x = 39 + (n % 8) * 30
    panel[f"Pad({n})"] = ("pad", x, 212 + (n // 8) * 32, 26, 28)
button("PagePrev", 269, 95, 21, 9)
button("PageNext", 292, 95, 21, 9)
button("MyPage", 269, 108, 44, 8)
for i, cid in enumerate(["ModeSelect", "ModeMute", "ModeSolo", "ModeArm"]):
    button(cid, 269 + (i % 2) * 23, 128 + (i // 2) * 11, 21, 9)
button("TrackBankPrev", 269, 154, 21, 9)
button("TrackBankNext", 292, 154, 21, 9)
button("PadsUp", 269, 200, 44, 10)
button("PadsDown", 269, 213, 44, 10)
button("StepsLow", 269, 232, 21, 10)
button("StepsHigh", 292, 232, 21, 10)
missing = [c["id"] for c in controls if c["id"] not in panel]
assert not missing, f"controls with no place on the panel: {missing}"

# The board: inside a 300 x 240 base whose corner is at (20, 30).
BOARD = (22.0, 34.0, 318.0, 268.0)

# ── Board and nets ───────────────────────────────────────────────────────
board = pcbnew.CreateEmptyBoard()
board.SetCopperLayerCount(2)
nets = {}


def net(name):
    if name not in nets:
        item = pcbnew.NETINFO_ITEM(board, name)
        board.Add(item)
        nets[name] = item
    return nets[name]


def inp(i):
    return f"IN{i}"


# ── Placement ────────────────────────────────────────────────────────────
footprints = []
occupied = []  # (x0, y0, x1, y1, side) in mm; side "top", "bottom" or "both"
counters = {}
report = {"lights_as_dots": [], "moved": []}


def ref(prefix):
    counters[prefix] = counters.get(prefix, 0) + 1
    return f"{prefix}{counters[prefix]}"


def load(lib, name):
    fp = pcbnew.FootprintLoad(os.path.join(LIB, lib + ".pretty"), name)
    assert fp is not None, f"{lib}:{name}"
    return fp


def anchor(fp):
    """Where a footprint is aimed: the middle of its mounting tabs (an
    encoder's shaft, a fader's lever), or else of its pads."""
    pads = list(fp.Pads())
    mp = [p for p in pads if p.GetNumber() == "MP"]
    use = mp or pads
    xs = [p.GetPosition().x for p in use]
    ys = [p.GetPosition().y for p in use]
    return (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2


def bbox(fp, grow=0.0):
    b = fp.GetBoundingBox(False, False)
    return (b.GetLeft() / MM - grow, b.GetTop() / MM - grow, b.GetRight() / MM + grow, b.GetBottom() / MM + grow)


def pad_boxes(fp, grow):
    out = []
    for p in fp.Pads():
        b = p.GetBoundingBox()
        out.append((b.GetLeft() / MM - grow, b.GetTop() / MM - grow, b.GetRight() / MM + grow, b.GetBottom() / MM + grow))
    return out


def overlaps(a, b):
    return a[0] < b[2] and b[0] < a[2] and a[1] < b[3] and b[1] < a[3]


def put(fp, x, y, angle=0.0, bottom=False):
    fp.SetOrientationDegrees(angle)
    if bottom:
        fp.Flip(fp.GetPosition(), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)
    ax, ay = anchor(fp)
    fp.Move(pcbnew.VECTOR2I(int(mm(x) - ax), int(mm(y) - ay)))


def add(lib, name, prefix, value, x, y, angle=0.0, bottom=False, pins=None):
    fp = load(lib, name)
    fp.SetReference(ref(prefix))
    fp.SetValue(value)
    board.Add(fp)
    put(fp, x, y, angle, bottom)
    for p in fp.Pads():
        n = (pins or {}).get(p.GetNumber())
        if n:
            p.SetNet(net(n))
    # Designators go on the assembly drawing, not the silkscreen: on top
    # they would sit under the caps, underneath they crowd the pads.
    fp.Reference().SetLayer(pcbnew.F_Fab if not bottom else pcbnew.B_Fab)
    fp.Value().SetVisible(False)
    footprints.append(fp)
    return fp


def claim(fp, side):
    """Mark what a placed footprint takes up. Through-hole pads take both
    sides; a surface part takes its whole body on its own side."""
    if fp.GetAttributes() & pcbnew.FP_THROUGH_HOLE or side == "top":
        for b in pad_boxes(fp, 0.35):
            occupied.append(b + ("both",))
    if side == "bottom":
        occupied.append(bbox(fp, 0.3) + ("bottom",))


def free(box, side):
    x0, y0, x1, y1 = box
    if x0 < BOARD[0] + 1.5 or y0 < BOARD[1] + 1.5 or x1 > BOARD[2] - 1.5 or y1 > BOARD[3] - 1.5:
        return False
    return not any(overlaps(box, o[:4]) and o[4] in ("both", side) for o in occupied)


def keep_out_of_hole(fp):
    """No track or via near a reverse-mount LED's light hole. The hole is
    drawn inside the footprint, which the autorouter does not see."""
    cuts = [g.GetBoundingBox() for g in fp.GraphicalItems() if g.GetLayer() == pcbnew.Edge_Cuts]
    if not cuts:
        return
    x0 = min(b.GetLeft() for b in cuts) - mm(0.3)
    y0 = min(b.GetTop() for b in cuts) - mm(0.3)
    x1 = max(b.GetRight() for b in cuts) + mm(0.3)
    y1 = max(b.GetBottom() for b in cuts) + mm(0.3)
    area = pcbnew.ZONE(board)
    layers = pcbnew.LSET()
    layers.AddLayer(pcbnew.F_Cu)
    layers.AddLayer(pcbnew.B_Cu)
    area.SetLayerSet(layers)
    area.SetIsRuleArea(True)
    area.SetDoNotAllowTracks(True)
    area.SetDoNotAllowVias(True)
    area.SetDoNotAllowPads(False)
    area.SetDoNotAllowFootprints(False)
    area.SetDoNotAllowZoneFills(False)
    poly = area.Outline()
    poly.NewOutline()
    for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)):
        poly.Append(x, y)
    board.Add(area)


def place_bottom(lib, name, prefix, value, x, y, pins=None, angles=(0, 90), radius=45.0):
    """A bottom-side part as close to (x, y) as it fits."""
    fp = add(lib, name, prefix, value, x, y, angles[0], bottom=True, pins=pins)
    step = 0.5
    for r in [0.0] + [step * k for k in range(1, int(radius / step))]:
        ring = [(0.0, 0.0)] if r == 0 else [
            (r * math.cos(t), r * math.sin(t)) for t in [2 * math.pi * k / max(8, int(r * 3)) for k in range(max(8, int(r * 3)))]
        ]
        for dx, dy in ring:
            for a in angles:
                fp.SetOrientationDegrees(a + 180)  # flipped parts read upside down
                ax, ay = anchor(fp)
                fp.Move(pcbnew.VECTOR2I(int(mm(x + dx) - ax), int(mm(y + dy) - ay)))
                if free(bbox(fp, 0.3), "bottom"):
                    claim(fp, "bottom")
                    if r > 8:
                        report["moved"].append((fp.GetReference(), round(r, 1)))
                    return fp
    raise SystemExit(f"no room for {fp.GetReference()} ({name}) near ({x:.1f}, {y:.1f})")


# Mounting holes first: they claim their space on both sides.
for hx, hy in [(26, 38), (314, 38), (26, 264), (314, 264), (264, 120), (264, 195)]:
    fp = add("MountingHole", "MountingHole_3.2mm_M3", "H", "M3", hx, hy)
    occupied.append((hx - 3.6, hy - 3.6, hx + 3.6, hy + 3.6, "both"))

# ── Top: the controls ────────────────────────────────────────────────────
for c in controls:
    cid, kind = c["id"], c["kind"]
    _, x, y, w, h = panel[cid]
    if kind == "Button":
        i = input_of[(cid, "switch")]
        fp = add("Button_Switch_THT", "SW_PUSH_6mm", "SW", c["label"], x, y, 0,
                 pins={"1": inp(i), "2": "GND"})
    elif kind == "Encoder":
        pins = {"A": inp(input_of[(cid, "A")]), "B": inp(input_of[(cid, "B")]), "C": "GND",
                "S1": inp(input_of[(cid, "push")]), "S2": "GND", "MP": "GND"}
        fp = add("Rotary_Encoder", "RotaryEncoder_Alps_EC11E-Switch_Vertical_H20mm", "ENC", c["label"], x, y, 0, pins=pins)
    elif kind == "Fader":
        n = int(cid[6:-1])
        fp = add("Potentiometer_THT", "Potentiometer_Bourns_PTA4543_Single_Slide", "RV", c["label"], x, y, 90,
                 pins={"1": "GND", "2": f"FADER{n}", "3": "+3V3", "MP": "GND"})
    elif kind == "Pad":
        n = int(cid[4:-1])
        # The FSR's two solder tabs, 2.54 mm apart at the end of its tail;
        # the sensing disc lies on the board above them, under the pad.
        fp = add("Connector_PinHeader_2.54mm", "PinHeader_1x02_P2.54mm_Vertical", "FSR", c["label"], x, y + 10, 90,
                 pins={"1": "+3V3", "2": f"PAD{n}"})
    claim(fp, "top")

# The pedal jacks' header, top side, by the right-hand rear corner.
fp = add("Connector_PinHeader_2.54mm", "PinHeader_1x04_P2.54mm_Vertical", "J", "PEDALS", 300, 182, 90,
         pins={"1": inp(input_of[("Sustain", "pedal")]), "2": "EXPR", "3": "EXPR_RING", "4": "GND"})
claim(fp, "top")
pedal_at = (300, 182)

# ── Bottom: the lights ───────────────────────────────────────────────────
lit = sorted((c for c in controls if c.get("light") is not None), key=lambda c: c["light"])
led_at = []
for c in lit:
    cid = c["id"]
    kind, x, y, w, h = panel[cid]
    k = c["light"]
    pins = {"1": "GND", "2": "LED_DIN0" if k == 0 else f"LED{k}", "3": "+5V", "4": f"LED{k + 1}"}
    # (x, y, angle) to try in turn.
    if kind == "pad":
        spots = [(x + 9, y - 10.5, 0), (x - 9, y - 10.5, 0)]
    else:
        # Inside the cap, turned on end beside the switch, when the cap is
        # wide enough: the switch's pins reach 4.25 mm out and the light's
        # 1.95 mm, with clearance between. Otherwise a dot above or below.
        side = 4.25 + 0.35 + 1.95 + 0.2
        spots = [(x + side, y, 90), (x - side, y, 90)] if w / 2 >= side + 1.95 else []
        spots += [(x, y - h / 2 - 2.2, 0), (x, y + h / 2 + 2.2, 0)]
    placed = None
    for sx, sy, angle in spots:
        fp = add("LED_SMD", "LED_SK6812MINI-E_3.2x2.8mm_P1.5mm_ReverseMount", "D", "SK6812MINI-E", sx, sy, angle, bottom=True, pins=pins)
        if free(bbox(fp, 0.2), "bottom") and free(bbox(fp, 0.2), "top"):
            claim(fp, "bottom")
            occupied.append(bbox(fp, 0.2) + ("both",))  # the light's hole goes through
            placed = (fp, sx, sy)
            if kind != "pad" and sx == x:
                report["lights_as_dots"].append(cid)
            break
        board.Remove(fp)
        footprints.remove(fp)
        counters["D"] -= 1
    assert placed, f"no room for {cid}'s light"
    keep_out_of_hole(placed[0])
    led_at.append((placed[1], placed[2]))
    place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "100n", placed[1], placed[2] + 3.2,
                 pins={"1": "+5V", "2": "GND"}, radius=12)

# ── Bottom: the logic ────────────────────────────────────────────────────
# Each shift register sits at the middle of the controls it reads.
def where(cid):
    return panel[cid][1], panel[cid][2]


for k in range(CHIPS):
    members = [digital[i] for i in range(k * 8, min(k * 8 + 8, len(digital)))]
    spots = [where(cid) if cid in panel else pedal_at for cid, _ in members]
    cx = sum(p[0] for p in spots) / len(spots)
    cy = sum(p[1] for p in spots) / len(spots)
    pins = {"1": "SR_LOAD", "2": "SR_CLK", "8": "GND", "15": "GND", "16": "+3V3",
            "9": "SR_DATA" if k == 0 else f"CHAIN{k}", "10": f"CHAIN{k + 1}" if k < CHIPS - 1 else "GND"}
    # D0..D7 are pins 11, 12, 13, 14, 3, 4, 5, 6: input A..H.
    for bit, pin in enumerate(["11", "12", "13", "14", "3", "4", "5", "6"]):
        i = k * 8 + bit
        pins[pin] = inp(i) if i < len(digital) else "+3V3"
    u = place_bottom("Package_SO", "SOIC-16_3.9x9.9mm_P1.27mm", "U", "74HC165", cx, cy, pins=pins)
    ux, uy = u.GetPosition().x / MM, u.GetPosition().y / MM
    place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "100n", ux, uy - 7, pins={"1": "+3V3", "2": "GND"}, radius=15)
    # Pull-ups, four to an array: resistor r joins pad r+1 to pad 8-r.
    for half in range(2):
        first = k * 8 + half * 4
        if first >= len(digital):
            continue
        pins = {}
        for r in range(4):
            i = first + r
            pins[str(r + 1)] = inp(i) if i < len(digital) else "+3V3"
            pins[str(8 - r)] = "+3V3"
        place_bottom("Resistor_SMD", "R_Array_Convex_4x0603", "RN", "4x10k", ux + (6 if half else -6), uy, pins=pins, radius=20)

# Encoder A and B: 10 nF each to ground, beside the encoder.
for c in controls:
    if c["kind"] == "Encoder":
        x, y = where(c["id"])
        for role, dx in (("A", -9.5), ("B", 9.5)):
            place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "10n", x + dx, y + 6,
                         pins={"1": inp(input_of[(c["id"], role)]), "2": "GND"}, radius=12)

# Each pad's 10 kΩ to ground, beside its FSR's tabs.
for n in range(16):
    x, y = where(f"Pad({n})")
    place_bottom("Resistor_SMD", "R_0603_1608Metric", "R", "10k", x + 5, y + 10, pins={"1": f"PAD{n}", "2": "GND"}, radius=12)

# The two multiplexers. 4067: COM 1, I7..I0 on 2..9, S0 10, S1 11, GND 12,
# S3 13, S2 14, E 15, I15..I8 on 16..23, VCC 24.
def mux_pins(channel_net, com):
    pins = {"1": com, "10": "MUX_S0", "11": "MUX_S1", "12": "GND", "13": "MUX_S3", "14": "MUX_S2", "15": "GND", "24": "+3V3"}
    for ch in range(16):
        pin = 9 - ch if ch < 8 else 23 - (ch - 8)
        pins[str(pin)] = channel_net(ch)
    return pins


u13 = place_bottom("Package_SO", "SOIC-24W_7.5x15.4mm_P1.27mm", "U", "74HC4067", 159, 228,
                   pins=mux_pins(lambda ch: f"PAD{ch}", "PADS_ADC"))
place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "100n", u13.GetPosition().x / MM, u13.GetPosition().y / MM - 10,
             pins={"1": "+3V3", "2": "GND"}, radius=15)
u14 = place_bottom("Package_SO", "SOIC-24W_7.5x15.4mm_P1.27mm", "U", "74HC4067", 159, 165,
                   pins=mux_pins(lambda ch: f"FADER{ch}" if ch < 8 else ("EXPR" if ch == 8 else "GND"), "FADERS_ADC"))
place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "100n", u14.GetPosition().x / MM, u14.GetPosition().y / MM - 10,
             pins={"1": "+3V3", "2": "GND"}, radius=15)

# The expression pedal: 1 kΩ in the supply to its ring, 100 kΩ holding the
# tip down when nothing is plugged in.
place_bottom("Resistor_SMD", "R_0603_1608Metric", "R", "1k", pedal_at[0], pedal_at[1] + 6, pins={"1": "+3V3", "2": "EXPR_RING"}, radius=15)
place_bottom("Resistor_SMD", "R_0603_1608Metric", "R", "100k", pedal_at[0], pedal_at[1] + 9, pins={"1": "EXPR", "2": "GND"}, radius=15)

# The Pico 2, bottom side, castellated. Pins as on the schematics.
pico_pins = {"4": "SR_CLK", "6": "SR_DATA", "7": "SR_LOAD", "9": "MUX_S0", "10": "MUX_S1", "11": "MUX_S2",
             "12": "MUX_S3", "21": "LED_3V3", "31": "PADS_ADC", "32": "FADERS_ADC", "33": "GND", "36": "+3V3",
             "40": "+5V"}
for g in ["3", "8", "13", "18", "23", "28", "38"]:
    pico_pins[g] = "GND"
pico = place_bottom("Module", "RaspberryPi_Pico_SMD", "U", "Pico 2", 290, 180, pins=pico_pins, angles=(0, 90), radius=60)
px, py = pico.GetPosition().x / MM, pico.GetPosition().y / MM
place_bottom("Capacitor_SMD", "C_0805_2012Metric", "C", "10u", px - 14, py, pins={"1": "+3V3", "2": "GND"}, radius=20)

# The LED level shifter: GP16 through gate 1 of a 74AHCT125 at 5 V, then
# 330 Ω into the first LED. The other three gates are tied off.
place_bottom("Package_SO", "SOIC-14_3.9x8.7mm_P1.27mm", "U", "74AHCT125", led_at[0][0] + 10, led_at[0][1] + 8,
             pins={"1": "GND", "2": "LED_3V3", "3": "LED_BUF", "4": "+5V", "5": "GND", "7": "GND", "9": "GND",
                   "10": "+5V", "12": "GND", "13": "+5V", "14": "+5V"}, radius=30)
place_bottom("Capacitor_SMD", "C_0603_1608Metric", "C", "100n", led_at[0][0] + 10, led_at[0][1] + 15, pins={"1": "+5V", "2": "GND"}, radius=20)
place_bottom("Resistor_SMD", "R_0603_1608Metric", "R", "330", led_at[0][0] + 4, led_at[0][1] + 8, pins={"1": "LED_BUF", "2": "LED_DIN0"}, radius=20)
place_bottom("Capacitor_SMD", "CP_Elec_8x10", "C", "1000u 10V", led_at[0][0] + 20, led_at[0][1] + 12, pins={"1": "+5V", "2": "GND"}, radius=40)

# ── Outline ──────────────────────────────────────────────────────────────
x0, y0, x1, y1 = BOARD
for a, b in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))]:
    seg = pcbnew.PCB_SHAPE(board)
    seg.SetShape(pcbnew.SHAPE_T_SEGMENT)
    seg.SetStart(vec(*a))
    seg.SetEnd(vec(*b))
    seg.SetLayer(pcbnew.Edge_Cuts)
    seg.SetWidth(mm(0.1))
    board.Add(seg)

# Text on the bottom: what the board is.
label = pcbnew.PCB_TEXT(board)
label.SetText("PHOSPHOR DECK  controller  rev A")
label.SetPosition(vec(170, 263))
label.SetLayer(pcbnew.B_SilkS)
label.SetMirrored(True)
label.SetTextSize(pcbnew.VECTOR2I(mm(2.5), mm(2.5)))
board.Add(label)

# ── Rules ────────────────────────────────────────────────────────────────
ds = board.GetDesignSettings()
ds.SetCopperLayerCount(2)
classes = ds.m_NetSettings
default = classes.GetDefaultNetclass()
default.SetTrackWidth(mm(0.25))
default.SetClearance(mm(0.2))
default.SetViaDiameter(mm(0.6))
default.SetViaDrill(mm(0.3))
# The reverse-mount LEDs' pads sit 0.35 mm from the hole they shine
# through, as KiCad's own footprint draws them; board houses take 0.3.
ds.m_CopperEdgeClearance = mm(0.3)
# The autorouter narrows a track to enter a small pad (a resistor array's,
# the Pico's); 0.15 mm is well inside what board houses make.
ds.m_TrackMinWidth = mm(0.15)

pcbnew.SaveBoard(OUT, board)

unused = [f.GetReference() for f in footprints for p in f.Pads() if p.GetNetname() == "" and p.GetNumber() not in ("", "MP")]
summary = {
    "footprints": len(footprints),
    "top": sum(1 for f in footprints if not f.IsFlipped()),
    "bottom": sum(1 for f in footprints if f.IsFlipped()),
    "nets": len(nets),
    "lights_as_dots": report["lights_as_dots"],
    "moved_far": report["moved"],
}
json.dump(summary, open(os.path.join(HERE, "placement.json"), "w"), indent=1)
print(json.dumps(summary, indent=1))
