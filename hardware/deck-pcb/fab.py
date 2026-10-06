"""The parts list for the routed board, grouped by part, with the
manufacturer part to buy where one is certain; and the same machine-placed
parts again in the two files JLCPCB's assembly service reads.

    fab.py   deck.kicad_pcb -> fab/bom.csv, fab/jlc-bom.csv, fab/jlc-cpl.csv
"""
import collections
import csv
import os

import pcbnew

HERE = os.path.dirname(os.path.abspath(__file__))
board = pcbnew.LoadBoard(os.path.join(HERE, "deck.kicad_pcb"))

# (value, footprint) -> what to buy. Blank where the choice is the
# builder's: a knob's feel, a switch's height under the caps chosen.
PARTS = {
    ("74HC165", "SOIC-16_3.9x9.9mm_P1.27mm"): ("Texas Instruments", "SN74HC165DR"),
    ("74HC4067", "SOIC-24W_7.5x15.4mm_P1.27mm"): ("Texas Instruments", "CD74HC4067M96"),
    ("74AHCT125", "SOIC-14_3.9x8.7mm_P1.27mm"): ("Texas Instruments", "SN74AHCT125DR"),
    ("SK6812MINI-E", "LED_SK6812MINI-E_3.2x2.8mm_P1.5mm_ReverseMount"): ("Opsco", "SK6812MINI-E"),
    ("4x10k", "R_Array_Convex_4x0603"): ("Yageo", "YC164-JR-0710KL"),
    ("10k", "R_0603_1608Metric"): ("Yageo", "RC0603FR-0710KL"),
    ("100k", "R_0603_1608Metric"): ("Yageo", "RC0603FR-07100KL"),
    ("1k", "R_0603_1608Metric"): ("Yageo", "RC0603FR-071KL"),
    ("330", "R_0603_1608Metric"): ("Yageo", "RC0603FR-07330RL"),
    ("100n", "C_0603_1608Metric"): ("Samsung", "CL10B104KB8NNNC"),
    ("10n", "C_0603_1608Metric"): ("Samsung", "CL10B103KB8NNNC"),
    ("10u", "C_0805_2012Metric"): ("Samsung", "CL21A106KAYNNNE"),
    ("Pico 2", "RaspberryPi_Pico_SMD"): ("Raspberry Pi", "Pico 2 (RP2350), soldered by its castellations"),
}

# The same parts by their LCSC numbers, which is how JLCPCB's assembly
# service picks a part off its shelves. Where JLC keeps a "Basic" part with
# the same rating, the Basic one is named: Basic parts sit on the machines
# already, so they cost no loading fee in an economic order. Each swap is
# the same value, tolerance, voltage, dielectric or power, and size as the
# part in PARTS; nothing here is a different part electrically.
LCSC = {
    # YAGEO CC0603KRX7R9BB104: 100 nF, 50 V, X7R, 10 %, 0603, as the Samsung.
    ("100n", "C_0603_1608Metric"): "C14663",
    # FH 0603B103K500NT: 10 nF, 50 V, X7R, 10 %, 0603, as the Samsung.
    ("10n", "C_0603_1608Metric"): "C57112",
    # The Samsung part itself; JLC keeps it as Basic.
    ("10u", "C_0805_2012Metric"): "C15850",
    # UNI-ROYAL 0603WAF: 1 %, 100 mW, 75 V, 100 ppm, 0603, as the Yageo RC0603FR.
    ("10k", "R_0603_1608Metric"): "C25804",
    ("1k", "R_0603_1608Metric"): "C21190",
    ("100k", "R_0603_1608Metric"): "C25803",
    ("330", "R_0603_1608Metric"): "C23138",
    # UNI-ROYAL 4D03WGJ0103T5E: four 10k, 5 %, 62.5 mW, 3.2 x 1.6 mm, 0.8 mm
    # pitch, convex ends, as the Yageo YC164, so it fits the convex footprint.
    ("4x10k", "R_Array_Convex_4x0603"): "C29718",
    # Panasonic EEE-FT1A102AP, 1000 uF 10 V, 8 x 10 mm. No Basic part exists.
    ("1000u 10V", "CP_Elec_8x10"): "C278408",
    # The rest are the exact parts in PARTS; JLC has them only as Extended.
    ("SK6812MINI-E", "LED_SK6812MINI-E_3.2x2.8mm_P1.5mm_ReverseMount"): "C5149201",
    ("74HC165", "SOIC-16_3.9x9.9mm_P1.27mm"): "C61060",
    ("74HC4067", "SOIC-24W_7.5x15.4mm_P1.27mm"): "C496123",
    ("74AHCT125", "SOIC-14_3.9x8.7mm_P1.27mm"): "C155176",
    # JLC stocks the module and places it by its castellations. Mark it "do
    # not place" on their parts page to solder one by hand instead.
    ("Pico 2", "RaspberryPi_Pico_SMD"): "C41407547",
}

