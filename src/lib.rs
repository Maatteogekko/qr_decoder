use chrono::{DateTime, NaiveDate, Utc};
use image::{DynamicImage, EncodableLayout, ImageFormat};
use pdfium_render::prelude::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use regex::Regex;
use rxing::{BarcodeFormat, DecodeHintType, DecodeHintValue, DecodingHintDictionary};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
};

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub barcodes: Vec<BarcodeData>,
}

#[derive(Debug, Serialize)]
pub struct BarcodeData {
    r#type: String,
    data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
}

/// Creates barcode detection hints from the given formats.
pub fn create_hints(
    formats: Option<Vec<BarcodeFormat>>,
) -> HashMap<DecodeHintType, DecodeHintValue> {
    let mut hints = HashMap::from([(DecodeHintType::TRY_HARDER, DecodeHintValue::TryHarder(true))]);

    if let Some(formats) = formats {
        hints.insert(
            DecodeHintType::POSSIBLE_FORMATS,
            DecodeHintValue::PossibleFormats(HashSet::from_iter(formats)),
        );
    }

    hints
}

/// Scan the file for barcodes and pagoPA payment dates.
pub fn process_file(
    path: &Path,
    hints: Option<DecodingHintDictionary>,
) -> Result<ScanResult, String> {
    let mut barcodes = scan_barcodes(path, hints)?;

    let mime = infer::get_from_path(path)
        .ok()
        .flatten()
        .map(|k| k.mime_type().to_string())
        .unwrap_or_default();

    let dates_and_codes = if mime == "application/pdf" {
        match run_mutool_to_html(path) {
            Ok(html) => extract_dates_and_codes_from_html(&html),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    enrich_barcodes_with_dates(&mut barcodes, &dates_and_codes);

    Ok(ScanResult { barcodes })
}

/// Process the file and extract barcodes.
pub fn scan_barcodes(
    path: &Path,
    hints: Option<DecodingHintDictionary>,
) -> Result<Vec<BarcodeData>, String> {
    let images = get_images(&path).map_err(|e| e.to_string())?;
    let barcode_list = Arc::new(Mutex::new(Vec::new()));

    images.par_iter().for_each(|image| {
        let width = image.width();
        let height = image.height();
        let luma_image: Vec<u8> = image.clone().into_luma8().as_bytes().into();

        let results = match &mut hints.clone() {
            Some(hints) => {
                rxing::helpers::detect_multiple_in_luma_with_hints(luma_image, width, height, hints)
            }
            None => rxing::helpers::detect_multiple_in_luma(luma_image, width, height),
        };

        if let Ok(results) = results {
            for result in results {
                let mut list = barcode_list.lock().expect("acquired Mutex");
                list.push(BarcodeData {
                    r#type: result.getBarcodeFormat().to_string(),
                    data: result.getText().to_string(),
                    date: None,
                });
            }
        }
    });

    Ok(Arc::into_inner(barcode_list)
        .expect("valid Arc")
        .into_inner()
        .expect("valid Mutex"))
}

/// Gets images from the provided file path, handling different formats.
fn get_images(path: &impl AsRef<Path>) -> Result<Vec<DynamicImage>, String> {
    let kind = infer::get_from_path(path)
        .map_err(|_| "Failed to read file".to_string())?
        .ok_or_else(|| "Unknown file type".to_string())?;

    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    match kind.mime_type() {
        "application/pdf" => {
            extract_images(path).map_err(|e| format!("Failed to extract images from PDF: {:?}", e))
        }
        mime_type @ ("image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/tiff"
        | "image/bmp") => {
            let format = ImageFormat::from_mime_type(mime_type).expect("found mime_type");
            image::load_from_memory_with_format(&buffer, format)
                .map(|img| vec![img])
                .map_err(|e| format!("Failed to read image: {}", e))
        }
        filetype => Err(format!("Unexpected file type: {filetype}")),
    }
}

/// Extracts rasterized page images from a PDF file using pdfium.
fn extract_images(path: &impl AsRef<Path>) -> Result<Vec<DynamicImage>, PdfiumError> {
    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file(path, None)?;

    let dpi: f32 = 144.0;
    let mut images = Vec::new();
    for page in document.pages().iter() {
        let w_px = ((page.width() / 72.0) * dpi).value.ceil() as i32;
        let h_px = ((page.height() / 72.0) * dpi).value.ceil() as i32;

        let render_config = PdfRenderConfig::new()
            .set_target_width(w_px)
            .set_target_height(h_px)
            .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);

        images.push(page.render_with_config(&render_config)?.as_image());
    }

    Ok(images)
}

#[derive(Debug, Serialize, Clone)]
struct DateCodePair {
    date: String,
    code: String,
}

#[derive(Debug, Clone)]
struct Item {
    val: String,
    score: f64,
}

#[derive(Copy, Clone)]
enum Kind {
    Date,
    PagoPa,
}

fn pagopa_qr_re() -> &'static Regex {
    static PAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PAT.get_or_init(|| {
        Regex::new(concat!(
            "(",
              r"^PAGOPA\|002\|(?P<code1>[0-9]{18})\|[0-9]{11}\|[0-9]{1,}",
              "|",
              r"^codfase=NBPA;18(?P<code2>[0-9]{18})12[0-9]{12}10[0-9]{10}38961P1[0-9]{11}[A-Z0-9 ]{16}.{162}A$",
            ")"
        ))
        .expect("valid combined PAGOPA QR regex")
    })
}
fn pagopa_text_re() -> &'static Regex {
    // Starts with 30 or 1x. Optional single space at 4/8/12/16 boundaries.
    static PAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PAT.get_or_init(|| Regex::new(r"^(?:30|1\d)\d{2}(?:\s?\d{4}){3}\s?\d{2}$").unwrap())
}
fn date_text_re() -> &'static Regex {
    static PAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PAT.get_or_init(|| Regex::new(r"^\b\d{2}/\d{2}/\d{4}\b$").unwrap())
}
fn page_id_re() -> &'static Regex {
    static PAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PAT.get_or_init(|| Regex::new(r"^page(\d+)$").unwrap())
}
fn css_float_re() -> &'static Regex {
    static PAT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PAT.get_or_init(|| Regex::new(r"(-?\d+(?:\.\d+)?)").unwrap())
}

