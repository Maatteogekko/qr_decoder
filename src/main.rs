use actix_multipart::form::{json::Json as MPJson, tempfile::TempFile, MultipartForm};
use actix_web::{post, App, HttpResponse, HttpServer, Responder, Result};
use image::{DynamicImage, EncodableLayout, ImageFormat};
use pdfium_render::prelude::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rxing::{BarcodeFormat, DecodeHintType, DecodeHintValue, DecodingHintDictionary};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Debug, Deserialize)]
struct Config {
    formats: Option<Vec<BarcodeFormat>>,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    #[multipart(limit = "20MB")]
    file: TempFile,
    json: MPJson<Config>,
}

#[derive(Debug, Serialize)]
struct BarcodeData {
    r#type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    message: String,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || App::new().service(scan_file))
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}

#[post("/scanner/scan")]
async fn scan_file(MultipartForm(form): MultipartForm<UploadForm>) -> impl Responder {
    let file_path = form.file.file.path();
    let hints = create_hints(form.json.formats.clone());

    match process_file(file_path, hints).await {
        Ok(barcodes) => HttpResponse::Ok().json(barcodes),
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            message: e.to_string(),
        }),
    }
}

/// Process the file and extract barcodes.
async fn process_file(
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
                });
            }
        }
    });

    Ok(Arc::into_inner(barcode_list)
        .expect("valid Arc")
        .into_inner()
        .expect("valid Mutex"))
}

/// Creates barcode detection hints from the given formats.
fn create_hints(
    formats: Option<Vec<BarcodeFormat>>,
) -> Option<HashMap<DecodeHintType, DecodeHintValue>> {
    formats.map(|formats| {
        HashMap::from([(
            DecodeHintType::POSSIBLE_FORMATS,
            DecodeHintValue::PossibleFormats(HashSet::from_iter(formats)),
        )])
    })
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

/// Extracts images from a PDF file using the pdfium library.
fn extract_images(path: &impl AsRef<Path>) -> Result<Vec<DynamicImage>, PdfiumError> {
    let pdfium = Pdfium::default();
    let render_config = PdfRenderConfig::new()
        .set_target_width(1000)
        .set_maximum_height(1000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);

    let document = pdfium.load_pdf_from_file(path, None)?;
    let mut images: Vec<DynamicImage> = Vec::new();
    for page in document.pages().iter() {
        images.push(page.render_with_config(&render_config)?.as_image());
    }

    Ok(images)
}
