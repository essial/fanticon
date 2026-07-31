mod host;

use std::sync::Arc;

use fanticon::video::Video;
use host::{
    AppMode, BootSplash, FramePacer, FrameStatus, Renderer, Terminal, TerminalAction,
    draw_boot_logo,
};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size);
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. }
                if should_process_keyboard_input(event.state, event.repeat) =>
            {
                let now = Instant::now();
                if self.boot_splash.is_active(now) {
                    self.boot_splash.try_dismiss(now);
                } else {
                    self.handle_key(&event.logical_key);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, .. } => {
                let now = Instant::now();
                if self.boot_splash.is_active(now) {
                    self.boot_splash.try_dismiss(now);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    match renderer.render(&mut self.video) {
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

        let cursor_visible = (self.frame_number / 30).is_multiple_of(2);
        self.terminal.render(&mut self.video, cursor_visible);
        self.video.begin_frame();
        self.frame_number = self.frame_number.wrapping_add(1);
    }

    fn handle_key(&mut self, key: &Key) {
        let action = dispatch_terminal_key(&mut self.terminal, key);
        if let TerminalAction::SwitchMode(mode) = action {
            self.terminal.switch_mode(mode);
        }
    }
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
    event_loop.run_app(&mut app)?;
    Ok(())
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
}
