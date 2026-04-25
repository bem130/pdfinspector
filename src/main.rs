use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use pdfinspector::{DocumentReport, ObjectId, PdfDocument, PdfObject, analyze_document};

#[derive(Clone, Debug, Default)]
struct CliOptions {
    show_objects: bool,
    show_streams: bool,
    dump_stream: Option<ObjectId>,
    extract_dir: Option<PathBuf>,
    export_svg_dir: Option<PathBuf>,
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
            "--extract" => {
                let Some(value) = args.next() else {
                    return Err(format!("missing value for --extract\n{}", usage()));
                };
                options.extract_dir = Some(PathBuf::from(value));
            }
            "--export-svg" => {
                let Some(value) = args.next() else {
                    return Err(format!("missing value for --export-svg\n{}", usage()));
                };
                options.export_svg_dir = Some(PathBuf::from(value));
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
    print_report(Path::new(&path), &report, &options);
    if let Some(object_id) = options.dump_stream {
        dump_stream(&document, object_id)?;
    }
    if let Some(dir) = options.extract_dir.as_deref() {
        extract_document(&document, &report, dir)?;
    }
    if let Some(dir) = options.export_svg_dir.as_deref() {
        export_svg_pages(&document, &report, dir)?;
    }
    Ok(())
}

fn print_report(path: &Path, report: &DocumentReport, options: &CliOptions) {
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
    "usage: pdfinspector [--objects] [--streams] [--all] [--dump-stream <obj>] [--extract <dir>] [--export-svg <dir>] <file.pdf>"
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

fn extract_document(document: &PdfDocument, report: &DocumentReport, out_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;

    let streams_dir = out_dir.join("streams");
    let images_dir = out_dir.join("images");
    let fonts_dir = out_dir.join("fonts");
    let embedded_dir = out_dir.join("embedded_files");
    fs::create_dir_all(&streams_dir).map_err(|error| format!("failed to create {}: {error}", streams_dir.display()))?;
    fs::create_dir_all(&images_dir).map_err(|error| format!("failed to create {}: {error}", images_dir.display()))?;
    fs::create_dir_all(&fonts_dir).map_err(|error| format!("failed to create {}: {error}", fonts_dir.display()))?;
    fs::create_dir_all(&embedded_dir).map_err(|error| format!("failed to create {}: {error}", embedded_dir.display()))?;

    let font_streams = collect_font_streams(document);
    let embedded_file_streams: BTreeSet<ObjectId> = report.embedded_files.iter().map(|file| file.object_id).collect();

    for (id, object) in document.objects() {
        let Some(stream) = object.as_stream() else {
            continue;
        };

        let raw_path = streams_dir.join(format!("{}_{}.bin", id.0, id.1));
        fs::write(&raw_path, &stream.data)
            .map_err(|error| format!("failed to write {}: {error}", raw_path.display()))?;

        if let Ok(decoded) = stream.decode() {
            let decoded_path = streams_dir.join(format!("{}_{}.decoded.bin", id.0, id.1));
            fs::write(&decoded_path, decoded)
                .map_err(|error| format!("failed to write {}: {error}", decoded_path.display()))?;
        }

        let subtype_name = stream
            .dict
            .get(b"Subtype".as_slice())
            .and_then(|value| document.resolve(value).as_name());
        let type_name = stream
            .dict
            .get(b"Type".as_slice())
            .and_then(|value| document.resolve(value).as_name());

        if subtype_name == Some(b"Image".as_slice()) {
            let extension = image_extension(stream);
            let target = images_dir.join(format!("{}_{}.{}", id.0, id.1, extension));
            fs::write(&target, &stream.data)
                .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
        }

        if let Some(font_kind) = font_streams.get(id) {
            let extension = font_extension(font_kind, subtype_name);
            let target = fonts_dir.join(format!("{}_{}.{}", id.0, id.1, extension));
            fs::write(&target, &stream.data)
                .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
        }

        if embedded_file_streams.contains(id) || type_name == Some(b"EmbeddedFile".as_slice()) {
            let extension = embedded_file_extension(stream);
            let target = embedded_dir.join(format!("{}_{}.{}", id.0, id.1, extension));
            fs::write(&target, &stream.data)
                .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
        }
    }

    let manifest_path = out_dir.join("manifest.json");
    let manifest = build_manifest_json(report);
    fs::write(&manifest_path, manifest)
        .map_err(|error| format!("failed to write {}: {error}", manifest_path.display()))?;

    println!();
    println!("Extracted to: {}", out_dir.display());
    Ok(())
}

fn collect_font_streams(document: &PdfDocument) -> BTreeMap<ObjectId, &'static str> {
    let mut font_streams = BTreeMap::new();
    for (_, object) in document.objects() {
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let type_name = dict
            .get(b"Type".as_slice())
            .and_then(|value| document.resolve(value).as_name());
        if type_name != Some(b"FontDescriptor".as_slice()) {
            continue;
        }

        for (key, label) in [
            (b"FontFile".as_slice(), "fontfile"),
            (b"FontFile2".as_slice(), "fontfile2"),
            (b"FontFile3".as_slice(), "fontfile3"),
        ] {
            if let Some(PdfObject::Reference(id)) = dict.get(key) {
                font_streams.insert(*id, label);
            }
        }
    }
    font_streams
}

fn image_extension(stream: &pdfinspector::PdfStream) -> &'static str {
    let filters = stream.filters();
    match filters.first().map(|value| value.as_str()) {
        Some("DCTDecode") => "jpg",
        Some("JPXDecode") => "jp2",
        Some("JBIG2Decode") => "jb2",
        Some("CCITTFaxDecode") => "fax",
        Some("FlateDecode") | Some("Fl") => "flate",
        _ => "bin",
    }
}

