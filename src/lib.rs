#![no_std]

extern crate alloc;

pub mod analysis;
pub mod pdf;

pub use analysis::{
    DocumentReport, FontReport, ImageReport, PageReport, analyze_document,
};
pub use pdf::{ObjectId, PdfDocument, PdfObject, PdfStream};

#[cfg(test)]
extern crate std;
