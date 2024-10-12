use std::{
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
}

fn main() {
    let args = Args::parse();

    let images = extract_images(&args.input).expect("extracted images");

    for image in images {
        let width = image.width();
        let height = image.height();
        let luma_image = image.into_luma8().as_bytes().into();

        if let Ok(results) = rxing::helpers::detect_multiple_in_luma_with_hints(
            luma_image,
            width,
            height,
            &mut HashMap::from([(
                DecodeHintType::POSSIBLE_FORMATS,
                DecodeHintValue::PossibleFormats(HashSet::from([BarcodeFormat::QR_CODE])),
            )]),
        ) {
            for result in results {
                println!("{} -> {}", result.getBarcodeFormat(), result.getText())
            }
        }
    }
}

fn extract_images(path: &impl AsRef<Path>) -> Result<Vec<DynamicImage>, PdfiumError> {
    let pdfium = Pdfium::default();

    let document = pdfium.load_pdf_from_file(path, None)?;

    let render_config = PdfRenderConfig::new()
        .set_target_width(1000)
        .set_maximum_height(1000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);

    let mut images: Vec<DynamicImage> = Vec::new();

    for page in document.pages().iter() {
        images.push(page.render_with_config(&render_config)?.as_image());
    }

    Ok(images)
}
