# Stage 1: Use a standard Rust image for building (glibc-based)
FROM rust:latest AS chef
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Create a dependency lock file with cargo-chef
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build dependencies and the application
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin qr_decoder

# Stage 4: Use a newer Debian runtime with updated glibc
FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/qr_decoder qr_decoder
COPY --from=builder /app/libpdfium.so libpdfium.so
EXPOSE 8080
ENTRYPOINT ["./qr_decoder"]