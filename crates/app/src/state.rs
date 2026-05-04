#![allow(clippy::cast_precision_loss)]

use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use tracing::debug;
use winit::window::Window;

use crate::{
    browser::{build_chrome_paint_plan, ChromeSnapshot, TOOLBAR_HEIGHT},
    render::{
        build_draw_mesh_from_plan, encode_vertices, vertex_buffer_layout, DrawMesh,
        SharedPaintState,
    },
};
use present::{Color, IncrementalReflowEngine, PaintCommand, PaintPlan, ReflowRequest};

const INITIAL_VERTEX_BUFFER_BYTES: wgpu::BufferAddress = 4096;
const EMPTY_PAGE_BACKGROUND: Color = Color::rgba(0.95, 0.95, 0.94, 1.0);
const EMPTY_PAGE_TEXT: Color = Color::rgba(0.18, 0.18, 0.20, 1.0);

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    clear_rgba: Arc<Mutex<[f32; 4]>>,
    paint_state: SharedPaintState,
    vertex_buffer: wgpu::Buffer,
    vertex_buffer_capacity: wgpu::BufferAddress,
    vertex_count: u32,
    media_bind_group_layout: wgpu::BindGroupLayout,
    media_bind_group: wgpu::BindGroup,
    last_paint_revision: u64,
    last_document_revision: u64,
    last_image_revision: u64,
    last_chrome_revision: u64,
    page_reflow: IncrementalReflowEngine,
    mesh_dirty: bool,
}

impl State {
    pub async fn new(
        window: Arc<Window>,
        clear_rgba: Arc<Mutex<[f32; 4]>>,
        paint_state: SharedPaintState,
    ) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();