# JLC's drawing of a part does not always face the way KiCad's does. KiCad
# draws a SOIC with its pins down the left and right sides, pin 1 top left;
# JLC draws it lying on its side, pins along the top and bottom, pin 1
# bottom left: a quarter turn apart. These are the turns that put each JLC
# part's pin 1 on KiCad's pin 1, worked out from both footprints' pads. A
# footprint missing here faces the same way in both.
JLC_TURN = {
    "SOIC-14_3.9x8.7mm_P1.27mm": 270,
    "SOIC-16_3.9x9.9mm_P1.27mm": 270,
    "SOIC-24W_7.5x15.4mm_P1.27mm": 270,
    "R_Array_Convex_4x0603": 270,
    "RaspberryPi_Pico_SMD": 270,
}
DESCRIBE = {
    "SW_PUSH_6mm": "6 x 6 mm through-hole tactile switch (Omron B3F-1000 family; pick the height that meets the caps)",
    "RotaryEncoder_Alps_EC11E-Switch_Vertical_H20mm": "Alps EC11E-series encoder with push switch, 20 detents, 20 mm shaft",
    "Potentiometer_Bourns_PTA4543_Single_Slide": "Bourns PTA4543-2015DPB103 slide fader, 45 mm, 10 kOhm linear",
    "PinHeader_1x02_P2.54mm_Vertical": "Interlink FSR 402 Short: its two solder tabs go through these holes",
    "PinHeader_1x04_P2.54mm_Vertical": "1 x 4 pin header, 2.54 mm, to the pedal jacks",
    "CP_Elec_8x10": "1000 uF 10 V aluminium electrolytic, 8 x 10 mm SMD",
    "MountingHole_3.2mm_M3": "M3 mounting hole (no part)",
}

groups = collections.OrderedDict()
for fp in sorted(board.GetFootprints(), key=lambda f: (f.IsFlipped(), f.GetFPIDAsString(), f.GetReference())):
    name = fp.GetFPID().GetLibItemName().wx_str()
    # A control's value is its panel label (PLAY, KNOB 3); the part is
    # the same under every label, so controls group by footprint alone.
    key = (fp.GetValue() if fp.IsFlipped() else "", name)
    groups.setdefault(key, []).append(fp)

os.makedirs(os.path.join(HERE, "fab"), exist_ok=True)
with open(os.path.join(HERE, "fab", "bom.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Qty", "References", "Value", "Footprint", "Side", "Manufacturer", "Part", "Notes"])
    for (value, name), fps in groups.items():
        if name.startswith("MountingHole"):
            continue
        maker, part = PARTS.get((value, name), ("", ""))
        side = "bottom (machine)" if fps[0].IsFlipped() else "top (hand)"
        refs = " ".join(sorted((p.GetReference() for p in fps), key=lambda r: (len(r), r)))
        w.writerow([len(fps), refs, value, name, side, maker, part, DESCRIBE.get(name, "")])
print("wrote fab/bom.csv:", sum(1 for _ in groups), "lines")

# JLCPCB's two files, for the parts their machines place: everything on
# the bottom. A machine-placed part with no LCSC number would quietly be
# left off the board, so that stops the script instead.
machine = [(key, fps) for key, fps in groups.items() if fps[0].IsFlipped()]
missing = [key for key, _ in machine if key not in LCSC]
if missing:
    raise SystemExit(f"no LCSC number for {missing}: add it to LCSC in fab.py")


def by_ref(fps):
    return sorted(fps, key=lambda p: (len(p.GetReference()), p.GetReference()))


with open(os.path.join(HERE, "fab", "jlc-bom.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
    for (value, name), fps in machine:
        w.writerow([value, ",".join(p.GetReference() for p in by_ref(fps)), name, LCSC[(value, name)]])

# Positions in millimetres from the same origin as the gerbers, with y
# flipped because KiCad counts y downwards and gerbers count it upwards;
# bottom parts keep their x as seen from the top, which is what JLC wants.
# JLC reads a bottom part's rotation as seen from underneath, looking at the
# part: from there KiCad's anticlockwise turn runs the other way and starts
# half a turn round, hence 180 minus KiCad's angle, then JLC's own turn.
with open(os.path.join(HERE, "fab", "jlc-cpl.csv"), "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
    for (value, name), fps in machine:
        for p in by_ref(fps):
            at = p.GetPosition()
            turn = (180 - p.GetOrientationDegrees() + JLC_TURN.get(name, 0)) % 360
            w.writerow([p.GetReference(), f"{at.x / 1e6:.4f}", f"{-at.y / 1e6:.4f}", "Bottom", f"{turn:g}"])
print("wrote fab/jlc-bom.csv and fab/jlc-cpl.csv:", sum(len(fps) for _, fps in machine), "parts")
