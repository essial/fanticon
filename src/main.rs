mod host;

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fanticon::{
    system::{ControllerState, FanticonMachine},
    video::Video,
};
use host::{
    AppMode, AudioOutput, BootSplash, EditorAction, FramePacer, FrameStatus, Renderer, Terminal,
    TerminalAction, TextEditor, draw_boot_logo,
};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

enum UserEvent {
    RendererReady(Result<Renderer, String>),
}

struct FanticonApp {
    event_proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    video: Video,
    frame_pacer: FramePacer,
    frame_number: u64,
    boot_splash: BootSplash,
    terminal: Terminal,
    text_editor: Option<TextEditor>,
    modifiers: ModifiersState,
    game: Option<GameSession>,
    audio_output: Option<AudioOutput>,
    mouse_position: Option<PhysicalPosition<f64>>,
}

struct GameSession {
    machine: FanticonMachine,
    save_backing: SaveBacking,
    last_save_generation: u64,
    last_save_write: Option<Instant>,
    launched_from_editor: bool,
    #[cfg(not(target_arch = "wasm32"))]
    _save_lock: Option<std::fs::File>,
}

enum SaveBacking {
    None,
    Console(String),
    #[cfg(not(target_arch = "wasm32"))]
    Native(PathBuf),
}

#[cfg(not(target_arch = "wasm32"))]
struct DirectGameLaunch {
    cartridge: fanticon::cartridge::Cartridge,
    save_path: Option<PathBuf>,
    save_ram: Vec<u8>,
}

impl FanticonApp {
    fn new(event_proxy: winit::event_loop::EventLoopProxy<UserEvent>, mode: AppMode) -> Self {
        let mut video = Video::new();
        draw_boot_logo(&mut video);
        let now = Instant::now();
        Self {
            event_proxy,
            window: None,
            renderer: None,
            video,
            frame_pacer: FramePacer::new(now),
            frame_number: 0,
            boot_splash: BootSplash::new(now),
            terminal: Terminal::new(mode),
            text_editor: None,
            modifiers: ModifiersState::empty(),
            game: None,
            audio_output: AudioOutput::new()
                .map_err(|error| eprintln!("Fanticon audio disabled: {error}"))
                .ok(),
            mouse_position: None,
        }
    }
}

