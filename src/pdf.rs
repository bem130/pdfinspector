use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::str;
use miniz_oxide::inflate::decompress_to_vec_zlib;

pub type Dict = BTreeMap<Vec<u8>, PdfObject>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObjectId(pub u32, pub u16);

#[derive(Clone, Debug)]
pub struct PdfStream {
    pub id: ObjectId,
    pub dict: Dict,
    pub data: Vec<u8>,
}

impl PdfStream {
    pub fn filters(&self) -> Vec<String> {
        self.dict
            .get(b"Filter".as_slice())
            .map(filters_from_object)
            .unwrap_or_default()
    }

    pub fn decode(&self) -> Result<Vec<u8>, String> {
        let filters = self.filters();
        if filters.is_empty() {
            return Ok(self.data.clone());
        }

        let mut current = self.data.clone();
        for filter in filters {
            current = match filter.as_str() {
                "FlateDecode" | "Fl" => decompress_to_vec_zlib(&current)
                    .map_err(|error| format!("flate decode failed: {:?}", error))?,
                other => return Err(format!("unsupported filter {other}")),
            };
        }
        Ok(current)
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum PdfObject {
    Null,
    Bool(bool),
    Integer(i64),
    Real(f64),
    Name(Vec<u8>),
    String(Vec<u8>),
    Array(Vec<PdfObject>),
    Dictionary(Dict),
    Stream(PdfStream),
    Reference(ObjectId),
}

impl PdfObject {
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            PdfObject::Dictionary(dict) => Some(dict),
            PdfObject::Stream(stream) => Some(&stream.dict),
            _ => None,
        }
    }

