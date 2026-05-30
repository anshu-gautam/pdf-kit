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

use clap::{Parser, Subcommand};
use pdfkit_core::{
    encode_png, extract, Engine, ExtractOptions, Mode, NativeRenderer, OpenOptions, PdfError,
    PdfInput, RenderOptions, Renderer,
};

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
    let input = read_input(&args.file)?;
    let password = resolve_password(&args.password, &args.password_file)?;

    let engine = Engine::new()?;
    let doc = engine.open(input, OpenOptions { password })?;
    let page = doc.page(args.page)?;

    let opts = RenderOptions {
        dpi: args.dpi,
        ..RenderOptions::default()
    };
    let bitmap = NativeRenderer.render(&page, &opts)?;
    let png = encode_png(&bitmap, true)?;

    match args.out {
        Some(path) => fs::write(&path, &png)?,
        None => {
            let stdout = io::stdout();
            stdout.lock().write_all(&png)?;
        }
    }
    Ok(())
}
