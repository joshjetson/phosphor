"""The routing steps that need KiCad's own Python.

    route.py export   deck.kicad_pcb -> deck.dsn, for Freerouting
    route.py import   deck.ses -> deck.kicad_pcb, then ground pours both sides
"""
import os
import sys

import pcbnew

HERE = os.path.dirname(os.path.abspath(__file__))
PCB = os.path.join(HERE, "deck.kicad_pcb")
MM = 1_000_000


def ground_pours(board):
    """A ground fill over the whole board on both sides: the return path
    for every signal, and the LEDs' heat spread."""
    gnd = board.FindNet("GND")
    outline = board.GetBoardEdgesBoundingBox()
    for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
        zone = pcbnew.ZONE(board)
        zone.SetLayer(layer)
        zone.SetNet(gnd)
        zone.SetLocalClearance(int(0.3 * MM))
        zone.SetMinThickness(int(0.25 * MM))
        # Solid to every pad. Thermal spokes into a pour that routing has
        # boxed into a scrap leave a pin hanging on a sliver, and the board
        # is thin enough to solder a solid pin with an ordinary iron.
        zone.SetPadConnection(pcbnew.ZONE_CONNECTION_FULL)
        zone.SetIsFilled(False)
        poly = zone.Outline()
        poly.NewOutline()
        for x, y in ((outline.GetLeft(), outline.GetTop()), (outline.GetRight(), outline.GetTop()),
                     (outline.GetRight(), outline.GetBottom()), (outline.GetLeft(), outline.GetBottom())):
            poly.Append(x, y)
        board.Add(zone)
    stitch(board, gnd)
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())


def stitch(board, gnd, pitch=8.0, size=0.6, drill=0.3, gap=0.35):
    """Ground vias on a grid wherever one fits, tying the two pours
    together so no scrap of either is left floating."""
    box = board.GetBoardEdgesBoundingBox()
    r = size / 2 * MM
    keep = int(gap * MM + r)
    obstacles = []  # (kind, geometry)
    for t in board.GetTracks():
        if t.GetClass() == "PCB_VIA":
            obstacles.append(("dot", t.GetPosition().x, t.GetPosition().y, t.GetWidth(pcbnew.F_Cu) / 2))
        else:
            obstacles.append(("seg", t.GetStart(), t.GetEnd(), t.GetWidth() / 2))
    for fp in board.GetFootprints():
        for p in fp.Pads():
            b = p.GetBoundingBox()
            obstacles.append(("box", b.GetLeft(), b.GetTop(), b.GetRight(), b.GetBottom()))
        for g in fp.GraphicalItems():
            if g.GetLayer() == pcbnew.Edge_Cuts:
                b = g.GetBoundingBox()
                obstacles.append(("box", b.GetLeft() - MM, b.GetTop() - MM, b.GetRight() + MM, b.GetBottom() + MM))
    for z in board.Zones():
        if z.GetIsRuleArea():
            b = z.GetBoundingBox()
            obstacles.append(("box", b.GetLeft(), b.GetTop(), b.GetRight(), b.GetBottom()))

    def clear(x, y):
        for o in obstacles:
            if o[0] == "dot":
                if (o[1] - x) ** 2 + (o[2] - y) ** 2 < (o[3] + keep) ** 2:
                    return False
            elif o[0] == "box":
                if o[1] - keep < x < o[3] + keep and o[2] - keep < y < o[4] + keep:
                    return False
            else:
                ax, ay, bx, by = o[1].x, o[1].y, o[2].x, o[2].y
                dx, dy = bx - ax, by - ay
                length = dx * dx + dy * dy
                t = 0.0 if length == 0 else max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / length))
                px, py = ax + t * dx, ay + t * dy
                if (px - x) ** 2 + (py - y) ** 2 < (o[3] + keep) ** 2:
                    return False
        return True

    margin = int(3 * MM)
    step = int(pitch * MM)
    added = 0
    y = box.GetTop() + margin
    while y < box.GetBottom() - margin:
        x = box.GetLeft() + margin
        while x < box.GetRight() - margin:
            if clear(x, y):
                via = pcbnew.PCB_VIA(board)
                via.SetPosition(pcbnew.VECTOR2I(x, y))
                via.SetWidth(int(size * MM))
                via.SetDrill(int(drill * MM))
                via.SetNet(gnd)
                board.Add(via)
                obstacles.append(("dot", x, y, r))
                added += 1
            x += step
        y += step
    print(f"{added} stitching vias")


step = sys.argv[1]
board = pcbnew.LoadBoard(PCB)
if step == "export":
    ok = pcbnew.ExportSpecctraDSN(board, os.path.join(HERE, "deck.dsn"))
    print("exported" if ok else "export failed")
elif step == "import":
    ok = pcbnew.ImportSpecctraSES(board, os.path.join(HERE, "deck.ses"))
    print("imported" if ok else "import failed")
    ground_pours(board)
    pcbnew.SaveBoard(PCB, board)
    tracks = [t for t in board.GetTracks() if t.GetClass() == "PCB_TRACK"]
    vias = [t for t in board.GetTracks() if t.GetClass() == "PCB_VIA"]
    print(f"{len(tracks)} track segments, {len(vias)} vias")
