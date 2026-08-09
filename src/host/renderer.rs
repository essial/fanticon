use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, RGBA_FRAME_LEN, Video};

use super::GraphicsSettings;
use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};
use super::surface::Surface;
use web_time::Instant;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DisplayUniform {
    surface_size: [f32; 2],
    source_size: [f32; 2],
    style: f32,
    effect_strength: f32,
    brightness: f32,
    integer_scaling: f32,
    time_seconds: f32,
    text_mode: f32,
    _padding: [f32; 2],
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    configuration: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    display_texture: wgpu::Texture,
    uniform_buffer: wgpu::Buffer,
    rgba_frame: Vec<u8>,
    start_time: Instant,
    text_mode: bool,
    source_size: (u32, u32),
    graphics: GraphicsSettings,
    diagnostics_lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStatus {
    Presented,
    Reconfigure,
    Skip,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, graphics: GraphicsSettings) -> Result<Self, String> {
        let size = nonzero_size(window.inner_size());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|error| format!("could not create display surface: {error}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| format!("could not find a graphics adapter: {error}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Fanticon GPU device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| format!("could not create graphics device: {error}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        // Emulation keeps its own exact 60 Hz clock, which never divides evenly
        // into a real display's refresh. Mailbox always shows the newest
        // completed frame and never blocks the loop waiting for a vblank, so a
        // frame finished slightly early is not held back to beat against the
        // panel. Fifo is the fallback: still tear-free, but it blocks inside the
        // redraw, which is what makes the mismatch visible as judder.
        let present_mode = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Fifo]
            .into_iter()
            .find(|mode| capabilities.present_modes.contains(mode))
            .unwrap_or(capabilities.present_modes[0]);
        let configuration = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width,
            height: size.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &configuration);

        let display_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Fanticon display"),
            size: wgpu::Extent3d {
                width: DISPLAY_WIDTH as u32,
                height: DISPLAY_HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = display_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Fanticon nearest-neighbor sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniform = DisplayUniform {
            surface_size: [size.width as f32, size.height as f32],
            source_size: [DISPLAY_WIDTH as f32, DISPLAY_HEIGHT as f32],
            style: graphics.style.shader_id() as f32,
            effect_strength: graphics.effect_strength,
            brightness: graphics.brightness,
            integer_scaling: u8::from(graphics.integer_scaling) as f32,
            time_seconds: 0.0,
            text_mode: 0.0,
            _padding: [0.0; 2],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Fanticon display uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Fanticon display bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fanticon display bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry { binding: 2, resource: uniform_buffer.as_entire_binding() },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Fanticon CRT shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("crt.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Fanticon display pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Fanticon CRT pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            configuration,
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            display_texture,
            uniform_buffer,
            rgba_frame: vec![0; RGBA_FRAME_LEN],
            start_time: Instant::now(),
            text_mode: false,
            source_size: (DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32),
            graphics,
            diagnostics_lines: Vec::new(),
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.configuration.width = size.width;
        self.configuration.height = size.height;
        self.surface.configure(&self.device, &self.configuration);
        self.write_uniform();
    }

    pub fn apply_graphics(&mut self, graphics: GraphicsSettings) {
        self.graphics = graphics;
        self.write_uniform();
    }

    pub fn set_diagnostics_lines(&mut self, lines: Vec<String>) {
        self.diagnostics_lines = lines;
    }

    /// Present the console's own output, resolving its indexed pixels through
    /// the cartridge palette.
    pub fn render(&mut self, video: &mut Video, text_mode: bool) -> FrameStatus {
        let source_size = (video.width() as u32, video.height() as u32);
        if source_size != self.source_size {
            self.set_source_size(source_size);
        }
        self.rgba_frame.resize(video.rgba_len(), 0);
        video.resolve_rgba(&mut self.rgba_frame).expect("fixed-size display buffer");
        draw_diagnostics_overlay(&mut self.rgba_frame, source_size, &self.diagnostics_lines);
        self.present(text_mode)
    }

    /// Present host interface pixels, which are already true color and owe the
    /// cartridge palette nothing.
    pub fn render_surface(&mut self, frame: &Surface, text_mode: bool) -> FrameStatus {
        let (width, height) = frame.dimensions();
        let source_size = (width as u32, height as u32);
        if source_size != self.source_size {
            self.set_source_size(source_size);
        }
        self.rgba_frame.clear();
        self.rgba_frame.extend_from_slice(frame.pixels());
        draw_diagnostics_overlay(&mut self.rgba_frame, source_size, &self.diagnostics_lines);
        self.present(text_mode)
    }

    fn present(&mut self, text_mode: bool) -> FrameStatus {
        self.text_mode = text_mode;
        self.write_uniform();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.display_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba_frame,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.source_size.0 * 4),
                rows_per_image: Some(self.source_size.1),
            },
            wgpu::Extent3d {
                width: self.source_size.0,
                height: self.source_size.1,
                depth_or_array_layers: 1,
            },
        );

        let (frame, reconfigure_after_present) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return FrameStatus::Reconfigure;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return FrameStatus::Skip,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Fanticon display commands"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Fanticon display pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        if reconfigure_after_present { FrameStatus::Reconfigure } else { FrameStatus::Presented }
    }

    fn write_uniform(&self) {
        let uniform = DisplayUniform {
            surface_size: [self.configuration.width as f32, self.configuration.height as f32],
            source_size: [self.source_size.0 as f32, self.source_size.1 as f32],
            style: self.graphics.style.shader_id() as f32,
            effect_strength: self.graphics.effect_strength,
            brightness: self.graphics.brightness,
            integer_scaling: u8::from(self.graphics.integer_scaling) as f32,
            time_seconds: self.start_time.elapsed().as_secs_f32(),
            text_mode: u8::from(self.text_mode) as f32,
            _padding: [0.0; 2],
        };
        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    fn set_source_size(&mut self, source_size: (u32, u32)) {
        self.display_texture = create_display_texture(&self.device, source_size);
        let texture_view =
            self.display_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.bind_group = create_display_bind_group(
            &self.device,
            &self.bind_group_layout,
            &texture_view,
            &self.sampler,
            &self.uniform_buffer,
        );
        self.source_size = source_size;
        self.rgba_frame.resize(source_size.0 as usize * source_size.1 as usize * 4, 0);
    }
}

fn create_display_texture(device: &wgpu::Device, size: (u32, u32)) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Fanticon display"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_display_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Fanticon display bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(texture_view),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform_buffer.as_entire_binding() },
        ],
    })
}

