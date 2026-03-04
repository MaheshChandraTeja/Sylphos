use anyhow::{Context, Result};
use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing::{error, info};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

mod state;

#[derive(Debug, Parser)]
#[command(name = "myapp", about = "Fetch→Parse→Present demo")]
struct Cli {
    #[arg(long, default_value = "https://example.com")]
    url: String,
}

struct App {
    url: String,
    clear_rgba: Arc<Mutex<[f32; 4]>>,
    window: Option<Arc<Window>>,
    renderer: Option<state::State>,
    rt: tokio::runtime::Runtime,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        let win = el
            .create_window(
                Window::default_attributes().with_title("Phase 4 — Fetch → Parse → Present"),
            )
            .expect("create window");
        let window = Arc::new(win);

        let clear = self.clear_rgba.clone();
        let renderer =
            pollster::block_on(state::State::new(window.clone(), clear)).expect("renderer");

        let url = self.url.clone();
        let clear_for_task = self.clear_rgba.clone();
        self.rt.spawn(async move {
            if let Err(e) = fetch_parse_present(url, clear_for_task).await {
                error!(err = %e, "pipeline failed");
            }
        });

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let (Some(window), Some(renderer)) = (self.window.as_ref(), self.renderer.as_mut()) else {
            return;
        };
        if id != window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => renderer.resize(size),
            WindowEvent::ScaleFactorChanged {
                mut inner_size_writer,
                ..
            } => {
                let _ = inner_size_writer.request_inner_size(window.inner_size());
            }
            WindowEvent::RedrawRequested => match renderer.render() {
                Ok(()) => {}
                Err(wgpu::SurfaceError::Lost) => renderer.resize(window.inner_size()),
                Err(wgpu::SurfaceError::OutOfMemory) => el.exit(),
                Err(e) => error!(error = ?e, "render error"),
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    util::init_tracing();
    let cli = Cli::parse();
    info!(url = %cli.url, "starting");

    let event_loop = EventLoop::new()?;
    let clear_rgba = Arc::new(Mutex::new([0.10, 0.10, 0.15, 1.0]));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()
        .context("tokio runtime")?;

    let mut app = App {
        url: cli.url,
        clear_rgba,
        window: None,
        renderer: None,
        rt,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}

async fn fetch_parse_present(url: String, clear: Arc<Mutex<[f32; 4]>>) -> Result<()> {
    fetch::init(Default::default());
    let resp = fetch::get(&url).await?;
    info!(status = %resp.status(), "GET ok");

    let text = resp.get_text().await?;
    info!(bytes = text.len(), "downloaded");

    let doc = html_mvp::parse(&text)?;
    if let Some(content) = find_meta_theme_color(&doc) {
        if let Some(rgb) = parse_css_color_hex(&content) {
            *clear.lock().expect("clear lock") = [rgb.0, rgb.1, rgb.2, 1.0];
            info!(theme_color = %content, "updated clear color from meta");
        }
    }

    let normalized = html_mvp::serialize_document(&doc);
    let preview: String = normalized.chars().take(512).collect();
    info!(preview = %preview, "normalized html (first 512 chars)");
    Ok(())
}

fn find_meta_theme_color(doc: &html_mvp::Document) -> Option<String> {
    use html_mvp::Node;

    fn walk(nodes: &[Node]) -> Option<String> {
        for n in nodes {
            if let Node::Element(el) = n {
                if el.tag.eq_ignore_ascii_case("meta") {
                    let mut name: Option<&str> = None;
                    let mut content: Option<&str> = None;

                    for (k, v) in &el.attrs {
                        if k.eq_ignore_ascii_case("name") {
                            name = Some(v.as_str());
                        } else if k.eq_ignore_ascii_case("content") {
                            content = Some(v.as_str());
                        }
                    }

                    if matches!(name, Some(nm) if nm.eq_ignore_ascii_case("theme-color")) {
                        if let Some(c) = content {
                            return Some(c.trim().to_string());
                        }
                    }
                }

                if let Some(found) = walk(&el.children) {
                    return Some(found);
                }
            }
        }
        None
    }

    walk(&doc.children)
}

fn parse_css_color_hex(s: &str) -> Option<(f32, f32, f32)> {
    let t = s.trim();
    if !t.starts_with('#') {
        return None;
    }
    let hex = &t[1..];
    let (r, g, b) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b)
        }
        _ => return None,
    };
    Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
}
