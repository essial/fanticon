TILE SCROLLER

Builds a 64x32 tilemap in VRAM and uses
a VBlank IRQ to read controller 1.

Arrow keys scroll in every direction.
The 16-bit scroll registers wrap around
the 320x200 map.

MAIN.ASM uses REPEAT blocks to generate the eight
tile-map and eight attribute-page stores.
