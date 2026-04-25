# pdfinspector

PDF file structure inspector written in Rust.

The project is split into:

- `pdfinspector` library core in `src/lib.rs`, built as `no_std + alloc`
- CLI frontend in `src/main.rs`, built with `std`

This CLI statically inspects a PDF and prints a page-oriented summary of:

- page count and indirect object count
- total stream count
- whether a page appears to contain text drawing operators
- image XObject metadata such as size, color space, bits per component, and filters
- fonts referenced by each page and whether they appear embedded via `FontDescriptor`
- embedded file streams when present
- object and stream inventories with `--objects`, `--streams`, or `--all`
- content stream filter names and decoded sizes when supported

## Build

```powershell
cargo build
```

## Usage

```powershell
cargo run -- path\to\file.pdf
```

Or run the built binary directly:

```powershell
target\debug\pdfinspector.exe path\to\file.pdf
```

Detailed inventories:

```powershell
target\debug\pdfinspector.exe --all path\to\file.pdf
```

## Output example

```text
File: sample.pdf
PDF Version: 1.4
Objects: 14
Pages: 2
Text pages: 2
Image XObjects: 1
Fonts: 2
Embedded fonts: 1

Page 1:
  Object: 3 0
  MediaBox: 0 0 595 842
  Text operators: 8
  Text present: yes
  Content filters: none
  Fonts:
    - F1: base=Helvetica, subtype=Type1, not embedded
  Images:
    - Im1: 640x480, color=DeviceRGB, bpc=8, filters=DCTDecode
```

## Scope and limitations

- The tool parses indirect objects directly from the file body and does not rely on the xref table.
- It supports dictionaries, arrays, names, strings, numbers, references, and stream objects.
- `FlateDecode` streams are decoded and inspected. Other filters such as `DCTDecode` are still reported, but not decompressed.
- The parser is designed for inspection and triage, not full PDF conformance.
- `cargo check` and `cargo test --no-run` work in this environment. Full `cargo test` execution may still be blocked by local application control policy on generated test executables.
