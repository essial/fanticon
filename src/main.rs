mod host;

use std::sync::Arc;

use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, RasterTick, Video};
use host::{FramePacer, FrameStatus, Renderer};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
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
}

impl FanticonApp {
    fn new(event_proxy: winit::event_loop::EventLoopProxy<UserEvent>) -> Self {
        let mut video = Video::new();
        draw_startup_screen(&mut video);
        Self {
            event_proxy,
            window: None,
            renderer: None,
            video,
            frame_pacer: FramePacer::new(Instant::now()),
            frame_number: 0,
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
                self.frame_pacer.reset(Instant::now());
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
            self.emulate_frame();
            window.request_redraw();
            self.frame_pacer.advance_after_frame(now);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.frame_pacer.next_deadline()));
    }
}

impl FanticonApp {
    fn emulate_frame(&mut self) {
        // The host currently has no VM to run. This split palette write makes
        // the timing path visible and exercises a sub-scanline state change.
        self.video.set_palette(0xe0, [245, 72, 66, 255]);
        self.video.begin_frame();
        let split_dot = (self.frame_number.wrapping_mul(2) % DISPLAY_WIDTH as u64) as u16;
        self.video
            .write_palette_at(RasterTick::new(100, split_dot).unwrap(), 0xe0, [66, 190, 245, 255])
            .expect("demo raster event is ordered");
        self.frame_number = self.frame_number.wrapping_add(1);
    }
}

fn draw_startup_screen(video: &mut Video) {
    let pixels = video.pixels_mut();
    for y in 0..DISPLAY_HEIGHT {
        for x in 0..DISPLAY_WIDTH {
            let grid = x % 32 == 0 || y % 25 == 0;
            let border =
                !(8..DISPLAY_WIDTH - 8).contains(&x) || !(8..DISPLAY_HEIGHT - 8).contains(&y);
            let color = if border {
                0xff
            } else if grid {
                0x49
            } else if ((x / 16) + (y / 16)) & 1 == 0 {
                0xe0
            } else {
                0x03
            };
            pixels[y * DISPLAY_WIDTH + x] = color;
        }
    }
}

fn create_event_loop() -> Result<EventLoop<UserEvent>, winit::error::EventLoopError> {
    EventLoop::<UserEvent>::with_user_event().build()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = create_event_loop()?;
    let mut app = FanticonApp::new(event_loop.create_proxy());
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use winit::platform::web::EventLoopExtWebSys;

    let event_loop = create_event_loop().expect("create Fanticon event loop");
    let app = FanticonApp::new(event_loop.create_proxy());
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