fn clean_text(s: &str) -> String {
    static WS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = WS.get_or_init(|| Regex::new(r"\s+").unwrap());
    re.replace_all(s, "").trim().to_string()
}

fn parse_style(style: Option<&str>) -> (Option<f64>, Option<f64>) {
    let mut top: Option<f64> = None;
    let mut left: Option<f64> = None;

    match style {
        None => (None, None),
        Some(style_str) => {
            for part in style_str
                .split(';')
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                let (k, v) = match part.split_once(':') {
                    Some((k, v)) => (k.trim().to_lowercase(), v.trim().to_lowercase()),
                    None => continue,
                };
                let cap = match css_float_re().captures(&v) {
                    Some(c) => c,
                    None => continue,
                };
                let val: f64 = match cap[1].parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                match k.as_str() {
                    "top" if top.is_none() => top = Some(val),
                    "left" if left.is_none() => left = Some(val),
                    _ => {}
                }
            }

            (top, left)
        }
    }
}

fn text_of(el: ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join(" ").trim().to_string()
}

fn find_coordinates(el: ElementRef<'_>) -> (Option<f64>, Option<f64>) {
    let mut top: Option<f64> = None;
    let mut left: Option<f64> = None;

    for node in el.ancestors() {
        if let Some(e) = ElementRef::wrap(node) {
            let (t, l) = parse_style(e.value().attr("style"));
            if let (true, Some(v)) = (top.is_none(), t) {
                top = Some(v)
            }
            if let (true, Some(v)) = (left.is_none(), l) {
                left = Some(v)
            }
            if top.is_some() && left.is_some() {
                break;
            }
        }
    }

    (top, left)
}

fn calculate_score(top: Option<f64>, left: Option<f64>) -> f64 {
    match (top, left) {
        (Some(t), Some(l)) => (t * t + l * l).sqrt(),
        _ => f64::INFINITY,
    }
}

