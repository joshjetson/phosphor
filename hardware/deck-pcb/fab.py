"""The parts list for the routed board, grouped by part, with the
manufacturer part to buy where one is certain.

    fab.py   deck.kicad_pcb -> fab/bom.csv
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