impl ApplicationHandler<UserEvent> for FanticonApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Fanticon")
            .with_inner_size(LogicalSize::new(960, 600))
            .with_min_inner_size(LogicalSize::new(320, 200));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("could not create Fanticon window: {error}");
                event_loop.exit();
                return;
            }
        };

        #[cfg(target_arch = "wasm32")]
        attach_canvas(&window);

        self.window = Some(Arc::clone(&window));
        let proxy = self.event_proxy.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = pollster::block_on(Renderer::new(window));
            let _ = proxy.send_event(UserEvent::RendererReady(result));
        }

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(async move {
            let result = Renderer::new(window).await;
            let _ = proxy.send_event(UserEvent::RendererReady(result));
        });
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RendererReady(Ok(renderer)) => {
                self.renderer = Some(renderer);
                let now = Instant::now();
                self.frame_pacer.reset(now);
                self.boot_splash.reset(now);
            }
            UserEvent::RendererReady(Err(error)) => {
                eprintln!("Fanticon renderer initialization failed: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref().cloned() else { return };
        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                self.flush_game_save();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let now = Instant::now();
                if self.boot_splash.is_active(now) {
                    if event.state == ElementState::Pressed {
                        self.boot_splash.try_dismiss(now);
                    }
                } else if self.game.is_some() {
                    self.handle_game_key(event.state, &event.logical_key, event.physical_key);
                } else if should_process_keyboard_input(event.state, event.repeat) {
                    self.handle_key(&event.logical_key, event.physical_key);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position = Some(position);
                if !self.boot_splash.is_active(Instant::now())
                    && self.game.is_none()
                    && let Some((x, y)) = window_to_source_position(
                        position,
                        window.inner_size(),
                        (host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT),
                    )
                    && let Some(editor) = &mut self.text_editor
                {
                    editor.handle_mouse_move(x, y);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let now = Instant::now();
                if state == ElementState::Pressed && self.boot_splash.is_active(now) {
                    self.boot_splash.try_dismiss(now);
                    return;
                }
                if button != MouseButton::Left || self.game.is_some() {
                    return;
                }
                if state == ElementState::Released {
                    if let Some(editor) = &mut self.text_editor {
                        editor.handle_mouse_release();
                    }
                    return;
                }
                let Some(position) = self.mouse_position else { return };
                let Some((x, y)) = window_to_source_position(
                    position,
                    window.inner_size(),
                    (host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT),
                ) else {
                    return;
                };
                if let Some(editor) = &mut self.text_editor {
                    let action = editor.handle_mouse_press(x, y, self.modifiers.shift_key());
                    self.apply_editor_action(action);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.boot_splash.is_active(Instant::now()) || self.game.is_some() {
                    return;
                }
                let (mut horizontal, mut vertical) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (f64::from(x), f64::from(y)),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x / 24.0, position.y / 24.0)
                    }
                };
                if self.modifiers.shift_key() && horizontal == 0.0 {
                    horizontal = vertical;
                    vertical = 0.0;
                }
                if let Some(editor) = &mut self.text_editor {
                    editor.handle_mouse_wheel(horizontal, vertical);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    let editor_presentation = !self.boot_splash.is_active(Instant::now())
                        && self.video.dimensions()
                            == (host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT);
                    match renderer.render(&mut self.video, editor_presentation) {
                        FrameStatus::Presented | FrameStatus::Skip => {}
                        FrameStatus::Reconfigure => renderer.resize(window.inner_size()),
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.as_ref().cloned() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if self.renderer.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }

        let now = Instant::now();
        if self.frame_pacer.is_due(now) {
            self.emulate_frame(now);
            window.request_redraw();
            self.frame_pacer.advance_after_frame(now);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.frame_pacer.next_deadline()));
    }
}

impl FanticonApp {
    fn emulate_frame(&mut self, now: Instant) {
        if self.boot_splash.is_active(now) {
            self.video.begin_frame();
            return;
        }

        if let Some(game) = &mut self.game {
            if self.video.dimensions()
                != (fanticon::video::DISPLAY_WIDTH, fanticon::video::DISPLAY_HEIGHT)
            {
                self.video = Video::new();
            }
            game.machine.run_frame();
            if let Some(audio) = &self.audio_output {
                audio.submit(game.machine.bus.audio_frame());
            }
            game.machine.bus.present(&mut self.video);
            self.video.begin_frame();
            let generation = game.machine.bus.save_generation();
            if generation != game.last_save_generation {
                game.last_save_generation = generation;
                game.last_save_write = Some(now);
            }
            if game
                .last_save_write
                .is_some_and(|write| now.duration_since(write).as_secs_f32() >= 1.0)
            {
                self.flush_game_save();
            }
            self.frame_number = self.frame_number.wrapping_add(1);
            return;
        }

        let dimensions = if self.text_editor.is_some() {
            (host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT)
        } else {
            self.terminal.display_dimensions()
        };
        if self.video.dimensions() != dimensions {
            self.video = Video::new_with_size(dimensions.0, dimensions.1);
        }

        let cursor_visible = (self.frame_number / 30).is_multiple_of(2);
        if let Some(editor) = &mut self.text_editor {
            let action = editor.update();
            if let EditorAction::Run(launch) = action {
                self.start_game(launch, true);
                return;
            }
            editor.render(&mut self.video, cursor_visible);
        } else {
            self.terminal.render(&mut self.video, cursor_visible);
        }
        self.video.begin_frame();
        self.frame_number = self.frame_number.wrapping_add(1);
    }