fn iter_matches_in_el(el: ElementRef<'_>, kind: Kind) -> Vec<Item> {
    let txt = text_of(el);
    if txt.is_empty() {
        return vec![];
    }
    let is_match = match kind {
        Kind::Date => date_text_re().is_match(&txt),
        Kind::PagoPa => pagopa_text_re().is_match(&txt),
    };
    if !is_match {
        return vec![];
    }
    let (top, left) = find_coordinates(el);
    let score = calculate_score(top, left);
    vec![Item {
        val: clean_text(&txt),
        score,
    }]
}

fn collect_items_for_root(root: ElementRef<'_>, kind: Kind) -> Vec<Item> {
    let selector = Selector::parse("p, span, div").unwrap();
    let mut found: Vec<Item> = Vec::new();
    for el in root.select(&selector) {
        found.extend(iter_matches_in_el(el, kind));
    }
    let mut best: HashMap<String, Item> = HashMap::new();
    for item in found {
        best.entry(item.val.clone())
            .and_modify(|ex| {
                if item.score < ex.score {
                    *ex = item.clone();
                }
            })
            .or_insert(item);
    }
    let mut items: Vec<Item> = best.into_values().collect();
    items.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal));
    items
}
fn process_pages(html_text: &str) -> Vec<(String, String)> {
    let doc = Html::parse_document(html_text);
    let divs_sel = Selector::parse("div[id]").unwrap();

    let mut pages: Vec<(u32, ElementRef)> = Vec::new();
    for el in doc.select(&divs_sel) {
        if let Some(caps) = el
            .value()
            .attr("id")
            .and_then(|id| page_id_re().captures(id))
        {
            if let Ok(n) = caps[1].parse::<u32>() {
                pages.push((n, el))
            }
        }
    }
    pages.sort_by_key(|(n, _)| *n);

    let mut all_pairs = Vec::new();
    for (_n, root) in pages {
        let dates = collect_items_for_root(root, Kind::Date);
        let codes = collect_items_for_root(root, Kind::PagoPa);
        if let (false, false) = (dates.is_empty(), codes.is_empty()) {
            for (d, c) in dates.iter().zip(codes.iter()) {
                all_pairs.push((d.val.clone(), c.val.clone()));
            }
        }
    }
    all_pairs
}

pub fn run_mutool_to_html(path: &Path) -> Result<String, String> {
    let output = Command::new("mutool")
        .args(["convert", "-F", "html", "-o", "-", &path.to_string_lossy()])
        .output()
        .map_err(|_| "Error: 'mutool' not found in PATH.".to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "mutool failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "mutool produced non-UTF8 output".to_string())
}

fn parse_date_to_iso(s: &str) -> Option<String> {
    let date = NaiveDate::parse_from_str(s.trim(), "%d/%m/%Y").ok()?;
    let dt = DateTime::<Utc>::from_naive_utc_and_offset(date.and_hms_opt(0, 0, 0)?, Utc);
    Some(dt.to_rfc3339())
}

fn extract_dates_and_codes_from_html(html_text: &str) -> Vec<DateCodePair> {
    process_pages(html_text)
        .into_iter()
        .filter_map(|(date_str, code)| {
            parse_date_to_iso(&date_str).map(|iso| DateCodePair {
                date: iso,
                code: clean_text(&code),
            })
        })
        .collect()
}

fn pagopa_qr_code_from_payload(payload: &str) -> Option<String> {
    let caps = pagopa_qr_re().captures(payload)?;
    if let Some(code) = caps.name("code1") {
        Some(code.as_str().to_string())
    } else {
        caps.name("code2").map(|code| code.as_str().to_string())
    }
}

fn enrich_barcodes_with_dates(barcodes: &mut [BarcodeData], pairs: &[DateCodePair]) {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|p| (p.code.clone(), p.date.clone()))
        .collect();

    for b in barcodes.iter_mut() {
        if let Some(code) = pagopa_qr_code_from_payload(&b.data) {
            if let Some(date_iso) = map.get(&code) {
                b.date = Some(date_iso.clone());
            }
        }
    }
}
