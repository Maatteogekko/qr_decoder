use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use clap::Parser;
use image::{DynamicImage, EncodableLayout};
use pdfium_render::prelude::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rxing::{BarcodeFormat, DecodeHintType, DecodeHintValue};

/// Simple CLI to extract QR codes from a PDF or image file
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file
    #[arg(short, long)]
    input: String,
    /// Comma separated list of barcode formats to detect.
    /// Supported values are:
    /// - AZTEC
    /// - CODABAR
    /// - CODE_39
    /// - CODE_93
    /// - CODE_128
    /// - DATA_MATRIX
    /// - EAN_8
    /// - EAN_13
    /// - ITF
    /// - MAXICODE
    /// - PDF_417
    /// - QR_CODE
    /// - MICRO_QR_CODE
    /// - RECTANGULAR_MICRO_QR_CODE
    /// - RSS_14
    /// - RSS_EXPANDED
    /// - TELEPEN
    /// - UPC_A
    /// - UPC_E
    /// - UPC_EAN_EXTENSION
    /// - DXFilmEdge
    #[arg(short, long, value_delimiter = ',')]
    formats: Option<Vec<BarcodeFormat>>,
}

fn main() -> Result<(), String> {
    let args = Args::parse();

    // TODO JSON output
    // TODO web server
    // TODO docker

    let hints = create_hints(args.formats);

    get_images(&args.input)?.par_iter().for_each(|image| {
        let width = image.width();
        let height = image.height();
        let luma_image: Vec<u8> = image.clone().into_luma8().as_bytes().into();

        let results = match &mut hints.clone() {
            Some(hints) => {
                rxing::helpers::detect_multiple_in_luma_with_hints(luma_image, width, height, hints)
            }
            None => rxing::helpers::detect_multiple_in_luma(luma_image, width, height),
        };

        match results {
            Ok(results) => {
                for result in results {
                    println!("{} -> {}", result.getBarcodeFormat(), result.getText());
                }
            }
            Err(e) => eprintln!("Error decoding barcodes: {}", e),
        }
    });

    Ok(())
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
    let kind = infer::get_from_path(path).map_err(|_| "Failed to read file".to_string())?;
    let kind = kind.ok_or_else(|| "Unknown file type".to_string())?;

    match kind.mime_type() {
        "application/pdf" => {
            extract_images(path).map_err(|e| format!("Failed to extract images from PDF: {:?}", e))
        }
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/tiff" | "image/bmp" => {
            image::open(path)
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
