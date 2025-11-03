FROM rust:latest AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin qr_decoder
RUN curl -LO https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7149/pdfium-linux-x64.tgz &&\
    mkdir $HOME/pdfium &&\
    tar -xvzf pdfium-linux-x64.tgz -C $HOME/pdfium &&\
    mv $HOME/pdfium/lib/libpdfium.so libpdfium.so

FROM debian:trixie-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends mupdf-tools && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/qr_decoder qr_decoder
COPY --from=builder /app/libpdfium.so libpdfium.so
EXPOSE 8000
ENTRYPOINT ["./qr_decoder"]
