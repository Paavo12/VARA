//! Upload sanitising (TRD §7.2): the bytes must really be the declared type, images are
//! decoded and re-encoded (which drops EXIF/GPS metadata and anything appended to the file),
//! and PDFs with active content are refused.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, codecs::jpeg::JpegEncoder, imageops::FilterType};

/// Longest edge kept after re-encoding: plenty for reading an ID card, small enough for review.
pub const MAX_EDGE_PX: u32 = 3000;
const MAX_DECODE_PX: u32 = 12_000;
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;
const JPEG_QUALITY: u8 = 88;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Pdf,
}

impl Kind {
    pub fn from_mime(mime: &str) -> Option<Kind> {
        match mime {
            "image/jpeg" => Some(Kind::Jpeg),
            "image/png" => Some(Kind::Png),
            "application/pdf" => Some(Kind::Pdf),
            _ => None,
        }
    }
}

/// Identifies a file by its first bytes, never by its name or declared type.
pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if bytes.starts_with(b"%PDF-") {
        Some(Kind::Pdf)
    } else {
        None
    }
}

#[derive(Debug)]
pub struct Sanitized {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub extension: &'static str,
}

/// Why a file was refused. Messages are shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Rejected(pub &'static str);

pub fn sanitize(bytes: &[u8], declared_mime: &str) -> Result<Sanitized, Rejected> {
    let declared = Kind::from_mime(declared_mime).ok_or(Rejected("This file type is not accepted."))?;
    let actual = sniff(bytes).ok_or(Rejected("This file is not a JPEG, PNG or PDF. Please take a new photo."))?;
    if actual != declared {
        return Err(Rejected("The file's contents don't match its type. Please choose the file again."));
    }
    match actual {
        Kind::Jpeg => reencode(bytes, ImageFormat::Jpeg),
        Kind::Png => reencode(bytes, ImageFormat::Png),
        Kind::Pdf => check_pdf(bytes).map(|()| Sanitized { bytes: bytes.to_vec(), mime: "application/pdf", extension: "pdf" }),
    }
}

fn reencode(bytes: &[u8], format: ImageFormat) -> Result<Sanitized, Rejected> {
    let unreadable = || Rejected("This photo couldn't be read. Please take a new photo.");
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_PX);
    limits.max_image_height = Some(MAX_DECODE_PX);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| unreadable())?;
    // Phones store "rotate 90°" in EXIF; apply it before EXIF is dropped.
    let orientation = decoder.orientation().map_err(|_| unreadable())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| unreadable())?;
    img.apply_orientation(orientation);
    if img.width() < 200 || img.height() < 200 {
        return Err(Rejected("This photo is too small to read. Please take a new photo."));
    }
    if img.width().max(img.height()) > MAX_EDGE_PX {
        img = img.resize(MAX_EDGE_PX, MAX_EDGE_PX, FilterType::Triangle);
    }
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    let mut out = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)).map_err(|_| unreadable())?;
    Ok(Sanitized { bytes: out, mime: "image/jpeg", extension: "jpg" })
}

/// Names that let a PDF run code, open other files or carry attachments.
const PDF_ACTIVE_CONTENT: [&[u8]; 8] =
    [b"/JavaScript", b"/JS", b"/Launch", b"/EmbeddedFile", b"/RichMedia", b"/XFA", b"/SubmitForm", b"/ImportData"];

/// PDFs cannot be re-rendered without a PDF engine, so they are checked instead: a real PDF
/// header and trailer, and none of the active-content names in its uncompressed text. Names
/// hidden inside compressed object streams are left to the virus scanner; reviewers open PDFs
/// in the ops console's sandboxed viewer (Phase 5). Rendering PDFs to images server-side is the
/// planned hardening step (Implementation Plan Phase 6).
fn check_pdf(bytes: &[u8]) -> Result<(), Rejected> {
    if !(bytes.starts_with(b"%PDF-1.") || bytes.starts_with(b"%PDF-2.")) {
        return Err(Rejected("This PDF couldn't be read. Please upload a photo instead."));
    }
    let tail = &bytes[bytes.len().saturating_sub(1024)..];
    if !tail.windows(5).any(|w| w == b"%%EOF") {
        return Err(Rejected("This PDF is incomplete. Please upload it again."));
    }
    if PDF_ACTIVE_CONTENT.iter().any(|name| contains_name(bytes, name)) {
        return Err(Rejected("PDFs with scripts, forms or attachments aren't accepted. Please upload a photo instead."));
    }
    Ok(())
}

