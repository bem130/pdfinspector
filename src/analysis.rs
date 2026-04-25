use alloc::borrow::ToOwned;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::pdf::{Dict, ObjectId, PdfDocument, PdfObject, PdfStream};

#[derive(Debug)]
pub struct DocumentReport {
    pub version: Option<String>,
    pub object_count: usize,
    pub total_streams: usize,
    pub pages: Vec<PageReport>,
    pub text_page_count: usize,
    pub total_images: usize,
    pub total_fonts: usize,
    pub embedded_fonts: usize,
    pub embedded_files: Vec<EmbeddedFileReport>,
    pub objects: Vec<ObjectReport>,
    pub streams: Vec<StreamReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct PageReport {
    pub number: usize,
    pub object_id: ObjectId,
    pub media_box: Option<String>,
    pub has_text: bool,
    pub text_operator_count: usize,
    pub graphics_operator_count: usize,
    pub path_operator_count: usize,
    pub paint_operator_count: usize,
    pub xobject_operator_count: usize,
    pub state_operator_count: usize,
    pub content_filters: Vec<String>,
    pub fonts: Vec<FontReport>,
    pub images: Vec<ImageReport>,
}

#[derive(Debug, Default, Clone, Copy)]
struct OperatorStats {
    text: usize,
    graphics: usize,
    path: usize,
    paint: usize,
    xobject: usize,
    state: usize,
}

#[derive(Debug)]
pub struct FontReport {
    pub name: String,
    pub base_font: Option<String>,
    pub subtype: Option<String>,
    pub embedded: bool,
}

#[derive(Debug)]
pub struct ImageReport {
    pub name: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub color_space: Option<String>,
    pub bits_per_component: Option<i64>,
    pub filters: Vec<String>,
}

#[derive(Debug)]
pub struct EmbeddedFileReport {
    pub object_id: ObjectId,
    pub filename: Option<String>,
    pub subtype: Option<String>,
    pub filters: Vec<String>,
    pub declared_size: Option<i64>,
    pub raw_size: usize,
    pub decoded_size: Option<usize>,
}

#[derive(Debug)]
pub struct ObjectReport {
    pub object_id: ObjectId,
    pub object_kind: String,
    pub type_name: Option<String>,
    pub subtype_name: Option<String>,
    pub has_stream: bool,
    pub raw_stream_size: Option<usize>,
    pub filters: Vec<String>,
}

#[derive(Debug)]
pub struct StreamReport {
    pub object_id: ObjectId,
    pub role: String,
    pub type_name: Option<String>,
    pub subtype_name: Option<String>,
    pub filters: Vec<String>,
    pub raw_size: usize,
    pub decoded_size: Option<usize>,
}

pub fn analyze_document(document: &PdfDocument) -> DocumentReport {
    let mut warnings = document.warnings().to_vec();
    let page_ids = document.page_ids();
    let mut pages = Vec::new();
    let mut text_page_count = 0;
    let mut total_images = 0;
    let mut total_fonts = 0;
    let mut embedded_fonts = 0;

    for (index, page_id) in page_ids.iter().enumerate() {
        let Some(page) = document.get(*page_id) else {
            warnings.push(format!("page object {} {} not found", page_id.0, page_id.1));
            continue;
        };

        let resources = document.inherited_resources(*page_id);
        let media_box = document
            .inherited_value(*page_id, b"MediaBox")
            .and_then(format_media_box);

        let mut content_filters = BTreeSet::new();
        let mut combined_content = Vec::new();
        for stream in document.page_content_streams(page) {
            for filter in stream.filters() {
                content_filters.insert(filter);
            }

            match stream.decode() {
                Ok(decoded) => {
                    combined_content.extend_from_slice(&decoded);
                    combined_content.push(b'\n');
                }
                Err(error) => warnings.push(format!(
                    "page {} content stream {} {} could not be decoded: {}",
                    index + 1,
                    stream.id.0,
                    stream.id.1,
                    error
                )),
            }
        }

        let operator_stats = analyze_operators(&combined_content);
        let text_operator_count = operator_stats.text;
        let has_text = text_operator_count > 0;
        if has_text {
            text_page_count += 1;
        }

        let (fonts, page_embedded_fonts) = collect_fonts(document, resources);
        total_fonts += fonts.len();
        embedded_fonts += page_embedded_fonts;

        let images = collect_images(document, resources);
        total_images += images.len();

        pages.push(PageReport {
            number: index + 1,
            object_id: *page_id,
            media_box,
            has_text,
            text_operator_count,
            graphics_operator_count: operator_stats.graphics,
            path_operator_count: operator_stats.path,
            paint_operator_count: operator_stats.paint,
            xobject_operator_count: operator_stats.xobject,
            state_operator_count: operator_stats.state,
            content_filters: content_filters.into_iter().collect(),
            fonts,
            images,
        });
    }

    let embedded_files = collect_embedded_files(document, &mut warnings);
    let objects = collect_object_reports(document);
    let streams = collect_stream_reports(document, &mut warnings);

    DocumentReport {
        version: document.version().map(ToOwned::to_owned),
        object_count: document.object_count(),
        total_streams: streams.len(),
        pages,
        text_page_count,
        total_images,
        total_fonts,
        embedded_fonts,
        embedded_files,
        objects,
        streams,
        warnings,
    }
}

fn collect_fonts(
    document: &PdfDocument,
    resources: Option<&BTreeMap<Vec<u8>, PdfObject>>,
) -> (Vec<FontReport>, usize) {
    let Some(resources) = resources else {
        return (Vec::new(), 0);
    };
    let Some(fonts_obj) = resources.get(b"Font".as_slice()) else {
        return (Vec::new(), 0);
    };
    let Some(font_dict) = document.resolve(fonts_obj).as_dict() else {
        return (Vec::new(), 0);
    };

    let mut reports = Vec::new();
    let mut embedded = 0;
    for (name, value) in font_dict {
        let font = document.resolve(value);
        let Some(font_dict) = font.as_dict() else {
            continue;
        };
        let descriptor = font_dict
            .get(b"FontDescriptor".as_slice())
            .and_then(|value| document.resolve(value).as_dict());
        let is_embedded = descriptor
            .map(|dict| {
                dict.contains_key(b"FontFile".as_slice())
                    || dict.contains_key(b"FontFile2".as_slice())
                    || dict.contains_key(b"FontFile3".as_slice())
            })
            .unwrap_or(false);
        if is_embedded {
            embedded += 1;
        }
        reports.push(FontReport {
            name: pdf_name(name),
            base_font: font_dict
                .get(b"BaseFont".as_slice())
                .and_then(|value| document.resolve(value).as_name())
                .map(pdf_name),
            subtype: font_dict
                .get(b"Subtype".as_slice())
                .and_then(|value| document.resolve(value).as_name())
                .map(pdf_name),
            embedded: is_embedded,
        });
    }
    reports.sort_by(|left, right| left.name.cmp(&right.name));
    (reports, embedded)
}

fn collect_images(
    document: &PdfDocument,
    resources: Option<&BTreeMap<Vec<u8>, PdfObject>>,
) -> Vec<ImageReport> {
    let Some(resources) = resources else {
        return Vec::new();
    };
    let Some(xobjects_obj) = resources.get(b"XObject".as_slice()) else {
        return Vec::new();
    };
    let Some(xobject_dict) = document.resolve(xobjects_obj).as_dict() else {
        return Vec::new();
    };

    let mut reports = Vec::new();
    for (name, value) in xobject_dict {
        let object = document.resolve(value);
        let Some(stream) = object.as_stream() else {
            continue;
        };
        let subtype = stream
            .dict
            .get(b"Subtype".as_slice())
            .and_then(|value| document.resolve(value).as_name());
        if subtype != Some(b"Image".as_slice()) {
            continue;
        }

        reports.push(ImageReport {
            name: pdf_name(name),
            width: stream
                .dict
                .get(b"Width".as_slice())
                .and_then(|value| document.resolve(value).as_i64()),
            height: stream
                .dict
                .get(b"Height".as_slice())
                .and_then(|value| document.resolve(value).as_i64()),
            color_space: stream
                .dict
                .get(b"ColorSpace".as_slice())
                .map(|value| format_object_brief(document.resolve(value))),
            bits_per_component: stream
                .dict
                .get(b"BitsPerComponent".as_slice())
                .and_then(|value| document.resolve(value).as_i64()),
            filters: stream.filters(),
        });
    }
    reports.sort_by(|left, right| left.name.cmp(&right.name));
    reports
}

fn collect_embedded_files(
    document: &PdfDocument,
    warnings: &mut Vec<String>,
) -> Vec<EmbeddedFileReport> {
    let mut reports = Vec::new();
    let mut seen = BTreeSet::new();

    for (_, object) in document.objects() {
        let Some(dict) = object.as_dict() else {
            continue;
        };

        let Some(ef) = dict
            .get(b"EF".as_slice())
            .and_then(|value| document.resolve(value).as_dict())
        else {
            continue;
        };

        let stream_ref = ef
            .get(b"UF".as_slice())
            .or_else(|| ef.get(b"F".as_slice()))
            .or_else(|| ef.values().next());
        let Some(PdfObject::Reference(stream_id)) = stream_ref else {
            continue;
        };
        if !seen.insert(*stream_id) {
            continue;
        }

        let Some(stream) = document.get(*stream_id).and_then(PdfObject::as_stream) else {
            warnings.push(format!(
                "embedded file stream {} {} not found",
                stream_id.0, stream_id.1
            ));
            continue;
        };

        let decoded_size = match stream.decode() {
            Ok(bytes) => Some(bytes.len()),
            Err(error) => {
                warnings.push(format!(
                    "embedded file stream {} {} could not be decoded: {}",
                    stream_id.0, stream_id.1, error
                ));
                None
            }
        };

        let subtype = stream
            .dict
            .get(b"Subtype".as_slice())
            .map(|value| format_object_brief(document.resolve(value)));
        let declared_size = stream
            .dict
            .get(b"Params".as_slice())
            .and_then(|value| document.resolve(value).as_dict())
            .and_then(|params| params.get(b"Size".as_slice()))
            .and_then(|value| document.resolve(value).as_i64());

        reports.push(EmbeddedFileReport {
            object_id: *stream_id,
            filename: dict
                .get(b"UF".as_slice())
                .or_else(|| dict.get(b"F".as_slice()))
                .map(|value| format_object_brief(document.resolve(value))),
            subtype,
            filters: stream.filters(),
            declared_size,
            raw_size: stream.data.len(),
            decoded_size,
        });
    }

    reports.sort_by(|left, right| left.object_id.cmp(&right.object_id));
    reports
}

fn collect_object_reports(document: &PdfDocument) -> Vec<ObjectReport> {
    let mut reports = Vec::new();
    for (id, object) in document.objects() {
        let (object_kind, type_name, subtype_name, has_stream, raw_stream_size, filters) =
            match object {
                PdfObject::Stream(stream) => (
                    classify_object_kind(document, Some(&stream.dict), true),
                    dict_name(document, &stream.dict, b"Type"),
                    dict_name(document, &stream.dict, b"Subtype"),
                    true,
                    Some(stream.data.len()),
                    stream.filters(),
                ),
                PdfObject::Dictionary(dict) => (
                    classify_object_kind(document, Some(dict), false),
                    dict_name(document, dict, b"Type"),
                    dict_name(document, dict, b"Subtype"),
                    false,
                    None,
                    Vec::new(),
                ),
                PdfObject::Array(_) => ("array".to_string(), None, None, false, None, Vec::new()),
                PdfObject::Name(_) => ("name".to_string(), None, None, false, None, Vec::new()),
                PdfObject::String(_) => {
                    ("string".to_string(), None, None, false, None, Vec::new())
                }
                PdfObject::Integer(_) | PdfObject::Real(_) => {
                    ("number".to_string(), None, None, false, None, Vec::new())
                }
                PdfObject::Bool(_) => ("bool".to_string(), None, None, false, None, Vec::new()),
                PdfObject::Null => ("null".to_string(), None, None, false, None, Vec::new()),
                PdfObject::Reference(_) => {
                    ("reference".to_string(), None, None, false, None, Vec::new())
                }
            };

        reports.push(ObjectReport {
            object_id: *id,
            object_kind,
            type_name,
            subtype_name,
            has_stream,
            raw_stream_size,
            filters,
        });
    }
    reports
}

fn collect_stream_reports(
    document: &PdfDocument,
    warnings: &mut Vec<String>,
) -> Vec<StreamReport> {
    let mut reports = Vec::new();
    for (id, object) in document.objects() {
        let Some(stream) = object.as_stream() else {
            continue;
        };
        let decoded_size = match stream.decode() {
            Ok(bytes) => Some(bytes.len()),
            Err(error) => {
                warnings.push(format!(
                    "stream {} {} could not be decoded: {}",
                    id.0, id.1, error
                ));
                None
            }
        };

        reports.push(StreamReport {
            object_id: *id,
            role: infer_stream_role(document, stream),
            type_name: dict_name(document, &stream.dict, b"Type"),
            subtype_name: dict_name(document, &stream.dict, b"Subtype"),
            filters: stream.filters(),
            raw_size: stream.data.len(),
            decoded_size,
        });
    }
    reports
}

fn infer_stream_role(document: &PdfDocument, stream: &PdfStream) -> String {
    let subtype = dict_name(document, &stream.dict, b"Subtype");
    let type_name = dict_name(document, &stream.dict, b"Type");

    if subtype.as_deref() == Some("Image") {
        return "image".to_string();
    }
    if type_name.as_deref() == Some("EmbeddedFile") {
        return "embedded-file".to_string();
    }
    if subtype
        .as_deref()
        .is_some_and(|name| matches!(name, "Type1C" | "CIDFontType0C" | "OpenType"))
    {
        return "font-program".to_string();
    }
    if stream.dict.contains_key(b"Length".as_slice()) {
        return "stream".to_string();
    }
    "unknown".to_string()
}

fn classify_object_kind(document: &PdfDocument, dict: Option<&Dict>, has_stream: bool) -> String {
    let Some(dict) = dict else {
        return if has_stream {
            "stream".to_string()
        } else {
            "object".to_string()
        };
    };

    if has_stream {
        return infer_stream_role(
            document,
            &PdfStream {
                id: ObjectId(0, 0),
                dict: dict.clone(),
                data: Vec::new(),
            },
        );
    }

    dict_name(document, dict, b"Type").unwrap_or_else(|| "dictionary".to_string())
}

fn dict_name(document: &PdfDocument, dict: &Dict, key: &[u8]) -> Option<String> {
    dict.get(key)
        .and_then(|value| document.resolve(value).as_name())
        .map(pdf_name)
}

fn analyze_operators(bytes: &[u8]) -> OperatorStats {
    let mut stats = OperatorStats::default();
    for token in content_tokens(bytes) {
        if is_text_operator(token) {
            stats.text += 1;
        }
        if is_graphics_operator(token) {
            stats.graphics += 1;
        }
        if is_path_operator(token) {
            stats.path += 1;
        }
        if is_paint_operator(token) {
            stats.paint += 1;
        }
        if token == b"Do" {
            stats.xobject += 1;
        }
        if is_state_operator(token) {
            stats.state += 1;
        }
    }
    stats
}

fn content_tokens(bytes: &[u8]) -> Vec<&[u8]> {
    let mut tokens = Vec::new();
    let mut in_literal = false;
    let mut escape = false;
    let mut start = None;
    let mut i = 0;

    while i < bytes.len() {
        let byte = bytes[i];
        if in_literal {
            if escape {
                escape = false;
                i += 1;
                continue;
            }
            match byte {
                b'\\' => escape = true,
                b')' => in_literal = false,
                _ => {}
            }
            i += 1;
            continue;
        }

        if byte == b'%' {
            if let Some(token_start) = start.take() {
                tokens.push(&bytes[token_start..i]);
            }
            while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
                i += 1;
            }
            continue;
        }

        if byte == b'(' {
            if let Some(token_start) = start.take() {
                tokens.push(&bytes[token_start..i]);
            }
            in_literal = true;
            i += 1;
            continue;
        }

        if byte.is_ascii_whitespace() || b"[]<>/{}/".contains(&byte) {
            if let Some(token_start) = start.take() {
                tokens.push(&bytes[token_start..i]);
            }
            i += 1;
            continue;
        }

        if start.is_none() {
            start = Some(i);
        }
        i += 1;
    }

