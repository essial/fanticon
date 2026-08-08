BITMAP BANKS

Fills the packed 320x200 bitmap through
VRAM banks 1 and 2. Startup takes a few
frames as the 6502 writes 32,000 bytes.

MAIN.ASM uses a named macro with a default argument,
a private @LOOP label, and compile-time IF to share
the two bank-fill loops without hiding their 6502 code.