fn font_extension(font_kind: &str, subtype_name: Option<&[u8]>) -> &'static str {
    match (font_kind, subtype_name) {
        ("fontfile2", _) => "ttf",
        ("fontfile3", Some(b"OpenType")) => "otf",
        ("fontfile3", Some(b"Type1C")) => "cff",
        ("fontfile3", Some(b"CIDFontType0C")) => "cff",
        ("fontfile", _) => "pfb",
        _ => "bin",
    }
}

fn embedded_file_extension(stream: &pdfinspector::PdfStream) -> String {
    stream
        .dict
        .get(b"Subtype".as_slice())
        .and_then(PdfObject::as_name)
        .and_then(|name| std::str::from_utf8(name).ok())
        .and_then(|name| name.rsplit('.').next())
        .filter(|ext| *ext != "unknown")
        .unwrap_or("bin")
        .to_string()
}

fn build_manifest_json(report: &DocumentReport) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"version\": {},\n", json_string_option(report.version.as_deref())));
    out.push_str(&format!("  \"object_count\": {},\n", report.object_count));
    out.push_str(&format!("  \"stream_count\": {},\n", report.total_streams));
    out.push_str(&format!("  \"page_count\": {},\n", report.pages.len()));
    out.push_str("  \"pages\": [\n");
    for (index, page) in report.pages.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"number\": {},\n", page.number));
        out.push_str(&format!("      \"object_id\": \"{} {}\",\n", page.object_id.0, page.object_id.1));
        out.push_str(&format!("      \"media_box\": {},\n", json_string_option(page.media_box.as_deref())));
        out.push_str(&format!("      \"text_present\": {},\n", if page.has_text { "true" } else { "false" }));
        out.push_str(&format!("      \"text_operators\": {},\n", page.text_operator_count));
        out.push_str(&format!("      \"graphics_operators\": {},\n", page.graphics_operator_count));
        out.push_str(&format!("      \"path_operators\": {},\n", page.path_operator_count));
        out.push_str(&format!("      \"paint_operators\": {},\n", page.paint_operator_count));
        out.push_str(&format!("      \"xobject_operators\": {},\n", page.xobject_operator_count));
        out.push_str(&format!("      \"state_operators\": {}\n", page.state_operator_count));
        out.push_str(if index + 1 == report.pages.len() { "    }\n" } else { "    },\n" });
    }
    out.push_str("  ],\n");
    out.push_str("  \"embedded_files\": [\n");
    for (index, file) in report.embedded_files.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"object_id\": \"{} {}\",\n", file.object_id.0, file.object_id.1));
        out.push_str(&format!("      \"filename\": {},\n", json_string_option(file.filename.as_deref())));
        out.push_str(&format!("      \"subtype\": {},\n", json_string_option(file.subtype.as_deref())));
        out.push_str(&format!("      \"raw_size\": {},\n", file.raw_size));
        out.push_str(&format!("      \"decoded_size\": {}\n", json_number_option(file.decoded_size)));
        out.push_str(if index + 1 == report.embedded_files.len() { "    }\n" } else { "    },\n" });
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

fn json_string_option(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape_json(value)),
        None => "null".to_string(),
    }
}