        let surface = instance
            .create_surface(window.clone())
            .context("failed to create GPU surface")?;

        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
        {
            Some(adapter) => adapter,
            None => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: true,
                })
                .await
                .context("no compatible GPU adapter found")?,
        };

        let required_features = wgpu::Features::empty();

        let required_limits = if cfg!(target_arch = "wasm32") {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::default()
        };

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Syphos GPU Device"),
                    required_features,
                    required_limits,
                },
                None,
            )
            .await
            .context("failed to request GPU device")?;

        let caps = surface.get_capabilities(&adapter);

        let format = caps
            .formats
            .first()
            .copied()
            .context("surface returned no supported formats")?;

        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .context("surface returned no supported alpha modes")?;

        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::AutoVsync) {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::Fifo
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let media_bind_group_layout = create_media_bind_group_layout(&device);
        let empty_draw_mesh = DrawMesh {
            vertices: Vec::new(),
            font_atlas: crate::render::font_atlas::FontAtlas::empty(),
            image_atlas: crate::render::ImageAtlas::empty(),
        };
        let media_bind_group =
            create_media_bind_group(&device, &queue, &media_bind_group_layout, &empty_draw_mesh);

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("syphos_paint_pipeline_layout"),
            bind_group_layouts: &[&media_bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_layout = vertex_buffer_layout();

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("syphos_paint_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let vertex_buffer = create_vertex_buffer(
            &device,
            INITIAL_VERTEX_BUFFER_BYTES,
            "syphos_initial_vertex_buffer",
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            clear_rgba,
            paint_state,
            vertex_buffer,
            vertex_buffer_capacity: INITIAL_VERTEX_BUFFER_BYTES,
            vertex_count: 0,
            media_bind_group_layout,
            media_bind_group,
            last_paint_revision: 0,
            last_document_revision: 0,
            last_image_revision: 0,
            last_chrome_revision: 0,
            page_reflow: IncrementalReflowEngine::new(),
            mesh_dirty: true,
        })
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.mesh_dirty = true;
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.update_paint_mesh_if_needed();

        let frame = self.surface.get_current_texture()?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("syphos_encoder"),
            });

        let rgba = self
            .clear_rgba
            .lock()
            .map_or([0.10, 0.10, 0.15, 1.0], |guard| *guard);

        let clear = wgpu::Color {
            r: f64::from(rgba[0]),
            g: f64::from(rgba[1]),
            b: f64::from(rgba[2]),
            a: f64::from(rgba[3]),
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("syphos_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            if self.vertex_count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.media_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..self.vertex_count, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();

        Ok(())
    }

    fn update_paint_mesh_if_needed(&mut self) {
        let Some(snapshot) = self.paint_state.snapshot() else {
            return;
        };

        if !self.mesh_dirty && snapshot.revision == self.last_paint_revision {
            return;
        }

        let force_page_reflow =
            self.mesh_dirty || snapshot.document_revision != self.last_document_revision;

        self.last_paint_revision = snapshot.revision;
        self.last_document_revision = snapshot.document_revision;
        self.last_image_revision = snapshot.image_revision;
        self.last_chrome_revision = snapshot.chrome_revision;
        self.mesh_dirty = false;

        let (paint_plan, reflow_summary) = self.build_viewport_paint_plan_incremental(
            snapshot.document.as_ref(),
            &snapshot.chrome,
            snapshot.invalidation.as_ref(),
            force_page_reflow,
        );

        if let Ok(mut guard) = self.clear_rgba.lock() {
            *guard = paint_plan.background.to_wgpu_clear();
        }

        let draw_mesh = build_draw_mesh_from_plan(
            &paint_plan,
            self.config.width as f32,
            self.config.height as f32,
            &snapshot.images,
        );

        debug!(
            font = %draw_mesh.font_atlas.font_name,
            images = snapshot.images.len(),
            reflow = %reflow_summary,
            "rebuilt paint mesh"
        );

        self.media_bind_group = create_media_bind_group(
            &self.device,
            &self.queue,
            &self.media_bind_group_layout,
            &draw_mesh,
        );

        let bytes = encode_vertices(&draw_mesh.vertices);

        if bytes.is_empty() {
            self.vertex_count = 0;
            return;
        }

        let required_size = match wgpu::BufferAddress::try_from(bytes.len()) {
            Ok(size) => size,
            Err(_) => {
                self.vertex_count = 0;
                return;
            }
        };

        if required_size > self.vertex_buffer_capacity {
            self.vertex_buffer_capacity = next_buffer_capacity(required_size);
            self.vertex_buffer = create_vertex_buffer(
                &self.device,
                self.vertex_buffer_capacity,
                "syphos_resized_vertex_buffer",
            );
        }

        self.queue.write_buffer(&self.vertex_buffer, 0, &bytes);

        self.vertex_count = match u32::try_from(draw_mesh.vertices.len()) {
            Ok(count) => count,
            Err(_) => u32::MAX,
        };
    }

    fn build_viewport_paint_plan_incremental(
        &mut self,
        document: Option<&present::RenderDocument>,
        chrome: &ChromeSnapshot,
        invalidation: Option<&present::InvalidationSet>,
        force_page_reflow: bool,
    ) -> (PaintPlan, String) {
        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let toolbar_height = TOOLBAR_HEIGHT.min(height.max(1.0));
        let page_height = (height - toolbar_height).max(1.0);

        let (page_plan, summary) = match document {
            None => {
                self.page_reflow.reset();
                (
                    build_empty_page_plan(width, page_height),
                    "empty-page full".to_owned(),
                )
            }
            Some(document) => {
                let output = self.page_reflow.update(ReflowRequest {
                    document,
                    width,
                    height: page_height,
                    invalidation,
                    force_full: force_page_reflow,
                });
                let dirty_count = output.dirty_regions.regions().len();
                let summary = format!(
                    "mode={:?} reason={:?} dirty_regions={} full={} commands={}->{}",
                    output.mode,
                    output.reason,
                    dirty_count,
                    output.dirty_regions.is_full_repaint(),
                    output.previous_command_count,
                    output.current_command_count
                );
                (output.paint_plan, summary)
            }
        };

        let page_background = page_plan.background;
        let mut commands = translate_page_commands(page_plan.commands, toolbar_height);
        let chrome_plan = build_chrome_paint_plan(chrome, width);
        commands.extend(chrome_plan.commands);

        (
            PaintPlan {
                background: page_background,
                commands,
            },
            summary,
        )
    }
}

