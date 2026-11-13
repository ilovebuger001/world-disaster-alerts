# Single Rust binary + embedded redb + static frontend (spec §2.1, §13).
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
 && useradd --system --home /app wda
WORKDIR /app
COPY --from=build /src/target/release/wda-server /app/wda-server
COPY config ./config
COPY frontend ./frontend
COPY reference ./reference
RUN mkdir -p /app/data && chown -R wda /app/data
USER wda
ENV WDA_CONFIG=/app/config/app.toml
EXPOSE 8080
VOLUME ["/app/data"]
CMD ["/app/wda-server"]