/// `name` followed by a PDF delimiter, so `/JS` matches but `/JSFoo` does not.
fn contains_name(haystack: &[u8], name: &[u8]) -> bool {
    haystack
        .windows(name.len() + 1)
        .any(|w| &w[..name.len()] == name && matches!(w[name.len()], b' ' | b'\n' | b'\r' | b'\t' | b'/' | b'<' | b'(' | b'[' | b'>' | 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| Rgb([(x % 255) as u8, (y % 255) as u8, 90]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn jpeg_with_exif(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |_, _| Rgb([200, 30, 90])));
        let mut plain = Vec::new();
        img.write_with_encoder(JpegEncoder::new_with_quality(&mut plain, 90)).unwrap();
        // Splice an APP1 "Exif" segment carrying a fake GPS marker in after SOI.
        let payload = b"Exif\0\0GPS-LATITUDE-22.5667";
        let len = (payload.len() + 2) as u16;
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&plain[2..]);
        out
    }

    #[test]
    fn sniffs_by_content() {
        assert_eq!(sniff(&png(1, 1)), Some(Kind::Png));
        assert_eq!(sniff(b"%PDF-1.7"), Some(Kind::Pdf));
        assert_eq!(sniff(b"<html><script>"), None);
        assert_eq!(sniff(b"GIF89a"), None);
    }

    #[test]
    fn jpeg_is_reencoded_without_metadata() {
        let input = jpeg_with_exif(400, 300);
        assert!(input.windows(12).any(|w| w == b"GPS-LATITUDE"));
        let out = sanitize(&input, "image/jpeg").unwrap();
        assert_eq!(out.mime, "image/jpeg");
        assert!(!out.bytes.windows(12).any(|w| w == b"GPS-LATITUDE"), "EXIF survived");
        assert!(!out.bytes.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn png_becomes_jpeg_and_large_images_shrink() {
        let out = sanitize(&png(4000, 500), "image/png").unwrap();
        assert_eq!(out.mime, "image/jpeg");
        let img = image::load_from_memory(&out.bytes).unwrap();
        assert_eq!(img.width(), MAX_EDGE_PX);
    }

    #[test]
    fn type_mismatch_and_garbage_rejected() {
        assert!(sanitize(&png(300, 300), "image/jpeg").is_err());
        assert!(sanitize(b"<svg onload=alert(1)>", "image/png").is_err());
        assert!(sanitize(&png(300, 300), "image/gif").is_err());
        // A truncated image is refused rather than half-decoded.
        let p = png(300, 300);
        assert!(sanitize(&p[..p.len() / 2], "image/png").is_err());
        assert!(sanitize(&png(50, 50), "image/png").is_err(), "too small to read");
    }

    #[test]
    fn appended_payload_is_dropped() {
        let mut p = png(300, 300);
        p.extend_from_slice(b"<?php system($_GET['c']); ?>");
        let out = sanitize(&p, "image/png").unwrap();
        assert!(!out.bytes.windows(5).any(|w| w == b"<?php"));
    }

    #[test]
    fn pdfs_with_active_content_rejected() {
        let ok = b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF\n";
        assert!(sanitize(ok, "application/pdf").is_ok());
        let js = b"%PDF-1.7\n1 0 obj << /OpenAction << /S /JavaScript /JS (app.alert(1)) >> >> endobj\n%%EOF";
        assert!(sanitize(js, "application/pdf").is_err());
        assert!(sanitize(b"%PDF-1.7\n no trailer", "application/pdf").is_err());
        assert!(!contains_name(b"/JSFoo ", b"/JS"));
    }
}