    fn handle_key(&mut self, key: &Key, physical_key: winit::keyboard::PhysicalKey) {
        if let Some(editor) = &mut self.text_editor {
            let action = editor.handle_key(key, physical_key, self.modifiers);
            self.apply_editor_action(action);
            return;
        }

        let action = dispatch_terminal_key(&mut self.terminal, key);
        match action {
            TerminalAction::SwitchMode(mode) => self.terminal.switch_mode(mode),
            TerminalAction::Edit(filename) => {
                self.text_editor = Some(TextEditor::new(
                    self.terminal.filesystem(),
                    self.terminal.colors(),
                    filename,
                ));
            }
            TerminalAction::Run(launch) => self.start_game(launch, true),
            TerminalAction::None => {}
        }
    }

    fn apply_editor_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::Exit => self.text_editor = None,
            EditorAction::Run(launch) => self.start_game(launch, true),
            EditorAction::None => {}
        }
    }

    fn start_game(&mut self, launch: host::GameLaunch, launched_from_editor: bool) {
        #[cfg(target_arch = "wasm32")]
        let machine = FanticonMachine::new(launch.cartridge, Some(launch.save_ram));
        #[cfg(not(target_arch = "wasm32"))]
        let mut machine = FanticonMachine::new(launch.cartridge, Some(launch.save_ram));
        #[cfg(not(target_arch = "wasm32"))]
        let save_lock = launch.save_path.as_ref().and_then(|path| {
            match self.terminal.filesystem().borrow().acquire_save_lock(path) {
                Ok(Some(lock)) => Some(lock),
                Ok(None) => {
                    machine.bus.set_save_writable(false);
                    eprintln!("Fanticon warning: save is already open; battery RAM is read-only");
                    None
                }
                Err(error) => {
                    machine.bus.set_save_writable(false);
                    eprintln!("Fanticon warning: {error}; battery RAM is read-only");
                    None
                }
            }
        });
        self.game = Some(GameSession {
            machine,
            save_backing: launch.save_path.map_or(SaveBacking::None, SaveBacking::Console),
            last_save_generation: 0,
            last_save_write: None,
            launched_from_editor,
            #[cfg(not(target_arch = "wasm32"))]
            _save_lock: save_lock,
        });
    }

    fn handle_game_key(
        &mut self,
        state: ElementState,
        logical_key: &Key,
        physical_key: PhysicalKey,
    ) {
        if state == ElementState::Pressed
            && matches!(logical_key, Key::Named(NamedKey::Escape))
            && self.game.as_ref().is_some_and(|game| game.launched_from_editor)
        {
            self.flush_game_save();
            self.game = None;
            if self.text_editor.is_none() {
                self.terminal.resume_after_game();
            }
            return;
        }
        let Some(game) = &mut self.game else { return };
        let mask = match physical_key {
            PhysicalKey::Code(KeyCode::ArrowUp) => ControllerState::UP,
            PhysicalKey::Code(KeyCode::ArrowDown) => ControllerState::DOWN,
            PhysicalKey::Code(KeyCode::ArrowLeft) => ControllerState::LEFT,
            PhysicalKey::Code(KeyCode::ArrowRight) => ControllerState::RIGHT,
            PhysicalKey::Code(KeyCode::KeyZ) => ControllerState::A,
            PhysicalKey::Code(KeyCode::KeyX) => ControllerState::B,
            PhysicalKey::Code(KeyCode::Space) => ControllerState::SELECT,
            PhysicalKey::Code(KeyCode::Enter) => ControllerState::START,
            _ => return,
        };
        let current = game.machine.bus.controller_host_state(0);
        let next = if state == ElementState::Pressed { current | mask } else { current & !mask };
        game.machine.bus.set_controller(0, ControllerState(next));
    }

    fn flush_game_save(&mut self) {
        let Some(game) = &mut self.game else { return };
        if !game.machine.bus.save_dirty() {
            return;
        }
        let result = match &game.save_backing {
            SaveBacking::None => return,
            SaveBacking::Console(path) => host::write_save(
                &self.terminal.filesystem(),
                path,
                game.machine.bus.cartridge_id(),
                game.machine.bus.save_ram(),
            ),
            #[cfg(not(target_arch = "wasm32"))]
            SaveBacking::Native(path) => write_native_save(
                path,
                game.machine.bus.cartridge_id(),
                game.machine.bus.save_ram(),
            ),
        };
        match result {
            Ok(()) => {
                game.machine.bus.mark_save_clean();
                game.last_save_write = None;
            }
            Err(error) => eprintln!("could not save cartridge RAM: {error}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_direct_game(&mut self, launch: DirectGameLaunch) {
        use fs2::FileExt;
        let mut machine = FanticonMachine::new(launch.cartridge, Some(launch.save_ram));
        let save_lock = launch.save_path.as_ref().and_then(|path| {
            let lock_path = path.with_extension("SAV.lock");
            match std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock_path) {
                Ok(file) => match file.try_lock_exclusive() {
                    Ok(()) => Some(file),
                    Err(error) => {
                        machine.bus.set_save_writable(false);
                        eprintln!("Fanticon warning: save lock unavailable ({error}); battery RAM is read-only");
                        None
                    }
                },
                Err(error) => {
                    machine.bus.set_save_writable(false);
                    eprintln!("Fanticon warning: save lock unavailable ({error}); battery RAM is read-only");
                    None
                }
            }
        });
        self.game = Some(GameSession {
            machine,
            save_backing: launch.save_path.map_or(SaveBacking::None, SaveBacking::Native),
            last_save_generation: 0,
            last_save_write: None,
            launched_from_editor: false,
            _save_lock: save_lock,
        });
    }
}

