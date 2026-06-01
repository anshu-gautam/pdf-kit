# pdfkit-api container (PRD §15).
#
# Built with --features render-pdfium and the PDFIUM native library bundled, so
# /v1/render works for text PDFs. Terminate TLS at a reverse proxy in front.
#
#   docker build -f crates/pdfkit-api/Dockerfile -t pdfkit-api .
#   docker run -p 8080:8080 -e PDFKIT_ALLOWED_ORIGINS=https://app.example.com pdfkit-api

FROM rust:1.82-bookworm AS build
WORKDIR /src
COPY . .
# scripts/fetch-pdfium.sh downloads the prebuilt PDFIUM into the cache dir used
# by the runtime loader; copy it into the image below.
RUN cargo build --release -p pdfkit-api --features render-pdfium \
 && ./scripts/fetch-pdfium.sh

FROM debian:bookworm-slim AS runtime
# PDFIUM needs a C++ runtime at load time.
RUN apt-get update \
 && apt-get install -y --no-install-recommends libstdc++6 ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/pdfkit-api /usr/local/bin/pdfkit-api
# The runtime loader looks for PDFIUM at $PDFKIT_PDFIUM_LIB, then this cache dir.
COPY --from=build /root/.cache/pdfkit/pdfium /opt/pdfium
ENV PDFKIT_PDFIUM_LIB=/opt/pdfium/lib/libpdfium.so \
    PDFKIT_API_ADDR=0.0.0.0:8080 \
    PDFKIT_MAX_BODY_BYTES=52428800 \
    PDFKIT_REQUEST_TIMEOUT_SECS=60
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
  CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["pdfkit-api"]
