# Phosphor Deck controller board

The circuit board for the Phosphor Deck: every knob, button, fader and pad on
top, and the electronics that read them underneath. It is the board drawn on
the schematics, and it runs the firmware in [`../../firmware`](../../firmware).

**Rev A has not been made yet.** It passes KiCad's design-rule check with no
errors and no unconnected pins, but nobody has held one. Build the breadboard
test in the firmware's README before ordering.

## What to send a board house

Everything is in `fab/`:

- `gerbers.zip` — the board itself: two copper layers, 296 × 234 mm, 1.6 mm.
- `bom.csv` — every part, grouped, with the part to buy where there is one.
- `positions-bottom.csv` — where each surface-mount part goes, for a house
  that assembles the underside.
- `jlc-bom.csv` and `jlc-cpl.csv` — the same parts and places in the exact
  form JLCPCB asks for, with their LCSC part numbers.

The board is laid out so that a board house only has to assemble one side:
every surface-mount part (chips, resistors, capacitors, LEDs, the Pico 2) is
on the bottom. Everything on top is through-hole and soldered by hand: 61
switches, 10 knobs, 8 faders, the 16 pad sensors' tabs and the pedal header.

## Ordering from JLCPCB, step by step

JLCPCB makes the board and solders every part on the underside for you. You
then solder the top parts by hand.

1. **Upload the board.** On jlcpcb.com, choose *Order now* and add
   `fab/gerbers.zip`. It reads the size (296 × 234 mm) and the two layers by
   itself.
2. **Choose the board options.** Layers 2, thickness 1.6 mm, FR-4. Five
   boards is the smallest order. Green is the cheapest colour; any colour
   works. For the surface finish, lead-free HASL is fine; ENIG costs more
   and gives flatter pads. Leave the rest as it is.
3. **Turn on PCB Assembly.** Choose:
   - *PCBA type: Standard.* Economic is cheaper, but it is meant for one
     side, usually the top, and JLC sells the Pico 2 for Standard only. If
     Economic offers *Bottom Side* when you order and you will solder the
     Pico yourself, Economic works too.
   - *Assembly side: Bottom Side.* Every machine part is underneath.
   - *Assembly quantity:* 2 or more; it can be fewer than the boards.
   - Let JLC add the edge rails, fiducials and tooling holes if it asks.
     Standard assembly needs them and the board has none of its own.
   - Tick *Confirm parts placement*: for a small fee, one of their engineers
     checks the placements and asks you before building.
4. **Upload the two parts files**: `fab/jlc-bom.csv` as the BOM and
   `fab/jlc-cpl.csv` as the CPL (the placement file). Every line should
   match a part. If one is out of stock, pick a part with the same value,
   rating and size, or wait.
5. **Check the placement preview.** It shows each part on the board, seen
   from underneath. The files already turn each part the way JLC's drawings
   need, but look at each kind once:
   - the chips (U1–U16): the dot that marks pin 1 sits on the board's pin-1
     mark;
   - the LEDs (D1–D42): the LED's cut corner matches its outline, and the
     body sits over the light hole;
   - the big capacitor (C79): its + side sits on the + pad;
   - the Pico 2 (U15): its USB socket is at the same end as on the board
     drawing.

   The small resistors and capacitors can sit either way round. If a part
   is wrong, select it and turn it in the preview, or change its *Rotation*
   in `jlc-cpl.csv` and upload it again.
6. **Solder the top by hand** when the boards arrive: the 61 switches, 10
   knobs, 8 faders, the 16 pad sensors' tabs and the pedal header. If you
   left the Pico 2 off the order (mark it *do not place* on the parts page),
   solder it underneath by its edge pads.

The board is larger than 650 cm², so JLC adds a large-board fee to the
assembly. Basic parts need no loading fee in an Economic order; in a
Standard order every kind of part has a small one.

## Things to know before ordering

- **The LED light holes.** The LEDs sit underneath and shine up through small
  holes. KiCad's own footprint for them puts their pads 0.25 mm from the
  hole's corners; the rest of the board keeps 0.3 mm. Most board houses accept
  this for internal cutouts — ask yours.
- **The buttons are 6 × 6 mm tactile switches** under printed caps, because
  the panel's buttons sit closer together than keyboard switches allow. Pick
  the switch height that meets your caps through the top plate.
- **The faders are centred at 167 mm**, 2 mm lower than the first panel
  drawing: a PTA4543's body is 60 mm long. The top plate's slots run from
  143 to 191 mm.
- **Lights on narrow buttons are dots.** The transport and function buttons
  are too narrow for a light inside the cap, so theirs sits just above as a
  dot in the top plate.
- **The Pico 2 is soldered flat by its edges** on the underside. Its USB
  socket points along the board, so use a right-angle micro-USB cable to the
  Raspberry Pi.

## Making it again

The board is generated, not drawn: `generate.py` places every part from the
panel drawing and wires it from `firmware/deck-layout.json`, Phosphor's own
control table, so the board, the firmware and the app share one table.
`route.sh` runs the lot — placement, routing with Freerouting, ground pours,
the design-rule check, and the files in `fab/`:

```
KICAD=/Applications/KiCad/KiCad.app \
FREEROUTING=freerouting-2.4.1.jar \
JAVA=<a Java 25 runtime>/bin/java \
./route.sh
```

It needs KiCad 10 and Freerouting 2.4, which needs Java 25. Routing takes
about five minutes a try. The router's result varies a little from run to
run and now and then leaves a connection unrouted, so the script tries up to
three times and writes `fab/` only from a board with no errors and nothing
unconnected; `drc.rpt` is the check that board passed. A ground fill covers
both sides, tied together by stitching vias wherever one fits.
