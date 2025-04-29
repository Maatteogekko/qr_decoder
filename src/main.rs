use actix_multipart::form::{json::Json as MPJson, tempfile::TempFile, MultipartForm};
use actix_web::{get, post, App, HttpResponse, HttpServer, Responder};
use qr_decoder::{create_hints, process_file};
use rxing::BarcodeFormat;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Config {
    formats: Option<Vec<BarcodeFormat>>,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    #[multipart(limit = "20MB")]
    file: TempFile,
    json: Option<MPJson<Config>>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    message: String,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || App::new().service(scan_file))
        .bind(("0.0.0.0", 8000))?
        .run()
        .await
}

#[post("/scanner/scan")]
async fn scan_file(MultipartForm(form): MultipartForm<UploadForm>) -> impl Responder {
    let file_path = form.file.file.path();
    let hints = form
        .json
        .and_then(|some| create_hints(some.formats.clone()));

    match process_file(file_path, hints) {
        Ok(barcodes) => HttpResponse::Ok().json(barcodes),
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            message: e.to_string(),
        }),
    }
}

#[get("/alive")]
async fn health_check() -> impl Responder {
    HttpResponse::Ok()
}