fn nonzero_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn draw_diagnostics_overlay(frame: &mut [u8], size: (u32, u32), lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let width = size.0 as usize;
    let height = size.1 as usize;
    let columns = lines.iter().map(String::len).max().unwrap_or(0).min(width / GLYPH_WIDTH);
    let box_width = (columns * GLYPH_WIDTH + 8).min(width);
    let box_height = (lines.len() * GLYPH_HEIGHT + 8).min(height);
    for y in 0..box_height {
        for x in 0..box_width {
            let offset = (y * width + x) * 4;
            for (channel, tint) in [8_u8, 10, 16].into_iter().enumerate() {
                frame[offset + channel] = frame[offset + channel] / 4 + tint * 3 / 4;
            }
            frame[offset + 3] = 255;
        }
    }
    for (line_index, line) in lines.iter().enumerate() {
        let y = 4 + line_index * GLYPH_HEIGHT;
        for (character_index, byte) in line.bytes().take(columns).enumerate() {
            let glyph = CHARACTER_ROM[usize::from(byte.min(127))];
            let x = 4 + character_index * GLYPH_WIDTH;
            for (glyph_y, bits) in glyph.into_iter().enumerate() {
                for glyph_x in 0..GLYPH_WIDTH {
                    if bits & (0x80 >> glyph_x) == 0
                        || x + glyph_x >= width
                        || y + glyph_y >= height
                    {
                        continue;
                    }
                    let offset = ((y + glyph_y) * width + x + glyph_x) * 4;
                    frame[offset..offset + 4].copy_from_slice(&[110, 235, 205, 255]);
                }
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn presentation_shader_parses_and_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("crt.wgsl"))
            .expect("presentation shader must parse");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("presentation shader must validate");
    }

    #[test]
    fn display_uniform_matches_wgsl_alignment() {
        assert_eq!(std::mem::size_of::<DisplayUniform>(), 48);
        assert_eq!(std::mem::align_of::<DisplayUniform>(), 4);
    }

    #[test]
    fn diagnostics_overlay_composites_into_rgba_frame() {
        let mut frame = vec![255; 320 * 200 * 4];
        draw_diagnostics_overlay(&mut frame, (320, 200), &["FPS 60.0".to_owned()]);
        assert_eq!(&frame[..4], &[69, 70, 75, 255]);
        assert!(frame.chunks_exact(4).any(|pixel| pixel == [110, 235, 205, 255]));
    }
}