#[allow(dead_code)]
fn build_viewport_paint_plan(
    document: Option<&present::RenderDocument>,
    chrome: &ChromeSnapshot,
    width: f32,
    height: f32,
) -> PaintPlan {
    let toolbar_height = TOOLBAR_HEIGHT.min(height.max(1.0));
    let page_height = (height - toolbar_height).max(1.0);

    let page_plan = document.map_or_else(
        || build_empty_page_plan(width, page_height),
        |document| present::build_paint_plan(document, width, page_height),
    );

    let page_background = page_plan.background;
    let mut commands = translate_page_commands(page_plan.commands, toolbar_height);
    let chrome_plan = build_chrome_paint_plan(chrome, width);
    commands.extend(chrome_plan.commands);

    PaintPlan {
        background: page_background,
        commands,
    }
}

fn build_empty_page_plan(width: f32, height: f32) -> PaintPlan {
    PaintPlan {
        background: EMPTY_PAGE_BACKGROUND,
        commands: vec![
            PaintCommand::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
                color: EMPTY_PAGE_BACKGROUND,
            },
            PaintCommand::TextPlaceholder {
                x: 32.0,
                y: 32.0,
                text: "Syphos is ready. Enter a URL above.".to_owned(),
                size: 18.0,
                color: EMPTY_PAGE_TEXT,
            },
        ],
    }
}

fn translate_page_commands(commands: Vec<PaintCommand>, y_offset: f32) -> Vec<PaintCommand> {
    commands
        .into_iter()
        .map(|command| match command {
            PaintCommand::Rect {
                x,
                y,
                width,
                height,
                color,
            } => PaintCommand::Rect {
                x,
                y: y + y_offset,
                width,
                height,
                color,
            },
            PaintCommand::TextPlaceholder {
                x,
                y,
                text,
                size,
                color,
            } => PaintCommand::TextPlaceholder {
                x,
                y: y + y_offset,
                text,
                size,
                color,
            },
            PaintCommand::Image {
                x,
                y,
                width,
                height,
                src,
                alt,
                background,
            } => PaintCommand::Image {
                x,
                y: y + y_offset,
                width,
                height,
                src,
                alt,
                background,
            },
            PaintCommand::SvgIcon {
                x,
                y,
                width,
                height,
                title,
                plan,
            } => PaintCommand::SvgIcon {
                x,
                y: y + y_offset,
                width,
                height,
                title,
                plan,
            },
        })
        .collect()
}

fn create_media_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("syphos_media_bind_group_layout"),
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
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn create_media_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    draw_mesh: &DrawMesh,
) -> wgpu::BindGroup {
    let font_texture = create_r8_texture(
        device,
        queue,
        "syphos_font_atlas_texture",
        draw_mesh.font_atlas.width.max(1),
        draw_mesh.font_atlas.height.max(1),
        &draw_mesh.font_atlas.pixels,
    );
    let image_texture = create_rgba_texture(
        device,
        queue,
        "syphos_image_atlas_texture",
        draw_mesh.image_atlas.width.max(1),
        draw_mesh.image_atlas.height.max(1),
        &draw_mesh.image_atlas.pixels,
    );

    let font_view = font_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let image_view = image_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("syphos_media_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("syphos_media_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&font_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&image_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

fn create_r8_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let texture_size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(width.max(1)),
            rows_per_image: Some(height.max(1)),
        },
        texture_size,
    );

    texture
}

fn create_rgba_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let texture_size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(width.max(1).saturating_mul(4)),
            rows_per_image: Some(height.max(1)),
        },
        texture_size,
    );

    texture
}

fn create_vertex_buffer(
    device: &wgpu::Device,
    size: wgpu::BufferAddress,
    label: &'static str,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(1),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn next_buffer_capacity(required_size: wgpu::BufferAddress) -> wgpu::BufferAddress {
    let mut capacity = INITIAL_VERTEX_BUFFER_BYTES;

    while capacity < required_size {
        capacity = capacity.saturating_mul(2);

        if capacity == wgpu::BufferAddress::MAX {
            break;
        }
    }

    capacity
}