fn json_number_option(value: Option<usize>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn escape_json(input: &str) -> String {
    input
        .chars()
        .flat_map(|ch| match ch {
            '\\' => ['\\', '\\'].into_iter().collect::<Vec<_>>(),
            '"' => ['\\', '"'].into_iter().collect::<Vec<_>>(),
            '\n' => ['\\', 'n'].into_iter().collect::<Vec<_>>(),
            '\r' => ['\\', 'r'].into_iter().collect::<Vec<_>>(),
            '\t' => ['\\', 't'].into_iter().collect::<Vec<_>>(),
            _ => [ch].into_iter().collect::<Vec<_>>(),
        })
        .collect()
}

fn export_svg_pages(document: &PdfDocument, report: &DocumentReport, out_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;

    for page in &report.pages {
        let Some(page_object) = document.get(page.object_id) else {
            continue;
        };
        let Some((width, height)) = parse_media_box_size(page.media_box.as_deref()) else {
            continue;
        };

        let mut content = Vec::new();
        for stream in document.page_content_streams(page_object) {
            let decoded = match stream.decode() {
                Ok(decoded) => decoded,
                Err(_) => continue,
            };
            content.extend_from_slice(&decoded);
            content.push(b'\n');
        }

        let svg = content_stream_to_svg(&content, width, height);
        let path = out_dir.join(format!("page-{}.svg", page.number));
        fs::write(&path, svg).map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }

    println!();
    println!("SVG exported to: {}", out_dir.display());
    Ok(())
}

fn parse_media_box_size(media_box: Option<&str>) -> Option<(f64, f64)> {
    let media_box = media_box?;
    let values: Vec<f64> = media_box
        .split_whitespace()
        .filter_map(|value| value.parse::<f64>().ok())
        .collect();
    if values.len() != 4 {
        return None;
    }
    Some((values[2] - values[0], values[3] - values[1]))
}

fn content_stream_to_svg(bytes: &[u8], width: f64, height: f64) -> String {
    let mut renderer = SvgRenderer::new(width, height);
    let tokens = tokenize_content(bytes);
    for token in tokens {
        renderer.push_token(token);
    }
    renderer.finish()
}

fn tokenize_content(bytes: &[u8]) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = Vec::new();
    let mut in_literal = false;
    let mut escape = false;
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        if in_literal {
            token.push(byte);
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b')' {
                in_literal = false;
                tokens.push(String::from_utf8_lossy(&token).into_owned());
                token.clear();
            }
            i += 1;
            continue;
        }

        if byte == b'%' {
            if !token.is_empty() {
                tokens.push(String::from_utf8_lossy(&token).into_owned());
                token.clear();
            }
            while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            continue;
        }

        if byte.is_ascii_whitespace() {
            if !token.is_empty() {
                tokens.push(String::from_utf8_lossy(&token).into_owned());
                token.clear();
            }
            i += 1;
            continue;
        }

        if byte == b'(' {
            if !token.is_empty() {
                tokens.push(String::from_utf8_lossy(&token).into_owned());
                token.clear();
            }
            in_literal = true;
            token.push(byte);
            i += 1;
            continue;
        }

        token.push(byte);
        i += 1;
    }

    if !token.is_empty() {
        tokens.push(String::from_utf8_lossy(&token).into_owned());
    }
    tokens
}

#[derive(Clone)]
struct SvgGraphicsState {
    stroke: String,
    fill: String,
    line_width: f64,
    transform: [f64; 6],
}

impl Default for SvgGraphicsState {
    fn default() -> Self {
        Self {
            stroke: "#000000".to_string(),
            fill: "#000000".to_string(),
            line_width: 1.0,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
}

struct SvgRenderer {
    width: f64,
    height: f64,
    elements: Vec<String>,
    stack: Vec<SvgGraphicsState>,
    state: SvgGraphicsState,
    operands: Vec<String>,
    current_path: String,
}

impl SvgRenderer {
    fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            elements: Vec::new(),
            stack: Vec::new(),
            state: SvgGraphicsState::default(),
            operands: Vec::new(),
            current_path: String::new(),
        }
    }

