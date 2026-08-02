RASTER WAVE

Uses one raster IRQ per visible scanline to bend
horizontal tilemap scroll along a smooth 128-step sine
curve. Vertical scroll follows the same curve once per
frame with a quarter-cycle offset. This keeps both axes
moving without folding at scanline boundaries.

The raster comparator fires before HBlank. IRQ entry and
table lookup consume enough cycles that SCROLL_X is
written during HBlank, before the following line begins.
This demonstrates timing an effect around real 6502
cycle costs.

From this folder, enter RUN to build and launch the
cartridge. There are no controls; press Escape to return
to the editor or command prompt.
