//! Tester support: a log file, panic logging, system info and issue links.
//!
//! Release builds on Windows have no console, so without a log file there
//! is nothing to attach to a bug report. Logs go to the per-user data
//! directory (`%LOCALAPPDATA%\OxyBrush\logs` on Windows,
//! `~/.local/share/oxybrush/logs` on Linux, inside `~/.var/app/…` for the
//! Flatpak). Each launch starts `oxybrush.log`; the previous run is kept as
//! `oxybrush.previous.log` so a crash log survives the restart.

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

pub const REPO_URL: &str = "https://github.com/crisprintsstuff/OxyBrush";

pub fn log_dir() -> Option<PathBuf> {
    let app = if cfg!(windows) { "OxyBrush" } else { "oxybrush" };
    Some(dirs::data_local_dir()?.join(app).join("logs"))
}

/// Writes every log line to stderr and (if available) the log file.
struct Tee {
    file: Option<File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Some(f) = &mut self.file {
            // Logging must never take the app down: ignore file errors.
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
        Ok(())
    }
}

/// Set up logging to stderr + log file and log panics. Returns the log file path.
pub fn init_logging() -> Option<PathBuf> {
    let path = log_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("oxybrush.log");
        if path.exists() {
            let _ = std::fs::rename(&path, dir.join("oxybrush.previous.log"));
        }
        Some(path)
    });
    let file = path.as_ref().and_then(|p| File::create(p).ok());
    let path = if file.is_some() { path } else { None };

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .target(env_logger::Target::Pipe(Box::new(Tee { file })))
    .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("PANIC: {info}\n{}", std::backtrace::Backtrace::force_capture());
        log::logger().flush();
        default_hook(info);
    }));

    log::info!("OxyBrush {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    if let Some(p) = &path {
        log::info!("log file: {}", p.display());
    }
    path
}

/// What a bug report needs to know about this machine.
#[derive(Debug, Clone, Default)]
pub struct SystemInfo {
    pub gpu: Option<String>,
    pub gpu_backend: Option<String>,
    pub gpu_driver: Option<String>,
    pub exact_face_colors: Option<bool>,
    pub log_path: Option<PathBuf>,
}

impl SystemInfo {
    pub fn os() -> String {
        let flatpak = std::env::var_os("FLATPAK_ID").is_some();
        format!("{} {}{}", std::env::consts::OS, std::env::consts::ARCH, if flatpak { " (Flatpak)" } else { "" })
    }

    pub fn gpu_summary(&self) -> String {
        match (&self.gpu, &self.gpu_backend) {
            (Some(g), Some(b)) => format!("{g} ({b})"),
            (Some(g), None) => g.clone(),
            _ => "unknown".into(),
        }
    }

    /// Plain-text block to paste into an issue.
    pub fn text(&self) -> String {
        let mut s =
            format!("OxyBrush: {}\nOS: {}\nGPU: {}\n", env!("CARGO_PKG_VERSION"), Self::os(), self.gpu_summary());
        if let Some(d) = &self.gpu_driver {
            s.push_str(&format!("GPU driver: {d}\n"));
        }
        if let Some(e) = self.exact_face_colors {
            s.push_str(&format!("Exact face-group colors: {e}\n"));
        }
        if let Some(p) = &self.log_path {
            s.push_str(&format!("Log file: {}\n", p.display()));
        }
        s
    }

    /// New-issue URL for the bug report form, with version/OS/GPU pre-filled.
    pub fn bug_report_url(&self) -> String {
        format!(
            "{REPO_URL}/issues/new?template=bug_report.yml&version={}&os={}&gpu={}",
            encode(env!("CARGO_PKG_VERSION")),
            encode(&Self::os()),
            encode(&self.gpu_summary())
        )
    }

    pub fn feedback_url(&self) -> String {
        format!("{REPO_URL}/issues/new?template=feedback.yml&version={}", encode(env!("CARGO_PKG_VERSION")))
    }
}

/// Percent-encode a query-string value (RFC 3986 unreserved characters kept).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Load a PNG as a brush alpha (luminance; transparent pixels count as black).
pub fn load_alpha_png(path: &std::path::Path) -> Result<oxy_sculpt::Alpha, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new_with_limits(io::BufReader::new(file), png::Limits { bytes: 256 << 20 });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let luma: Vec<u8> = match info.color_type {
        png::ColorType::Grayscale => px.to_vec(),
        png::ColorType::GrayscaleAlpha => {
            px.as_chunks::<2>().0.iter().map(|p| ((p[0] as u16 * p[1] as u16) / 255) as u8).collect()
        }
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().map(|p| luminance(p)).collect(),
        png::ColorType::Rgba => {
            px.as_chunks::<4>().0.iter().map(|p| ((luminance(p) as u16 * p[3] as u16) / 255) as u8).collect()
        }
        other => return Err(format!("unsupported PNG color type {other:?}")),
    };
    let name = path.file_stem().map_or("Alpha".into(), |s| s.to_string_lossy().into_owned());
    oxy_sculpt::Alpha::from_luma(name, w, h, &luma).ok_or_else(|| "empty image".to_string())
}

fn luminance(p: &[u8]) -> u8 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8
}

/// `file://` URL for a local folder (opened by the OS file manager).
pub fn folder_url(path: &std::path::Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("linux x86_64 (Flatpak)"), "linux%20x86_64%20%28Flatpak%29");
        assert_eq!(encode("0.0.1-alpha"), "0.0.1-alpha");
        assert_eq!(encode("é"), "%C3%A9");
    }

    #[test]
    fn report_url_prefills_fields() {
        let info =
            SystemInfo { gpu: Some("Test GPU".into()), gpu_backend: Some("Vulkan".into()), ..Default::default() };
        let url = info.bug_report_url();
        assert!(url.starts_with("https://github.com/crisprintsstuff/OxyBrush/issues/new?template=bug_report.yml"));
        assert!(url.contains("&gpu=Test%20GPU%20%28Vulkan%29"));
        assert!(info.text().contains("GPU: Test GPU (Vulkan)"));
    }

    #[test]
    fn loads_png_alphas() {
        let dir = std::env::temp_dir().join(format!("oxy-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // RGBA gradient, left black to right white, fully opaque.
        let (w, h) = (32u32, 16u32);
        let mut px = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let v = (x * 255 / (w - 1)) as u8;
                px.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let path = dir.join("grad.png");
        {
            let f = File::create(&path).unwrap();
            let mut enc = png::Encoder::new(io::BufWriter::new(f), w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&px).unwrap();
        }
        let a = load_alpha_png(&path).unwrap();
        assert_eq!(a.name, "grad");
        assert!(a.sample(glam::Vec2::new(-0.95, 0.0)) < 0.1);
        assert!(a.sample(glam::Vec2::new(0.95, 0.0)) > 0.9);
        std::fs::write(dir.join("bad.png"), b"not a png").unwrap();
        assert!(load_alpha_png(&dir.join("bad.png")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn folder_urls() {
        assert_eq!(folder_url(std::path::Path::new("/home/a/logs")), "file:///home/a/logs");
        assert_eq!(folder_url(std::path::Path::new("C:\\Users\\a\\logs")), "file:///C:/Users/a/logs");
    }
}
