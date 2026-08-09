// Release/dist builds run without an attached console, so a plain Windows
// binary would otherwise pop up a separate console window alongside the app
// window. Debug builds keep the console so eprintln!/panic output is still
// visible while developing. This attribute is a no-op on non-Windows targets.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod host;

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fanticon::{
    debugger::Debugger,
    machine::CPU_CYCLES_PER_FRAME,
    system::{ControllerState, FanticonMachine},
    video::Video,
};
use host::{
    AppMode, AudioOutput, BootSplash, DebugCommand, EditorAction, FramePacer, FrameStatus,
    GamepadInput, MusicCommand, MusicRadio, Renderer, Surface, Terminal, TerminalAction,
    TextEditor, draw_boot_logo,
};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition, PhysicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

enum UserEvent {
    RendererReady(Result<Renderer, String>),
}

struct FanticonApp {
    event_proxy: winit::event_loop::EventLoopProxy<UserEvent>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    video: Video,
    /// True-color surface for the host's own interface, kept separate from the
    /// console's indexed output so chrome never competes for cartridge colors.
    editor_surface: Surface,
    frame_pacer: FramePacer,
    frame_number: u64,
    boot_splash: BootSplash,
    terminal: Terminal,
    text_editor: Option<TextEditor>,
    modifiers: ModifiersState,
    game: Option<GameSession>,
    audio_output: Option<AudioOutput>,
    music: MusicRadio,
    mouse_position: Option<PhysicalPosition<f64>>,
    gamepads: GamepadInput,
    keyboard_controller: u8,
    input_focused: bool,
}