    if let Some(token_start) = start {
        tokens.push(&bytes[token_start..]);
    }
    tokens
}

fn is_text_operator(token: &[u8]) -> bool {
    matches!(token, b"Tj" | b"TJ" | b"'" | b"\"" | b"Tf" | b"BT" | b"ET")
}

fn is_graphics_operator(token: &[u8]) -> bool {
    matches!(
        token,
        b"w" | b"J" | b"j" | b"M" | b"d" | b"ri" | b"i" | b"gs" | b"q" | b"Q" | b"cm"
    )
}

fn is_path_operator(token: &[u8]) -> bool {
    matches!(token, b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re")
}

fn is_paint_operator(token: &[u8]) -> bool {
    matches!(
        token,
        b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n"
    )
}

fn is_state_operator(token: &[u8]) -> bool {
    matches!(token, b"CS" | b"cs" | b"SC" | b"SCN" | b"sc" | b"scn" | b"G" | b"g" | b"RG" | b"rg" | b"K" | b"k")
}

fn format_media_box(object: &PdfObject) -> Option<String> {
    let values = object.as_array()?;
    let numbers: Vec<String> = values
        .iter()
        .filter_map(PdfObject::as_f64)
        .map(|value| {
            let integer = value as i64;
            if integer as f64 == value {
                integer.to_string()
            } else {
                format!("{value}")
            }
        })
        .collect();
    if numbers.is_empty() {
        None
    } else {
        Some(numbers.join(" "))
    }
}

fn format_object_brief(object: &PdfObject) -> String {
    match object {
        PdfObject::Name(name) => pdf_name(name),
        PdfObject::Array(values) => {
            let parts: Vec<String> = values.iter().map(format_object_brief).collect();
            format!("[{}]", parts.join(", "))
        }
        PdfObject::Reference(id) => format!("{} {} R", id.0, id.1),
        PdfObject::Integer(value) => value.to_string(),
        PdfObject::Real(value) => value.to_string(),
        PdfObject::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => "complex".to_string(),
    }
}

fn pdf_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}