fn window_to_source_position(
    position: PhysicalPosition<f64>,
    surface: PhysicalSize<u32>,
    source: (usize, usize),
) -> Option<(usize, usize)> {
    if surface.width == 0 || surface.height == 0 || source.0 == 0 || source.1 == 0 {
        return None;
    }
    let scale = (f64::from(surface.width) / source.0 as f64)
        .min(f64::from(surface.height) / source.1 as f64);
    let content_width = source.0 as f64 * scale;
    let content_height = source.1 as f64 * scale;
    let origin_x = ((f64::from(surface.width) - content_width) * 0.5).floor();
    let origin_y = ((f64::from(surface.height) - content_height) * 0.5).floor();
    if position.x < origin_x
        || position.y < origin_y
        || position.x >= origin_x + content_width
        || position.y >= origin_y + content_height
    {
        return None;
    }
    Some((
        ((position.x - origin_x) / scale).floor() as usize,
        ((position.y - origin_y) / scale).floor() as usize,
    ))
}

fn dispatch_terminal_key(terminal: &mut Terminal, key: &Key) -> TerminalAction {
    match key {
        Key::Named(NamedKey::Enter) => terminal.submit(),
        Key::Named(NamedKey::Backspace) => {
            terminal.backspace();
            TerminalAction::None
        }
        Key::Named(NamedKey::F1) => TerminalAction::SwitchMode(AppMode::Editor),
        Key::Named(NamedKey::F2) => TerminalAction::SwitchMode(AppMode::Game),
        Key::Named(NamedKey::Space) => {
            terminal.input_character(' ');
            TerminalAction::None
        }
        Key::Character(text) => {
            for character in text.chars() {
                terminal.input_character(character);
            }
            TerminalAction::None
        }
        _ => TerminalAction::None,
    }
}

const fn should_process_keyboard_input(state: ElementState, _repeat: bool) -> bool {
    matches!(state, ElementState::Pressed)
}

fn create_event_loop() -> Result<EventLoop<UserEvent>, winit::error::EventLoopError> {
    EventLoop::<UserEvent>::with_user_event().build()
}