struct GameSession {
    debugger: Debugger,
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
            editor_surface: Surface::new(host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT),
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
            music: MusicRadio::new(),
            mouse_position: None,
            gamepads: GamepadInput::new(),
            keyboard_controller: 0,
            input_focused: true,
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
        #[cfg(not(target_arch = "wasm32"))]
        let attributes = attributes.with_window_icon(app_icon());
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
                // Alt+Enter toggles fullscreen regardless of what has focus
                // (boot splash, running game, or editor), so it must be
                // handled first -- otherwise, e.g., a running game would also
                // see the Enter half of the chord and fire START.
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && self.modifiers.alt_key()
                    && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Enter))
                {
                    if window.fullscreen().is_some() {
                        window.set_fullscreen(None);
                    } else {
                        window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    }
                    return;
                }
                let now = Instant::now();
                if self.boot_splash.is_active(now) {
                    if event.state == ElementState::Pressed {
                        self.boot_splash.try_dismiss(now);
                    }
                } else if self.game_running() {
                    self.handle_game_key(event.state, &event.logical_key, event.physical_key);
                } else {
                    // A breakpoint can take focus while a controller key is held. Its
                    // eventual release must still clear the host-side controller latch,
                    // even though other input now belongs to the editor/debugger.
                    if event.state == ElementState::Released {
                        self.update_game_controller_key(event.state, event.physical_key);
                        let action =
                            self.text_editor.as_mut().map_or(EditorAction::None, |editor| {
                                editor.handle_key_release(event.physical_key)
                            });
                        self.apply_editor_action(action);
                    }
                    if should_process_keyboard_input(event.state, event.repeat) {
                        self.handle_key(event_loop, &event.logical_key, event.physical_key);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Focused(focused) => {
                self.input_focused = focused;
                if !focused {
                    self.clear_game_inputs();
                    let action = self
                        .text_editor
                        .as_mut()
                        .map_or(EditorAction::None, TextEditor::cancel_music_audition);
                    self.apply_editor_action(action);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position = Some(position);
                if !self.boot_splash.is_active(Instant::now())
                    && !self.game_running()
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
                if button != MouseButton::Left || self.game_running() {
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
                if self.boot_splash.is_active(Instant::now()) || self.game_running() {
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
                // Present whatever the frame actually drew into. This has to match
                // the branch in emulate_frame exactly: a game stopped at a
                // breakpoint still exists, but the editor is what is on screen and
                // drawing, so the surface is the live target. Both are settled
                // before the renderer is borrowed.
                let splash = self.boot_splash.is_active(Instant::now());
                let editor_surface = self.text_editor.is_some() && !splash && !self.game_running();
                let editor_presentation = !splash
                    && (editor_surface
                        || self.video.dimensions()
                            == (host::EDITOR_DISPLAY_WIDTH, host::EDITOR_DISPLAY_HEIGHT));
                if let Some(renderer) = &mut self.renderer {
                    let status = if editor_surface {
                        renderer.render_surface(&self.editor_surface, editor_presentation)
                    } else {
                        renderer.render(&mut self.video, editor_presentation)
                    };
                    match status {
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
    fn game_running(&self) -> bool {
        self.game.as_ref().is_some_and(|game| !game.debugger.paused())
    }

    fn emulate_frame(&mut self, now: Instant) {
        if self.boot_splash.is_active(now) {
            self.video.begin_frame();
            return;
        }

        if self.game_running() {
            self.poll_gamepads();
            let game = self.game.as_mut().expect("running game session");
            if self.video.dimensions()
                != (fanticon::video::DISPLAY_WIDTH, fanticon::video::DISPLAY_HEIGHT)
            {
                self.video = Video::new();
            }
            game.debugger.run_cycles(u64::from(CPU_CYCLES_PER_FRAME));
            if game.debugger.paused() {
                // Debugger focus owns the keyboard now. Drop the host latch so a
                // controller key held when the stop occurred cannot remain pressed
                // after execution resumes.
                game.debugger.machine.bus.set_controller(0, ControllerState::default());
                game.debugger.machine.bus.set_controller(1, ControllerState::default());
                self.keyboard_controller = 0;
                self.gamepads.suppress_held_inputs();
            }
            if !game.debugger.paused()
                && let Some(audio) = &self.audio_output
            {
                audio.submit(game.debugger.machine.bus.audio_frame());
            }
            game.debugger.machine.bus.present(&mut self.video);
            self.video.begin_frame();
            let generation = game.debugger.machine.bus.save_generation();
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
            if let Some(snapshot) = self
                .game
                .as_ref()
                .filter(|game| game.debugger.paused())
                .map(|game| game.debugger.snapshot())
                && let Some(editor) = &mut self.text_editor
            {
                editor.set_debug_snapshot(snapshot);
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

        if let Some(frame) = self.music.render_frame()
            && let Some(audio) = &self.audio_output
        {
            audio.submit_at_rate(frame.samples, frame.source_rate);
        }

        let cursor_visible = (self.frame_number / 30).is_multiple_of(2);
        if let Some(editor) = &mut self.text_editor {
            editor.set_music_status(self.music.status());
            let action = editor.update();
            if let EditorAction::Run(launch) = action {
                self.start_game(launch, true);
                return;
            }
            // The editor owns its blink phase so it can restart on caret movement.
            let cursor_visible = editor.cursor_blink_visible();
            self.editor_surface.resize(dimensions.0, dimensions.1);
            editor.render(&mut self.editor_surface, cursor_visible);
        } else {
            self.terminal.render(&mut self.video, cursor_visible);
        }
        self.video.begin_frame();
        self.frame_number = self.frame_number.wrapping_add(1);
    }

    fn handle_key(
        &mut self,
        event_loop: &ActiveEventLoop,
        key: &Key,
        physical_key: winit::keyboard::PhysicalKey,
    ) {
        if let Some(editor) = &mut self.text_editor {
            let action = editor.handle_key(key, physical_key, self.modifiers);
            self.apply_editor_action(action);
            return;
        }

        let action = dispatch_terminal_key(&mut self.terminal, key);
        match action {
            TerminalAction::SwitchMode(mode) => self.terminal.switch_mode(mode),
            TerminalAction::Edit(filename) => {
                let mut editor =
                    TextEditor::new(self.terminal.filesystem(), self.terminal.colors(), filename);
                editor.set_music_status(self.music.status());
                self.text_editor = Some(editor);
            }
            TerminalAction::Run(launch) => self.start_game(launch, true),
            TerminalAction::Music(command) => {
                let result = self.apply_music_command(command);
                self.terminal.finish_music_command(result);
            }
            // Typing EXIT/QUIT kills the whole virtual console, the same
            // shutdown path as closing the window.
            TerminalAction::Exit => {
                self.flush_game_save();
                event_loop.exit();
            }
            TerminalAction::None => {}
        }
    }

    fn apply_editor_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::Exit => self.text_editor = None,
            EditorAction::Run(launch) => self.start_game(launch, true),
            EditorAction::Debug(command) => self.apply_debug_command(command),
            EditorAction::Music(command) => {
                let _ = self.apply_music_command(command);
            }
            EditorAction::None => {}
        }
    }

    fn apply_music_command(&mut self, command: MusicCommand) -> Result<String, String> {
        if matches!(
            command,
            MusicCommand::Load { .. }
                | MusicCommand::LoadTracker { .. }
                | MusicCommand::AuditionTracker { .. }
                | MusicCommand::Stop
                | MusicCommand::Next
                | MusicCommand::Previous
        ) && let Some(audio) = &self.audio_output
        {
            audio.clear();
        }
        let result = self.music.apply(command);
        if let Some(editor) = &mut self.text_editor {
            editor.set_music_status(self.music.status());
        }
        result
    }

    fn apply_debug_command(&mut self, command: DebugCommand) {
        if matches!(command, DebugCommand::Stop) {
            self.flush_game_save();
            self.game = None;
            if let Some(editor) = &mut self.text_editor {
                editor.stop_debug_session();
            }
            return;
        }
        let Some(game) = &mut self.game else { return };
        match command {
            DebugCommand::Continue => game.debugger.resume(),
            DebugCommand::StepInstruction => {
                game.debugger.step_instruction();
            }
            DebugCommand::StepCycle => game.debugger.step_cycle(),
            DebugCommand::StepOver => {
                game.debugger.step_over(10_000_000);
            }
            DebugCommand::StepOut => {
                game.debugger.step_out(10_000_000);
            }
            DebugCommand::SyncBreakpoints(breakpoints) => {
                game.debugger.set_source_breakpoints(breakpoints);
            }
            DebugCommand::AddReadWatchpoint(address) => game.debugger.add_read_watchpoint(address),
            DebugCommand::AddWriteWatchpoint(address) => {
                game.debugger.add_write_watchpoint(address);
            }
            DebugCommand::AddRasterBreakpoint { dot, line } => {
                game.debugger.add_raster_breakpoint(dot, line);
            }
            DebugCommand::WriteMemory { address, value } => {
                if let Err(error) = game.debugger.write_memory(address, value)
                    && let Some(editor) = &mut self.text_editor
                {
                    editor.show_debug_error(error);
                }
            }
            DebugCommand::RemoveStop(stop) => game.debugger.remove_stop(stop),
            DebugCommand::ClearBreakpoints => game.debugger.clear_breakpoints(),
            DebugCommand::Stop => unreachable!(),
        }
        if game.debugger.paused()
            && let Some(editor) = &mut self.text_editor
        {
            editor.set_debug_snapshot(game.debugger.snapshot());
        }
    }

    fn start_game(&mut self, launch: host::GameLaunch, launched_from_editor: bool) {
        self.keyboard_controller = 0;
        self.gamepads.suppress_held_inputs();
        if let Some(audio) = &self.audio_output {
            audio.clear();
        }
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
        let mut debugger = Debugger::new(machine);
        debugger.set_source_breakpoints(launch.breakpoints);
        debugger.resume();
        self.game = Some(GameSession {
            debugger,
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
            && matches!(logical_key, Key::Named(NamedKey::F6))
            && self.text_editor.is_some()
            && let Some(game) = &mut self.game
        {
            game.debugger.pause();
            game.debugger.machine.bus.set_controller(0, ControllerState::default());
            game.debugger.machine.bus.set_controller(1, ControllerState::default());
            self.keyboard_controller = 0;
            self.gamepads.suppress_held_inputs();
            if let Some(editor) = &mut self.text_editor {
                editor.set_debug_snapshot(game.debugger.snapshot());
            }
            return;
        }
        if state == ElementState::Pressed
            && matches!(logical_key, Key::Named(NamedKey::Escape))
            && self.game.as_ref().is_some_and(|game| game.launched_from_editor)
        {
            self.flush_game_save();
            self.game = None;
            if let Some(editor) = &mut self.text_editor {
                editor.stop_debug_session();
            } else {
                self.terminal.resume_after_game();
            }
            return;
        }
        self.update_game_controller_key(state, physical_key);
    }

    fn update_game_controller_key(&mut self, state: ElementState, physical_key: PhysicalKey) {
        let Some(next) = updated_controller_state(self.keyboard_controller, state, physical_key)
        else {
            return;
        };
        self.keyboard_controller = next;
        self.poll_gamepads();
    }

    fn poll_gamepads(&mut self) {
        let gamepad = if self.input_focused { self.gamepads.poll() } else { [0; 2] };
        let Some(game) = &mut self.game else { return };
        game.debugger
            .machine
            .bus
            .set_controller(0, ControllerState(self.keyboard_controller | gamepad[0]));
        game.debugger.machine.bus.set_controller(1, ControllerState(gamepad[1]));
    }

    fn clear_game_inputs(&mut self) {
        self.keyboard_controller = 0;
        self.gamepads.suppress_held_inputs();
        if let Some(game) = &mut self.game {
            game.debugger.machine.bus.set_controller(0, ControllerState::default());
            game.debugger.machine.bus.set_controller(1, ControllerState::default());
        }
    }

    fn flush_game_save(&mut self) {
        let Some(game) = &mut self.game else { return };
        if !game.debugger.machine.bus.save_dirty() {
            return;
        }
        let result = match &game.save_backing {
            SaveBacking::None => return,
            SaveBacking::Console(path) => host::write_save(
                &self.terminal.filesystem(),
                path,
                game.debugger.machine.bus.cartridge_id(),
                game.debugger.machine.bus.save_ram(),
            ),
            #[cfg(not(target_arch = "wasm32"))]
            SaveBacking::Native(path) => write_native_save(
                path,
                game.debugger.machine.bus.cartridge_id(),
                game.debugger.machine.bus.save_ram(),
            ),
        };
        match result {
            Ok(()) => {
                game.debugger.machine.bus.mark_save_clean();
                game.last_save_write = None;
            }
            Err(error) => eprintln!("could not save cartridge RAM: {error}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_direct_game(&mut self, launch: DirectGameLaunch) {
        use fs2::FileExt;
        self.keyboard_controller = 0;
        self.gamepads.suppress_held_inputs();
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
        let mut debugger = Debugger::new(machine);
        debugger.resume();
        self.game = Some(GameSession {
            debugger,
            save_backing: launch.save_path.map_or(SaveBacking::None, SaveBacking::Native),
            last_save_generation: 0,
            last_save_write: None,
            launched_from_editor: false,
            _save_lock: save_lock,
        });
    }
}

fn updated_controller_state(
    current: u8,
    state: ElementState,
    physical_key: PhysicalKey,
) -> Option<u8> {
    let mask = match physical_key {
        PhysicalKey::Code(KeyCode::ArrowUp) => ControllerState::UP,
        PhysicalKey::Code(KeyCode::ArrowDown) => ControllerState::DOWN,
        PhysicalKey::Code(KeyCode::ArrowLeft) => ControllerState::LEFT,
        PhysicalKey::Code(KeyCode::ArrowRight) => ControllerState::RIGHT,
        PhysicalKey::Code(KeyCode::KeyZ) => ControllerState::A,
        PhysicalKey::Code(KeyCode::KeyX) => ControllerState::B,
        PhysicalKey::Code(KeyCode::Space) => ControllerState::SELECT,
        PhysicalKey::Code(KeyCode::Enter) => ControllerState::START,
        _ => return None,
    };
    Some(if state == ElementState::Pressed { current | mask } else { current & !mask })
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

/// Raw RGBA8 pixels (128x128, row-major, straight alpha) for the Fanticon
/// badge icon, baked from `assets/branding/fanticon-icon-master.png`. Used to
/// set the window/taskbar icon so `cargo run` and dev builds show the real
/// icon, not just the packaged installers. Not wired up for wasm32: the web
/// build has no window chrome to attach an icon to.
#[cfg(not(target_arch = "wasm32"))]
const APP_ICON_RGBA: &[u8] =
    include_bytes!("../assets/branding/icons/fanticon-window-icon-128.rgba");

#[cfg(not(target_arch = "wasm32"))]
fn app_icon() -> Option<winit::window::Icon> {
    winit::window::Icon::from_rgba(APP_ICON_RGBA.to_vec(), 128, 128)
        .map_err(|error| eprintln!("Fanticon window icon could not be loaded: {error}"))
        .ok()
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
    if let Some(bytes) = fanticon::export::read_standalone_cartridge(&std::env::current_exe()?)? {
        app.start_direct_game(load_embedded_cartridge(&bytes)?);
    } else if let Some(path) = direct_cartridge_argument() {
        app.start_direct_game(load_direct_cartridge(&path)?);
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn load_embedded_cartridge(bytes: &[u8]) -> Result<DirectGameLaunch, Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let cartridge = fanticon::cartridge::Cartridge::from_bytes(bytes)?;
    let legacy_save_path = executable.with_extension("SAV");
    let save_path = dirs::data_local_dir()
        .map(|directory| {
            directory.join("Fanticon").join("saves").join(format!("{:016X}.SAV", cartridge.id))
        })
        .unwrap_or_else(|| legacy_save_path.clone());
    if save_path != legacy_save_path && !save_path.exists() && legacy_save_path.is_file() {
        if let Some(parent) = save_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(legacy_save_path, &save_path)?;
    }
    load_cartridge_with_save(cartridge, save_path)
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
    load_direct_cartridge_bytes(&std::fs::read(path)?, path.with_extension("SAV"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_direct_cartridge_bytes(
    bytes: &[u8],
    save_path: PathBuf,
) -> Result<DirectGameLaunch, Box<dyn std::error::Error>> {
    let cartridge = fanticon::cartridge::Cartridge::from_bytes(bytes)?;
    load_cartridge_with_save(cartridge, save_path)
}

#[cfg(not(target_arch = "wasm32"))]
fn load_cartridge_with_save(
    cartridge: fanticon::cartridge::Cartridge,
    save_path: PathBuf,
) -> Result<DirectGameLaunch, Box<dyn std::error::Error>> {
    if cartridge.save_banks == 0 {
        return Ok(DirectGameLaunch { cartridge, save_path: None, save_ram: Vec::new() });
    }
    if let Some(parent) = save_path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
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
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
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
    let deferred = web_sys::window().is_some_and(|window| {
        js_sys::Reflect::get(&window, &"FANTICON_DEFER_START".into())
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    });
    if deferred {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let web_cartridge = fetch_web_cartridge().await;
        start_web_app(web_cartridge);
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_fanticon(cartridge: js_sys::Uint8Array) {
    start_web_app(Some(cartridge.to_vec()));
}

#[cfg(target_arch = "wasm32")]
fn start_web_app(web_cartridge: Option<Vec<u8>>) {
    use winit::platform::web::EventLoopExtWebSys;

    let event_loop = create_event_loop().expect("create Fanticon event loop");
    let mut app = FanticonApp::new(event_loop.create_proxy(), initial_mode());
    if let Some(bytes) = web_cartridge {
        let filesystem = host::shared_filesystem();
        let launch = host::load_cartridge_bytes(&filesystem, "game.fcn", &bytes).unwrap_or_else(
            |diagnostics| panic!("exported web cartridge is invalid: {diagnostics:?}"),
        );
        app.start_game(launch, false);
    }
    event_loop.spawn_app(app);
}

#[cfg(target_arch = "wasm32")]
async fn fetch_web_cartridge() -> Option<Vec<u8>> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window()?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str("game.fcn"))
        .await
        .ok()?
        .dyn_into::<web_sys::Response>()
        .ok()?;
    if !response.ok() {
        return None;
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    Some(js_sys::Uint8Array::new(&buffer).to_vec())
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
    fn controller_key_release_clears_only_its_held_button() {
        let held = ControllerState::UP | ControllerState::A;
        assert_eq!(
            updated_controller_state(
                held,
                ElementState::Released,
                PhysicalKey::Code(KeyCode::KeyZ),
            ),
            Some(ControllerState::UP)
        );
        assert_eq!(
            updated_controller_state(
                ControllerState::UP,
                ElementState::Pressed,
                PhysicalKey::Code(KeyCode::Space),
            ),
            Some(ControllerState::UP | ControllerState::SELECT)
        );
        assert_eq!(
            updated_controller_state(held, ElementState::Released, PhysicalKey::Code(KeyCode::F9),),
            None
        );
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