    fn push_token(&mut self, token: String) {
        match token.as_str() {
            "q" => {
                self.stack.push(self.state.clone());
                self.operands.clear();
            }
            "Q" => {
                if let Some(state) = self.stack.pop() {
                    self.state = state;
                }
                self.operands.clear();
            }
            "w" => {
                if let Some(value) = self.last_number() {
                    self.state.line_width = value;
                }
                self.operands.clear();
            }
            "RG" => {
                if let Some(color) = self.rgb_color(3) {
                    self.state.stroke = color;
                }
                self.operands.clear();
            }
            "rg" => {
                if let Some(color) = self.rgb_color(3) {
                    self.state.fill = color;
                }
                self.operands.clear();
            }
            "G" => {
                if let Some(color) = self.gray_color() {
                    self.state.stroke = color;
                }
                self.operands.clear();
            }
            "g" => {
                if let Some(color) = self.gray_color() {
                    self.state.fill = color;
                }
                self.operands.clear();
            }
            "cm" => {
                if let Some(matrix) = self.take_numbers(6) {
                    self.state.transform = multiply_matrix(self.state.transform, [
                        matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5],
                    ]);
                }
                self.operands.clear();
            }
            "m" => {
                if let Some(values) = self.take_numbers(2) {
                    self.current_path.push_str(&format!("M {} {} ", values[0], values[1]));
                }
                self.operands.clear();
            }
            "l" => {
                if let Some(values) = self.take_numbers(2) {
                    self.current_path.push_str(&format!("L {} {} ", values[0], values[1]));
                }
                self.operands.clear();
            }
            "c" => {
                if let Some(values) = self.take_numbers(6) {
                    self.current_path.push_str(&format!(
                        "C {} {}, {} {}, {} {} ",
                        values[0], values[1], values[2], values[3], values[4], values[5]
                    ));
                }
                self.operands.clear();
            }
            "h" => {
                self.current_path.push_str("Z ");
                self.operands.clear();
            }
            "re" => {
                if let Some(values) = self.take_numbers(4) {
                    let x = values[0];
                    let y = values[1];
                    let w = values[2];
                    let h = values[3];
                    self.current_path.push_str(&format!(
                        "M {} {} L {} {} L {} {} L {} {} Z ",
                        x,
                        y,
                        x + w,
                        y,
                        x + w,
                        y + h,
                        x,
                        y + h
                    ));
                }
                self.operands.clear();
            }
            "S" => self.flush_path("none", true),
            "s" => {
                self.current_path.push_str("Z ");
                self.flush_path("none", true);
            }
            "f" | "F" | "f*" => self.flush_path(&self.state.fill.clone(), false),
            "B" | "B*" => self.flush_path(&self.state.fill.clone(), true),
            "b" | "b*" => {
                self.current_path.push_str("Z ");
                self.flush_path(&self.state.fill.clone(), true);
            }
            "n" => {
                self.current_path.clear();
                self.operands.clear();
            }
            _ => self.operands.push(token),
        }
    }

    fn finish(self) -> String {
        let body = if self.elements.is_empty() {
            "<!-- no SVG-convertible path operations found -->".to_string()
        } else {
            self.elements.join("\n")
        };
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\">\n  <g transform=\"matrix(1 0 0 -1 0 {h})\">\n{body}\n  </g>\n</svg>\n",
            w = self.width,
            h = self.height,
            body = indent_lines(&body, 4)
        )
    }

    fn flush_path(&mut self, fill: &str, stroke: bool) {
        if self.current_path.is_empty() {
            self.operands.clear();
            return;
        }

        let stroke_value = if stroke { self.state.stroke.as_str() } else { "none" };
        let transform = format!(
            "matrix({} {} {} {} {} {})",
            self.state.transform[0],
            self.state.transform[1],
            self.state.transform[2],
            self.state.transform[3],
            self.state.transform[4],
            self.state.transform[5]
        );
        self.elements.push(format!(
            "    <path d=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" transform=\"{}\" />",
            self.current_path.trim(),
            fill,
            stroke_value,
            self.state.line_width,
            transform
        ));
        self.current_path.clear();
        self.operands.clear();
    }

    fn gray_color(&self) -> Option<String> {
        let value = self.last_number()?;
        let component = channel_to_u8(value);
        Some(format!("#{0:02X}{0:02X}{0:02X}", component))
    }

    fn rgb_color(&self, count: usize) -> Option<String> {
        let values = self.last_numbers(count)?;
        Some(format!(
            "#{:02X}{:02X}{:02X}",
            channel_to_u8(values[0]),
            channel_to_u8(values[1]),
            channel_to_u8(values[2])
        ))
    }

    fn last_number(&self) -> Option<f64> {
        self.operands.last()?.parse::<f64>().ok()
    }

    fn take_numbers(&self, count: usize) -> Option<Vec<f64>> {
        self.last_numbers(count)
    }

    fn last_numbers(&self, count: usize) -> Option<Vec<f64>> {
        if self.operands.len() < count {
            return None;
        }
        self.operands[self.operands.len() - count..]
            .iter()
            .map(|value| value.parse::<f64>().ok())
            .collect()
    }
}

fn channel_to_u8(value: f64) -> u8 {
    let clamped = value.clamp(0.0, 1.0);
    (clamped * 255.0).round() as u8
}

fn multiply_matrix(left: [f64; 6], right: [f64; 6]) -> [f64; 6] {
    [
        left[0] * right[0] + left[2] * right[1],
        left[1] * right[0] + left[3] * right[1],
        left[0] * right[2] + left[2] * right[3],
        left[1] * right[2] + left[3] * right[3],
        left[0] * right[4] + left[2] * right[5] + left[4],
        left[1] * right[4] + left[3] * right[5] + left[5],
    ]
}

fn indent_lines(text: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
