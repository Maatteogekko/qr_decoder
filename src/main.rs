use std::{
    borrow::BorrowMut,
    collections::{HashMap, HashSet},
    path::Path,
};

use clap::Parser;
use image::{DynamicImage, EncodableLayout};
use pdfium_render::prelude::*;
use rxing::{BarcodeFormat, DecodeHintType, DecodeHintValue};

/// Simple CLI to extract QR codes from a PDF file
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input PDF file
    #[arg(short, long)]
    input: String,
    /// List of barcode formats to detect
    #[arg(short, long, value_delimiter = ',')]
    formats: Option<Vec<BarcodeFormat>>,
}

fn main() {
    let args = Args::parse();

    // TODO JSON output
    // TODO web server
    // TODO docker

    let hints = args.formats.map(|formats| {
        HashMap::from([(
            DecodeHintType::POSSIBLE_FORMATS,
            DecodeHintValue::PossibleFormats(HashSet::from_iter(formats.iter().cloned())),
        )])
    });

    for image in get_images(&args.input) {
        let width = image.width();
        let height = image.height();
        let luma_image: Vec<u8> = image.into_luma8().as_bytes().into();

        if let Ok(results) = hints.clone().map_or(
            rxing::helpers::detect_multiple_in_luma(luma_image.clone(), width, height),
            |mut hints| {
                rxing::helpers::detect_multiple_in_luma_with_hints(
                    luma_image,
                    width,
                    height,
                    hints.borrow_mut(),
                )
            },
        ) {
            for result in results {
                println!("{} -> {}", result.getBarcodeFormat(), result.getText())
            }
        }
    }
}

fn get_images(path: &impl AsRef<Path>) -> Vec<DynamicImage> {
    let kind = infer::get_from_path(path)
        .expect("file read successfully")
        .expect("file type is known");

    match kind.mime_type() {
        "application/pdf" => extract_images(path).expect("extracted images"),
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/tiff" | "image/bmp" => {
            vec![image::open(path).expect("file read successfully")]
        }
        filetype => panic!("Unexpected file type, {filetype}"),
    }
}

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
