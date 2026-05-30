//! `pdfkit` command-line interface (PRD §9 / M9).
//!
//! - `pdfkit <file>`: extract text (default).
//! - `pdfkit <file> --json`: extract result as JSON.
//! - `pdfkit render <file> --page N [-o out.png]`: render a page to PNG.
//! - `--password` / `--password-file`, `-` for stdin, and sensible exit codes.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use pdfkit_core::{
    encode_png, extract, Engine, ExtractOptions, Mode, NativeRenderer, OpenOptions, PdfError,
    PdfInput, RenderOptions, Renderer,
};

/// Which rendering backend to use for `render`.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum Backend {
    /// Use PDFIUM if available (and compiled in), else the native path.
    Auto,
    /// Pure-Rust path: sizes the page and composites raster images only.
    Native,
    /// High-fidelity PDFIUM (renders text + vector); needs the render-pdfium build.
    Pdfium,
}

/// AI-oriented PDF toolkit: read-first extraction, with a separate render path.
#[derive(Parser, Debug)]
#[command(name = "pdfkit", version, about)]
struct Cli {
    /// PDF file to extract from (or `-` for stdin) when no subcommand is given.
    file: Option<String>,

    /// Emit the extraction result as JSON.
    #[arg(long)]
    json: bool,

    /// Password for an encrypted document.
    #[arg(long)]
    password: Option<String>,

    /// Read the password from a file (whitespace-trimmed).
    #[arg(long, value_name = "PATH")]
    password_file: Option<PathBuf>,

    /// Maximum number of pages to process.
    #[arg(long)]
    max_pages: Option<usize>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Render a single page to a PNG.
    Render(RenderArgs),
}

#[derive(Parser, Debug)]
struct RenderArgs {
    /// PDF file to render (or `-` for stdin).
    file: String,

    /// One-based page number to render.
    #[arg(long, default_value_t = 1)]
    page: usize,

    /// Output PNG path (defaults to stdout).
    #[arg(long, short = 'o', value_name = "PATH")]
    out: Option<PathBuf>,

    /// Render resolution in DPI.
    #[arg(long)]
    dpi: Option<f32>,

    /// Rendering backend.
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    backend: Backend,

    /// Password for an encrypted document.
    #[arg(long)]
    password: Option<String>,

    /// Read the password from a file.
    #[arg(long, value_name = "PATH")]
    password_file: Option<PathBuf>,
}

/// CLI error with an associated process exit code.
#[derive(Debug)]
enum CliError {
    Pdf(PdfError),
    Io(io::Error),
    Usage(String),
}

impl CliError {
    fn exit_code(&self) -> i32 {
        match self {
            CliError::Pdf(PdfError::Password) => 3,
            CliError::Pdf(_) => 1,
            CliError::Io(_) => 1,
            CliError::Usage(_) => 2,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Pdf(e) => write!(f, "{e}"),
            CliError::Io(e) => write!(f, "{e}"),
            CliError::Usage(m) => write!(f, "{m}"),
        }
    }
}

impl From<PdfError> for CliError {
    fn from(e: PdfError) -> Self {
        CliError::Pdf(e)
    }
}

impl From<io::Error> for CliError {
    fn from(e: io::Error) -> Self {
        CliError::Io(e)
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match run(cli) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("pdfkit: error: {err}");
            err.exit_code()
        }
    };
    std::process::exit(code);
}

fn run(mut cli: Cli) -> Result<(), CliError> {
    if let Some(Command::Render(args)) = cli.command.take() {
        return run_render(args);
    }
    let file = cli
        .file
        .clone()
        .ok_or_else(|| CliError::Usage("a file argument or a subcommand is required".into()))?;
    run_extract(&file, &cli)
}

/// Read a PDF input from a path or stdin (`-`).
fn read_input(file: &str) -> Result<PdfInput, CliError> {
    if file == "-" {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(PdfInput::Bytes(buf))
    } else {
        Ok(PdfInput::Path(PathBuf::from(file)))
    }
}