fn initial_mode() -> AppMode {
    if std::env::args().any(|argument| argument == "--game") {
        AppMode::Game
    } else {
        AppMode::Editor
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = create_event_loop()?;
    let mut app = FanticonApp::new(event_loop.create_proxy(), initial_mode());
    if let Some(path) = direct_cartridge_argument() {
        app.start_direct_game(load_direct_cartridge(&path)?);
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn direct_cartridge_argument() -> Option<PathBuf> {
    std::env::args_os().skip(1).find_map(|argument| {
        let path = PathBuf::from(argument);
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("fcn"))
            .then_some(path)
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn load_direct_cartridge(path: &Path) -> Result<DirectGameLaunch, Box<dyn std::error::Error>> {
    let cartridge = fanticon::cartridge::Cartridge::from_bytes(&std::fs::read(path)?)?;
    if cartridge.save_banks == 0 {
        return Ok(DirectGameLaunch { cartridge, save_path: None, save_ram: Vec::new() });
    }
    let save_path = path.with_extension("SAV");
    let expected = usize::from(cartridge.save_banks) * fanticon::machine::BANK_SIZE;
    let save_ram = match std::fs::read(&save_path) {
        Ok(bytes) => {
            let save = fanticon::cartridge::SaveImage::from_bytes(&bytes)?;
            if save.cartridge_id != cartridge.id {
                return Err("save belongs to a different cartridge ID".into());
            }
            if save.ram.len() == expected {
                save.ram
            } else {
                let ram = vec![0; expected];
                write_native_save(&save_path, cartridge.id, &ram)?;
                ram
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![0; expected],
        Err(error) => return Err(error.into()),
    };
    Ok(DirectGameLaunch { cartridge, save_path: Some(save_path), save_ram })
}

#[cfg(not(target_arch = "wasm32"))]
fn write_native_save(path: &Path, cartridge_id: u64, ram: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let bytes = fanticon::cartridge::SaveImage { cartridge_id, ram: ram.to_vec() }
        .to_bytes()
        .map_err(|error| error.0)?;
    let temporary = path.with_extension("SAV.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use winit::platform::web::EventLoopExtWebSys;

    let event_loop = create_event_loop().expect("create Fanticon event loop");
    let app = FanticonApp::new(event_loop.create_proxy(), initial_mode());
    event_loop.spawn_app(app);
}

#[cfg(target_arch = "wasm32")]
fn attach_canvas(window: &Window) {
    use wasm_bindgen::JsCast;
    use winit::platform::web::WindowExtWebSys;

    let Some(browser_window) = web_sys::window() else { return };
    let Some(document) = browser_window.document() else { return };
    let Some(body) = document.body() else { return };
    let canvas = window.canvas().expect("Fanticon window has a canvas");
    canvas.set_id("fanticon-display");
    let _ = body.append_child(canvas.unchecked_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_space_key_is_inserted_into_terminal_input() {
        let mut terminal = Terminal::new(AppMode::Editor);

        dispatch_terminal_key(&mut terminal, &Key::Character("ECHO".into()));
        dispatch_terminal_key(&mut terminal, &Key::Named(NamedKey::Space));
        dispatch_terminal_key(&mut terminal, &Key::Character("OK".into()));

        assert_eq!(terminal.submit(), TerminalAction::None);
    }

    #[test]
    fn pressed_key_repeats_are_processed_but_releases_are_not() {
        assert!(should_process_keyboard_input(ElementState::Pressed, false));
        assert!(should_process_keyboard_input(ElementState::Pressed, true));
        assert!(!should_process_keyboard_input(ElementState::Released, false));
        assert!(!should_process_keyboard_input(ElementState::Released, true));
    }

    #[test]
    fn mouse_coordinates_follow_letterboxed_source_image() {
        let surface = PhysicalSize::new(1_000, 600);
        let source = (640, 400);

        assert_eq!(
            window_to_source_position(PhysicalPosition::new(20.0, 0.0), surface, source),
            Some((0, 0))
        );
        assert_eq!(
            window_to_source_position(PhysicalPosition::new(979.0, 599.0), surface, source),
            Some((639, 399))
        );
        assert_eq!(
            window_to_source_position(PhysicalPosition::new(19.0, 300.0), surface, source),
            None
        );
    }
}