    pub fn as_stream(&self) -> Option<&PdfStream> {
        match self {
            PdfObject::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[PdfObject]> {
        match self {
            PdfObject::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            PdfObject::Name(name) => Some(name),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PdfObject::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PdfObject::Integer(value) => Some(*value as f64),
            PdfObject::Real(value) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PdfDocument {
    version: Option<String>,
    objects: BTreeMap<ObjectId, PdfObject>,
    warnings: Vec<String>,
}

impl PdfDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let version = detect_version(bytes);
        let mut parser = ObjectParser::new(bytes);
        let (objects, warnings) = parser.parse_objects();
        if objects.is_empty() {
            return Err("no PDF indirect objects found".to_string());
        }
        Ok(Self {
            version,
            objects,
            warnings,
        })
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn get(&self, id: ObjectId) -> Option<&PdfObject> {
        self.objects.get(&id)
    }

    pub fn objects(&self) -> impl Iterator<Item = (&ObjectId, &PdfObject)> {
        self.objects.iter()
    }

    pub fn resolve<'a>(&'a self, object: &'a PdfObject) -> &'a PdfObject {
        let mut current = object;
        let mut visited = BTreeSet::new();
        while let PdfObject::Reference(id) = current {
            if !visited.insert(*id) {
                break;
            }
            let Some(next) = self.objects.get(id) else {
                break;
            };
            current = next;
        }
        current
    }

    pub fn page_ids(&self) -> Vec<ObjectId> {
        let mut pages = Vec::new();
        for (id, object) in &self.objects {
            let Some(dict) = object.as_dict() else {
                continue;
            };
            let page_type = dict
                .get(b"Type".as_slice())
                .and_then(|value| self.resolve(value).as_name());
            if page_type == Some(b"Page".as_slice()) {
                pages.push(*id);
            }
        }
        pages
    }

    pub fn inherited_value(&self, page_id: ObjectId, key: &[u8]) -> Option<&PdfObject> {
        let mut current = self.objects.get(&page_id)?;
        let mut visited = BTreeSet::new();
        loop {
            let dict = current.as_dict()?;
            if let Some(value) = dict.get(key) {
                return Some(self.resolve(value));
            }
            let parent_ref = dict.get(b"Parent".as_slice())?;
            let PdfObject::Reference(parent_id) = parent_ref else {
                return None;
            };
            if !visited.insert(*parent_id) {
                return None;
            }
            current = self.objects.get(parent_id)?;
        }
    }

    pub fn inherited_resources(&self, page_id: ObjectId) -> Option<&Dict> {
        self.inherited_value(page_id, b"Resources")?.as_dict()
    }

    pub fn page_content_streams<'a>(&'a self, page: &'a PdfObject) -> Vec<&'a PdfStream> {
        let mut streams = Vec::new();
        let Some(dict) = page.as_dict() else {
            return streams;
        };
        let Some(contents) = dict.get(b"Contents".as_slice()) else {
            return streams;
        };

        match self.resolve(contents) {
            PdfObject::Stream(stream) => streams.push(stream),
            PdfObject::Array(values) => {
                for value in values {
                    if let Some(stream) = self.resolve(value).as_stream() {
                        streams.push(stream);
                    }
                }
            }
            _ => {}
        }
        streams
    }
}

fn detect_version(bytes: &[u8]) -> Option<String> {
    let end = bytes.len().min(1024);
    let scan = &bytes[..end];
    let start = scan.windows(5).position(|window| window == b"%PDF-")?;
    let prefix = scan.get(start..(start + 16).min(scan.len()))?;
    let text = str::from_utf8(prefix).ok()?;
    let version = text.strip_prefix("%PDF-")?;
    let end = version
        .find(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or(version.len());
    Some(version[..end].to_string())
}

fn filters_from_object(object: &PdfObject) -> Vec<String> {
    match object {
        PdfObject::Name(name) => vec![String::from_utf8_lossy(name).into_owned()],
        PdfObject::Array(values) => values
            .iter()
            .filter_map(|value| match value {
                PdfObject::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

struct ObjectParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
    warnings: Vec<String>,
}

impl<'a> ObjectParser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            warnings: Vec::new(),
        }
    }

    fn parse_objects(&mut self) -> (BTreeMap<ObjectId, PdfObject>, Vec<String>) {
        let mut objects = BTreeMap::new();
        while let Some((id, object, next)) = self.next_object() {
            objects.insert(id, object);
            self.cursor = next;
        }
        (objects, core::mem::take(&mut self.warnings))
    }

    fn next_object(&self) -> Option<(ObjectId, PdfObject, usize)> {
        let mut pos = self.cursor;
        while pos < self.bytes.len() {
            pos = find_token(self.bytes, pos, b" obj")?;
            let line_start = reverse_to_line_start(self.bytes, pos);
            let header = &self.bytes[line_start..pos];
            let mut parts = header.split(|byte| byte.is_ascii_whitespace()).filter(|part| !part.is_empty());
            let object_number = str::from_utf8(parts.next()?).ok()?.parse::<u32>().ok()?;
            let generation = str::from_utf8(parts.next()?).ok()?.parse::<u16>().ok()?;
            let obj_start = pos + 4;
            let end = find_token(self.bytes, obj_start, b"endobj")?;
            let raw = &self.bytes[obj_start..end];
            match parse_indirect_object(self.bytes, ObjectId(object_number, generation), raw, obj_start) {
                Ok(object) => return Some((ObjectId(object_number, generation), object, end + 6)),
                Err(_) => {
                    pos = end + 6;
                }
            }
        }
        None
    }
}

fn parse_indirect_object(
    source: &[u8],
    id: ObjectId,
    raw: &[u8],
    start_offset: usize,
) -> Result<PdfObject, String> {
    let mut parser = ValueParser::new(raw, source, start_offset);
    let value = parser.parse_value()?;
    parser.skip_ws();

    if let PdfObject::Dictionary(dict) = value {
        if raw.get(parser.pos..).is_some_and(|tail| tail.starts_with(b"stream")) {
            parser.pos += "stream".len();
            if raw.get(parser.pos) == Some(&b'\r') {
                parser.pos += 1;
            }
            if raw.get(parser.pos) == Some(&b'\n') {
                parser.pos += 1;
            }

            let length = dict
                .get(b"Length".as_slice())
                .and_then(|value| match value {
                    PdfObject::Integer(length) => usize::try_from(*length).ok(),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    raw[parser.pos..]
                        .windows(9)
                        .position(|window| window == b"endstream")
                        .unwrap_or(0)
                });

            let end = parser.pos.saturating_add(length).min(raw.len());
            let data = raw[parser.pos..end].to_vec();
            return Ok(PdfObject::Stream(PdfStream { id, dict, data }));
        }
        return Ok(PdfObject::Dictionary(dict));
    }

    Ok(value)
}

struct ValueParser<'a> {
    bytes: &'a [u8],
    full_source: &'a [u8],
    base_offset: usize,
    pos: usize,
}

impl<'a> ValueParser<'a> {
    fn new(bytes: &'a [u8], full_source: &'a [u8], base_offset: usize) -> Self {
        Self {
            bytes,
            full_source,
            base_offset,
            pos: 0,
        }
    }

    fn parse_value(&mut self) -> Result<PdfObject, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'<') if self.peek_next() == Some(b'<') => self.parse_dictionary(),
            Some(b'[') => self.parse_array(),
            Some(b'/') => self.parse_name().map(PdfObject::Name),
            Some(b'(') => self.parse_literal_string().map(PdfObject::String),
            Some(b't') => self.parse_keyword(b"true", PdfObject::Bool(true)),
            Some(b'f') => self.parse_keyword(b"false", PdfObject::Bool(false)),
            Some(b'n') => self.parse_keyword(b"null", PdfObject::Null),
            Some(_) => self.parse_number_or_reference(),
            None => Err("unexpected end of object".to_string()),
        }
    }

    fn parse_dictionary(&mut self) -> Result<PdfObject, String> {
        self.expect_bytes(b"<<")?;
        let mut dict = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.starts_with(b">>") {
                self.pos += 2;
                break;
            }
            let key = self.parse_name()?;
            let value = self.parse_value()?;
            dict.insert(key, value);
        }
        Ok(PdfObject::Dictionary(dict))
    }

