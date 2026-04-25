use std::env;
use std::path::Path;

use pdfinspector::{DocumentReport, ObjectId, PdfDocument, analyze_document};

#[derive(Clone, Copy, Debug, Default)]
struct CliOptions {
    show_objects: bool,
    show_streams: bool,
    dump_stream: Option<ObjectId>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut options = CliOptions::default();
    let mut path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--objects" => options.show_objects = true,
            "--streams" => options.show_streams = true,
            "--all" => {
                options.show_objects = true;
                options.show_streams = true;
            }
            "--dump-stream" => {
                let Some(value) = args.next() else {
                    return Err(format!("missing value for --dump-stream\n{}", usage()));
                };
                options.dump_stream = Some(parse_object_id(&value)?);
            }
            "--help" | "-h" => return Err(usage().to_string()),
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}\n{}", usage()));
            }
            _ if path.is_none() => path = Some(arg),
            _ => return Err(usage().to_string()),
        }
    }

    let Some(path) = path else {
        return Err(usage().to_string());
    };

    let bytes = std::fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let document = PdfDocument::parse(&bytes)?;
    let report = analyze_document(&document);
    print_report(Path::new(&path), &report, options);
    if let Some(object_id) = options.dump_stream {
        dump_stream(&document, object_id)?;
    }
    Ok(())
}

fn print_report(path: &Path, report: &DocumentReport, options: CliOptions) {
    println!("File: {}", path.display());
    println!("PDF Version: {}", report.version.as_deref().unwrap_or("unknown"));
    println!("Objects: {}", report.object_count);
    println!("Streams: {}", report.total_streams);
    println!("Pages: {}", report.pages.len());
    println!("Text pages: {}", report.text_page_count);
    println!("Image XObjects: {}", report.total_images);
    println!("Fonts: {}", report.total_fonts);
    println!("Embedded fonts: {}", report.embedded_fonts);
    println!("Embedded files: {}", report.embedded_files.len());

    if !report.warnings.is_empty() {
        println!();
        println!("Warnings:");
        for warning in &report.warnings {
            println!("  - {warning}");
        }
    }

    for page in &report.pages {
        println!();
        println!("Page {}:", page.number);
        println!("  Object: {} {}", page.object_id.0, page.object_id.1);
        println!("  MediaBox: {}", page.media_box.as_deref().unwrap_or("unknown"));
        println!("  Text operators: {}", page.text_operator_count);
        println!("  Graphics operators: {}", page.graphics_operator_count);
        println!("  Path operators: {}", page.path_operator_count);
        println!("  Paint operators: {}", page.paint_operator_count);
        println!("  XObject operators: {}", page.xobject_operator_count);
        println!("  State operators: {}", page.state_operator_count);
        println!(
            "  Text present: {}",
            if page.has_text { "yes" } else { "no" }
        );
        println!(
            "  Content filters: {}",
            join_or_none(&page.content_filters)
        );

        println!("  Fonts:");
        if page.fonts.is_empty() {
            println!("    (none)");
        } else {
            for font in &page.fonts {
                let embedded = if font.embedded { "embedded" } else { "not embedded" };
                let subtype = font.subtype.as_deref().unwrap_or("unknown");
                println!(
                    "    - {}: base={}, subtype={}, {}",
                    font.name,
                    font.base_font.as_deref().unwrap_or("unknown"),
                    subtype,
                    embedded
                );
            }
        }

        println!("  Images:");
        if page.images.is_empty() {
            println!("    (none)");
        } else {
            for image in &page.images {
                println!(
                    "    - {}: {}x{}, color={}, bpc={}, filters={}",
                    image.name,
                    image.width.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string()),
                    image.height.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string()),
                    image.color_space.as_deref().unwrap_or("unknown"),
                    image
                        .bits_per_component
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    join_or_none(&image.filters)
                );
            }
        }
    }

    if !report.embedded_files.is_empty() {
        println!();
        println!("Embedded Files:");
        for file in &report.embedded_files {
            println!(
                "  - {} {}: name={}, subtype={}, raw={}, decoded={}, declared={}, filters={}",
                file.object_id.0,
                file.object_id.1,
                file.filename.as_deref().unwrap_or("unknown"),
                file.subtype.as_deref().unwrap_or("unknown"),
                file.raw_size,
                file.decoded_size
                    .map(|size| size.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                file.declared_size
                    .map(|size| size.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                join_or_none(&file.filters)
            );
        }
    }

    if options.show_streams {
        println!();
        println!("Streams:");
        for stream in &report.streams {
            println!(
                "  - {} {}: role={}, type={}, subtype={}, raw={}, decoded={}, filters={}",
                stream.object_id.0,
                stream.object_id.1,
                stream.role,
                stream.type_name.as_deref().unwrap_or("unknown"),
                stream.subtype_name.as_deref().unwrap_or("unknown"),
                stream.raw_size,
                stream
                    .decoded_size
                    .map(|size| size.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                join_or_none(&stream.filters)
            );
        }
    }

    if options.show_objects {
        println!();
        println!("Objects:");
        for object in &report.objects {
            println!(
                "  - {} {}: kind={}, type={}, subtype={}, stream={}, raw_size={}, filters={}",
                object.object_id.0,
                object.object_id.1,
                object.object_kind,
                object.type_name.as_deref().unwrap_or("unknown"),
                object.subtype_name.as_deref().unwrap_or("unknown"),
                if object.has_stream { "yes" } else { "no" },
                object
                    .raw_stream_size
                    .map(|size| size.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                join_or_none(&object.filters)
            );
        }
    }
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn usage() -> &'static str {
    "usage: pdfinspector [--objects] [--streams] [--all] [--dump-stream <obj>] <file.pdf>"
}

fn parse_object_id(value: &str) -> Result<ObjectId, String> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_suffix('R').unwrap_or(trimmed).trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    match parts.as_slice() {
        [object] => {
            let object = object
                .parse::<u32>()
                .map_err(|_| format!("invalid object id: {value}"))?;
            Ok(ObjectId(object, 0))
        }
        [object, generation] => {
            let object = object
                .parse::<u32>()
                .map_err(|_| format!("invalid object id: {value}"))?;
            let generation = generation
                .parse::<u16>()
                .map_err(|_| format!("invalid generation in object id: {value}"))?;
            Ok(ObjectId(object, generation))
        }
        _ => Err(format!("invalid object id: {value}")),
    }
}

fn dump_stream(document: &PdfDocument, object_id: ObjectId) -> Result<(), String> {
    let object = document
        .get(object_id)
        .ok_or_else(|| format!("stream object {} {} not found", object_id.0, object_id.1))?;
    let stream = object
        .as_stream()
        .ok_or_else(|| format!("object {} {} is not a stream", object_id.0, object_id.1))?;
    let decoded = stream.decode()?;

    println!();
    println!("Decoded Stream {} {}:", object_id.0, object_id.1);
    match std::str::from_utf8(&decoded) {
        Ok(text) => print!("{text}"),
        Err(_) => {
            for byte in decoded {
                print!("{byte:02X} ");
            }
            println!();
        }
    }
    Ok(())
}
