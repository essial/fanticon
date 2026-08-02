GRAPHICS ASSET DEMO

Open GAME.PAL to edit the shared 256-color
palette. Open SCENE.GFX to edit its patterns,
64x32 circular map, attributes, and 16x16 sprite
art. Map view shows a pannable 40x25 window.

MAIN.ASM demonstrates the complete runtime path:

  1. PUT the PAL and GFX resources in ROM.
  2. Upload GAME_PAL through PALDATA.
  3. Stage GFX pages through normal RAM.
  4. Copy patterns, map, and attributes to VRAM.
  5. Create a sprite from patterns 4 through 7.

From this folder, use RUN or press F5 in the
editor. Use the arrow keys to scroll the map.