/// Resolve a password from `--password` or `--password-file`.
fn resolve_password(
    password: &Option<String>,
    password_file: &Option<PathBuf>,
) -> Result<Option<String>, CliError> {
    if let Some(p) = password {
        return Ok(Some(p.clone()));
    }
    if let Some(path) = password_file {
        let contents = fs::read_to_string(path)?;
        return Ok(Some(contents.trim().to_string()));
    }
    Ok(None)
}

fn run_extract(file: &str, cli: &Cli) -> Result<(), CliError> {
    let input = read_input(file)?;
    let password = resolve_password(&cli.password, &cli.password_file)?;

    let mut opts = ExtractOptions {
        mode: Mode::Text,
        password,
        ..ExtractOptions::default()
    };
    if let Some(max) = cli.max_pages {
        opts.max_pages = max;
    }

    let result = extract(input, opts)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if cli.json {
        let json = serde_json::json!({
            "text": result.text,
            "pages_processed": result.pages_processed,
            "images": result
                .images
                .iter()
                .map(|i| serde_json::json!({
                    "page": i.page,
                    "width": i.width,
                    "height": i.height,
                    "png_bytes": i.png.len(),
                }))
                .collect::<Vec<_>>(),
            "truncated": {
                "text": result.truncated.text,
                "images": result.truncated.images,
            },
        });
        let rendered = serde_json::to_string_pretty(&json)
            .map_err(|e| CliError::Pdf(PdfError::Backend(format!("json: {e}"))))?;
        writeln!(out, "{rendered}")?;
    } else {
        writeln!(out, "{}", result.text)?;
    }
    Ok(())
}

fn run_render(args: RenderArgs) -> Result<(), CliError> {
    let bytes = read_bytes(&args.file)?;
    let password = resolve_password(&args.password, &args.password_file)?;
    let opts = RenderOptions {
        dpi: args.dpi,
        ..RenderOptions::default()
    };

    let png = render_to_png(&bytes, args.page, password.as_deref(), &opts, args.backend)?;

    match args.out {
        Some(path) => fs::write(&path, &png)?,
        None => {
            let stdout = io::stdout();
            stdout.lock().write_all(&png)?;
        }
    }
    Ok(())
}

/// Read the whole document into memory (PDFIUM and the native fallback both want
/// bytes; `-` reads stdin).
fn read_bytes(file: &str) -> Result<Vec<u8>, CliError> {
    if file == "-" {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        Ok(fs::read(file)?)
    }
}

/// Render a page to PNG with the chosen backend.
fn render_to_png(
    bytes: &[u8],
    page: usize,
    password: Option<&str>,
    opts: &RenderOptions,
    backend: Backend,
) -> Result<Vec<u8>, CliError> {
    let want_pdfium = match backend {
        Backend::Native => false,
        Backend::Pdfium => true,
        Backend::Auto => cfg!(feature = "render-pdfium"),
    };

    if want_pdfium {
        #[cfg(feature = "render-pdfium")]
        {
            match pdfkit_render::PdfiumRenderer::new() {
                Ok(renderer) => {
                    let bitmap = renderer.render_page(bytes, page, password, opts)?;
                    return Ok(encode_png(&bitmap, true)?);
                }
                Err(e) if backend == Backend::Pdfium => return Err(CliError::Pdf(e)),
                Err(e) => eprintln!("pdfkit: PDFIUM unavailable ({e}); falling back to native"),
            }
        }
        #[cfg(not(feature = "render-pdfium"))]
        if backend == Backend::Pdfium {
            return Err(CliError::Usage(
                "this build has no PDFIUM backend; rebuild with --features render-pdfium".into(),
            ));
        }
    }

    // Native fallback (pure-Rust: background + composited raster images).
    let doc = Engine::new()?.open(
        bytes.to_vec(),
        OpenOptions {
            password: password.map(str::to_string),
        },
    )?;
    let view = doc.page(page)?;
    let bitmap = NativeRenderer.render(&view, opts)?;
    Ok(encode_png(&bitmap, true)?)
}
