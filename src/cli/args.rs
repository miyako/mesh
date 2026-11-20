//! CLI argument definitions (clap derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::camera::presets::ViewPreset;
use crate::render::options::Background;

#[derive(Parser)]
#[command(
    name = "mesh-thumb",
    version = crate::versions::APP_VERSION,
    about = "Headless STL/PLY thumbnail renderer (CPU, no GPU required)",
    long_about = "mesh-thumb renders STL (ascii/binary) and PLY (ascii/binary LE/BE) \
                  files to PNG thumbnails on the CPU. No display or GPU required.",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Whether the selected subcommand requested JSON output. Used so a
    /// failing command can still emit a structured JSON error envelope to
    /// stdout (in addition to the human message on stderr).
    pub fn wants_json(&self) -> bool {
        match &self.command {
            Command::Render(a) => a.json,
            Command::Inspect(a) => a.json,
            Command::Capabilities(a) => a.json,
            Command::Batch(a) => a.json,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Render one input file to a PNG thumbnail.
    Render(RenderArgs),
    /// Inspect a file: format, counts, bounds (no render).
    Inspect(InspectArgs),
    /// Report what this build supports (formats, backends, limits).
    Capabilities(CapabilitiesArgs),
    /// Render many files to a directory (per-file errors).
    Batch(BatchArgs),
}

/// Shared render options used by `render` and `batch`.
#[derive(Args, Debug, Clone)]
pub struct RenderOptionsArgs {
    /// Output side length (square image). Conflicts with --width/--height.
    #[arg(long, value_name = "N", conflicts_with_all = ["width", "height"])]
    pub size: Option<u32>,

    /// Output width in pixels.
    #[arg(long, value_name = "W")]
    pub width: Option<u32>,

    /// Output height in pixels.
    #[arg(long, value_name = "H")]
    pub height: Option<u32>,

    /// Camera view: iso (default), top, front, side.
    #[arg(long, value_name = "VIEW", default_value = "iso")]
    pub view: ViewPreset,

    /// Background: hex color (#RRGGBB) or `transparent`.
    #[arg(long, value_name = "COLOR", default_value = "#20242b")]
    pub background: Background,

    /// Supersampling scale (1=off .. 8). Higher = smoother edges,
    /// slower. Default 2.
    #[arg(long, value_name = "N", default_value_t = 2)]
    pub quality: u32,

    /// Manual point-splat radius in output pixels (point clouds only).
    /// Omitted = automatic sizing.
    #[arg(long, value_name = "PX")]
    pub point_size: Option<f32>,

    /// Render backend: auto (default) or cpu. `gpu` fails clearly.
    #[arg(long, value_name = "BACKEND", default_value = "auto")]
    pub backend: crate::render::cpu::Backend,
}

impl RenderOptionsArgs {
    pub fn to_options(&self) -> Result<crate::render::options::RenderOptions, String> {
        let (width, height) = match (self.size, self.width, self.height) {
            (Some(s), None, None) => (s, s),
            (None, Some(w), Some(h)) => (w, h),
            (None, Some(w), None) => (w, w),
            (None, None, Some(h)) => (h, h),
            (Some(_), _, _) => {
                return Err("--size conflicts with --width/--height".to_string());
            }
            (None, None, None) => (256, 256),
        };
        Ok(crate::render::options::RenderOptions {
            width,
            height,
            view: self.view,
            background: self.background,
            quality: self.quality,
            point: crate::render::options::PointRender {
                point_size: self.point_size,
            },
            backend: self.backend,
        })
    }
}

/// Options common to commands that read a single input file.
#[derive(Args, Debug, Clone)]
pub struct InputArgs {
    /// Input file (STL or PLY).
    #[arg(value_name = "FILE")]
    pub input: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct RenderArgs {
    #[command(flatten)]
    pub input: InputArgs,

    /// Output PNG path. Default: input path with `.png` extension.
    #[arg(short, long, value_name = "PNG")]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub render: RenderOptionsArgs,

    /// Cache directory. Enables the content-addressed PNG cache.
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<PathBuf>,

    /// Disable the cache even if --cache-dir is given.
    #[arg(long)]
    pub no_cache: bool,

    /// Emit JSON to stdout (stable schema; see docs/protocol.md).
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    #[command(flatten)]
    pub input: InputArgs,

    /// Emit JSON to stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CapabilitiesArgs {
    /// Emit JSON to stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct BatchArgs {
    /// Input files (STL or PLY), repeatable.
    #[arg(required = true, value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Output directory for PNGs (created if missing).
    #[arg(short, long, value_name = "DIR", required = true)]
    pub out: PathBuf,

    #[command(flatten)]
    pub render: RenderOptionsArgs,

    /// Cache directory (shared by all items).
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<PathBuf>,

    /// Disable the cache.
    #[arg(long)]
    pub no_cache: bool,

    /// Stop at the first failing file instead of skipping (default:
    /// per-file errors are reported and the batch continues).
    #[arg(long)]
    pub abort_on_error: bool,

    /// Emit JSON result for the whole batch.
    #[arg(long)]
    pub json: bool,
}