    fn parse_array(&mut self) -> Result<PdfObject, String> {
        self.expect_byte(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b']') {
                self.pos += 1;
                break;
            }
            values.push(self.parse_value()?);
        }
        Ok(PdfObject::Array(values))
    }

    fn parse_name(&mut self) -> Result<Vec<u8>, String> {
        self.expect_byte(b'/')?;
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() || b"[]<>{}()/".contains(&byte) {
                break;
            }
            self.pos += 1;
        }
        Ok(self.bytes[start..self.pos].to_vec())
    }

    fn parse_literal_string(&mut self) -> Result<Vec<u8>, String> {
        self.expect_byte(b'(')?;
        let mut depth = 1;
        let mut out = Vec::new();
        let mut escape = false;
        while let Some(byte) = self.next() {
            if escape {
                out.push(byte);
                escape = false;
                continue;
            }
            match byte {
                b'\\' => escape = true,
                b'(' => {
                    depth += 1;
                    out.push(byte);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(byte);
                }
                _ => out.push(byte),
            }
        }
        Err("unterminated literal string".to_string())
    }

    fn parse_keyword(&mut self, keyword: &[u8], value: PdfObject) -> Result<PdfObject, String> {
        self.expect_bytes(keyword)?;
        Ok(value)
    }

    fn parse_number_or_reference(&mut self) -> Result<PdfObject, String> {
        let first = self.parse_number_token()?;
        let checkpoint = self.pos;
        self.skip_ws();
        if let Ok(second) = self.parse_number_token() {
            self.skip_ws();
            if self.peek() == Some(b'R') {
                self.pos += 1;
                let object = first.parse::<u32>().map_err(|_| "invalid ref".to_string())?;
                let generation = second.parse::<u16>().map_err(|_| "invalid ref".to_string())?;
                return Ok(PdfObject::Reference(ObjectId(object, generation)));
            }
        }
        self.pos = checkpoint;

        if first.contains('.') || first.contains('-') && first.len() > 1 {
            first
                .parse::<f64>()
                .map(PdfObject::Real)
                .map_err(|_| "invalid real".to_string())
        } else {
            first
                .parse::<i64>()
                .map(PdfObject::Integer)
                .map_err(|_| "invalid integer".to_string())
        }
    }

    fn parse_number_token(&mut self) -> Result<String, String> {
        self.skip_ws();
        let start = self.pos;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.pos += 1;
        }
        while let Some(byte) = self.peek() {
            if !(byte.is_ascii_digit() || byte == b'.') {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err("expected number".to_string());
        }
        let source = &self.full_source[self.base_offset + start..self.base_offset + self.pos];
        Ok(String::from_utf8_lossy(source).into_owned())
    }

    fn skip_ws(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if self.peek() == Some(b'%') {
                while self.peek().is_some() && self.peek() != Some(b'\n') && self.peek() != Some(b'\r') {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn starts_with(&self, bytes: &[u8]) -> bool {
        self.bytes.get(self.pos..).is_some_and(|tail| tail.starts_with(bytes))
    }

    fn expect_byte(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected byte {}", byte))
        }
    }

    fn expect_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.starts_with(bytes) {
            self.pos += bytes.len();
            Ok(())
        } else {
            Err("unexpected token".to_string())
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }
}

fn find_token(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|pos| pos + start)
}

fn reverse_to_line_start(bytes: &[u8], mut pos: usize) -> usize {
    while pos > 0 {
        if matches!(bytes[pos - 1], b'\n' | b'\r') {
            break;
        }
        pos -= 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{string::String, vec};

    const SIMPLE_PDF: &[u8] = br#"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Resources << /Font << /F1 4 0 R >> /XObject << /Im1 6 0 R >> >> /Contents 5 0 R >>
endobj
4 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
5 0 obj
<< /Length 39 >>
stream
BT /F1 12 Tf (Hello PDF) Tj ET
endstream
endobj
6 0 obj
<< /Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 3 >>
stream
abc
endstream
endobj
trailer
<< /Root 1 0 R >>
%%EOF
"#;

    #[test]
    fn parse_document_objects() {
        let document = PdfDocument::parse(SIMPLE_PDF).expect("parse");
        assert_eq!(document.version(), Some("1.4"));
        assert_eq!(document.object_count(), 6);
        assert_eq!(document.page_ids(), vec![ObjectId(3, 0)]);
    }

    #[test]
    fn decode_unfiltered_stream() {
        let document = PdfDocument::parse(SIMPLE_PDF).expect("parse");
        let page = document.get(ObjectId(3, 0)).expect("page");
        let streams = document.page_content_streams(page);
        assert_eq!(streams.len(), 1);
        let decoded = streams[0].decode().expect("decode");
        assert!(String::from_utf8_lossy(&decoded).contains("Hello PDF"));
    }
}
