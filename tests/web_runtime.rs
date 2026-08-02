#![cfg(target_arch = "wasm32")]

use fanticon::{
    assembler::assemble_cartridge_with_loader,
    cartridge::{Cartridge, SaveImage},
    machine::BANK_SIZE,
    system::{ControllerState, FanticonMachine},
    video::Video,
};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn browser_executes_cartridge_video_audio_and_input() {
    let source = r#"
         FIXED
         ORG   $C100
RESET    SEI
         LDA   #$E0
         STA   $C012
         LDA   #$CC
         STA   $C030
         LDA   #$20
         STA   $C031
         LDA   #0
         STA   $C032
         STA   $C033
         LDA   #$8F
         STA   $C040
         LDA   $C050
         STA   $10
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI
         ORG   $FFFA
         DA    NMI,RESET,IRQ
"#;
    let assembled = assemble_cartridge_with_loader("web.asm", source, |_| {
        Err("includes are unavailable".to_owned())
    })
    .unwrap();
    let cartridge =
        Cartridge::new("WEB SMOKE", 0x5745_4253_4d4f_4b45, 0, assembled.fixed_rom, Vec::new())
            .unwrap();
    let mut machine = FanticonMachine::new(cartridge, None);
    machine.bus.set_controller(0, ControllerState(ControllerState::A));
    machine.run_frame();

    assert_eq!(machine.bus.peek(0x10), ControllerState::A);
    assert!(machine.bus.audio_frame().iter().any(|&sample| sample != 0));
    let mut video = Video::new();
    machine.bus.present(&mut video);
    assert!(video.pixels().iter().filter(|&&pixel| pixel == 0xe0).count() > 60_000);
}

#[wasm_bindgen_test]
fn browser_storage_round_trips_a_real_save_image() {
    let storage = web_sys::window().unwrap().local_storage().unwrap().unwrap();
    let key = "fanticon-web-runtime-smoke";
    let save = SaveImage { cartridge_id: 7, ram: vec![0x5a; BANK_SIZE] }.to_bytes().unwrap();
    let encoded = save.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

    storage.set_item(key, &encoded).unwrap();
    let restored = storage.get_item(key).unwrap().unwrap();
    storage.remove_item(key).unwrap();
    let bytes = restored
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(SaveImage::from_bytes(&bytes).unwrap().ram, vec![0x5a; BANK_SIZE]);
}
