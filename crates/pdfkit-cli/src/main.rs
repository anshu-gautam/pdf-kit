//! `pdfkit` command-line interface (PRD §9 / M9).
//!
//! - `pdfkit <file>`: extract text (default).
//! - `pdfkit <file> --json`: extract result as JSON.
//! - `pdfkit render <file> --page N [-o out.png]`: render a page to PNG.
//! - `pdfkit chunk <file> --format json|md|text`: structured RAG chunks.
//! - `--password` / `--password-file`, `-` for stdin, and sensible exit codes.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use pdfkit_chunk::{chunk_document, document_text, to_markdown, ChunkOptions};
use pdfkit_core::{
    encode_png, extract, Document, Engine, ExtractOptions, Mode, NativeRenderer, OpenOptions,
    OutlineItem, PdfError, PdfInput, RenderOptions, Renderer, StructNode,
};
use serde_json::{json, Value};

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
    /// Emit structured RAG chunks as JSON, Markdown, or plain reading-order text.
    Chunk(ChunkArgs),
    /// Print the document outline (bookmarks / table of contents) as JSON.
    Outline(DocArgs),
    /// Print the tagged-PDF logical structure tree as JSON (or `{"tagged":false}`).
    Structure(DocArgs),
    /// Print image/figure regions (bbox + caption) per page as JSON.
    Figures(DocArgs),
    /// Convert a Word .docx document to PDF (pure-Rust, offline).
    Convert(ConvertArgs),
}

/// Args for `convert` (docx → PDF).
#[derive(Parser, Debug)]
struct ConvertArgs {
    /// Word .docx file to convert (or `-` for stdin).
    file: String,

    /// Output PDF path (defaults to stdout).
    #[arg(long, short = 'o', value_name = "PATH")]
    out: Option<PathBuf>,
}

/// Args shared by the read-only inspection subcommands.
#[derive(Parser, Debug)]
struct DocArgs {
    /// PDF file to inspect (or `-` for stdin).
    file: String,

    /// Password for an encrypted document.
    #[arg(long)]
    password: Option<String>,

    /// Read the password from a file.
    #[arg(long, value_name = "PATH")]
    password_file: Option<PathBuf>,
}

/// Output format for `chunk`.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum ChunkFormat {
    /// Lossless JSON array of chunks (id, text, page, bbox, kind, heading_path,
    /// char span, token estimate).
    Json,
    /// Human-readable GitHub-flavored Markdown.
    Md,
    /// Plain reading-order text (the chunks joined; char spans index into it).
    Text,
}

#[derive(Parser, Debug)]
struct ChunkArgs {
    /// PDF file to chunk (or `-` for stdin).
    file: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = ChunkFormat::Json)]
    format: ChunkFormat,

    /// Target chunk size in tokens.
    #[arg(long, default_value_t = 512)]
    target_tokens: usize,

    /// Token overlap carried across a budget split (0 = none).
    #[arg(long, default_value_t = 0)]
    overlap_tokens: usize,

    /// Add a situating context prefix (title + heading path + page) to each chunk.
    #[arg(long)]
    context: bool,

    /// Password for an encrypted document.
    #[arg(long)]
    password: Option<String>,

    /// Read the password from a file.
    #[arg(long, value_name = "PATH")]
    password_file: Option<PathBuf>,
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
    match cli.command.take() {
        Some(Command::Render(args)) => return run_render(args),
        Some(Command::Chunk(args)) => return run_chunk(args),
        Some(Command::Outline(args)) => return run_outline(args),
        Some(Command::Structure(args)) => return run_structure(args),
        Some(Command::Figures(args)) => return run_figures(args),
        Some(Command::Convert(args)) => return run_convert(args),
        None => {}
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

fn run_chunk(args: ChunkArgs) -> Result<(), CliError> {
    let input = read_input(&args.file)?;
    let password = resolve_password(&args.password, &args.password_file)?;
    let doc = Engine::new()?.open(input, OpenOptions { password })?;

    let opts = ChunkOptions {
        target_tokens: args.target_tokens,
        overlap_tokens: args.overlap_tokens,
        contextual_prefix: args.context,
        ..ChunkOptions::default()
    };
    let chunks = chunk_document(&doc, &opts)?;

    let rendered = match args.format {
        ChunkFormat::Json => pdfkit_chunk::to_json(&chunks)?,
        ChunkFormat::Md => to_markdown(&chunks),
        ChunkFormat::Text => document_text(&chunks),
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{rendered}")?;
    Ok(())
}

/// Open a document for a read-only inspection subcommand.
fn open_doc(args: &DocArgs) -> Result<Document, CliError> {
    let input = read_input(&args.file)?;
    let password = resolve_password(&args.password, &args.password_file)?;
    Ok(Engine::new()?.open(input, OpenOptions { password })?)
}

/// Print a JSON value (pretty) to stdout.
fn print_json(value: &Value) -> Result<(), CliError> {
    let rendered = serde_json::to_string_pretty(value)
        .map_err(|e| CliError::Pdf(PdfError::Backend(format!("json: {e}"))))?;
    writeln!(io::stdout().lock(), "{rendered}")?;
    Ok(())
}

fn outline_json(item: &OutlineItem) -> Value {
    json!({
        "title": item.title,
        "page": item.page,
        "children": item.children.iter().map(outline_json).collect::<Vec<_>>(),
    })
}

fn run_outline(args: DocArgs) -> Result<(), CliError> {
    let doc = open_doc(&args)?;
    let outline: Vec<Value> = doc.outline().iter().map(outline_json).collect();
    print_json(&Value::Array(outline))
}

fn structure_json(node: &StructNode) -> Value {
    json!({
        "tag": node.tag,
        "raw_tag": node.raw_tag,
        "text": node.text,
        "alt": node.alt,
        "page": node.page,
        "children": node.children.iter().map(structure_json).collect::<Vec<_>>(),
    })
}

fn run_structure(args: DocArgs) -> Result<(), CliError> {
    let doc = open_doc(&args)?;
    let value = match doc.structure_tree() {
        Some(root) => structure_json(&root),
        None => json!({ "tagged": false }),
    };
    print_json(&value)
}

fn run_figures(args: DocArgs) -> Result<(), CliError> {
    let doc = open_doc(&args)?;
    let mut figures = Vec::new();
    for page in 1..=doc.page_count() {
        for region in doc.page(page)?.image_regions() {
            figures.push(json!({
                "page": page,
                "bbox": region.bbox,
                "caption": region.caption,
            }));
        }
    }
    print_json(&Value::Array(figures))
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

/// Convert a Word `.docx` to PDF and write it to `--out` or stdout.
fn run_convert(args: ConvertArgs) -> Result<(), CliError> {
    let docx = read_bytes(&args.file)?;
    let pdf = pdfkit_docx::docx_to_pdf(&docx)?;
    match args.out {
        Some(path) => fs::write(&path, &pdf)?,
        None => {
            let stdout = io::stdout();
            stdout.lock().write_all(&pdf)?;
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
