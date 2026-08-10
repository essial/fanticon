use std::collections::BTreeSet;

use fanticon::{project::build_project_with_loader, system::FanticonMachine};

/// `SEED_RANDOM`/`NEXT_RANDOM` in `FANTICON.INC` are an 8-bit Galois LFSR
/// (tap `$1D`). That tap is only useful if it actually has the maximal
/// 255-value period; this drives the real macros through the real 6502
/// core for 256 steps and checks the sequence never repeats early and
/// never gets stuck at zero, rather than trusting the polynomial by
/// reputation.
#[test]
fn next_random_visits_every_nonzero_byte_exactly_once_per_period() {
    let manifest = "TITLE=RANDOM TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=TEST.FCN\nSAVE_BANKS=0\nMACHINE=1.0\n";
    let source = r#"
         INCLUDE FANTICON.INC
         FIXED
         ORG   $C100
RESET
         PMC   SEED_RANDOM;$20
         LDX   #0
LOOP
         PMC   NEXT_RANDOM;$20
         STA   $0300,X
         INX
         BNE   LOOP
HALT     JMP   HALT
NMI      RTI
IRQ      RTI
         ORG   VECTOR_NMI
         DA    NMI,RESET,IRQ
"#;
    let build = build_project_with_loader(manifest, |path| {
        path.eq_ignore_ascii_case("main.asm")
            .then(|| source.to_owned())
            .ok_or_else(|| "not found".to_owned())
    })
    .unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));

    let mut machine = FanticonMachine::new(build.cartridge, None);
    machine.run_frame();

    let sequence: Vec<u8> = (0..256).map(|offset| machine.bus.peek(0x0300 + offset)).collect();
    assert!(!sequence[..255].contains(&0), "LFSR must never produce zero: {sequence:?}");
    let distinct: BTreeSet<u8> = sequence[..255].iter().copied().collect();
    assert_eq!(distinct.len(), 255, "255 steps must visit all 255 nonzero bytes exactly once");
    assert_eq!(sequence[255], sequence[0], "step 256 must close the period back to step 1's value");
}
