FROM docker.io/library/rust:1.90.0-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
	pkg-config \
	libssl-dev \
	make \
	gcc \
	libc6-dev \
	&& rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY docs/dist ./docs/dist

# Build release binary
RUN cargo build --release --package polkadot-rest-api

# ---------------------------------

FROM docker.io/library/debian:bookworm-slim

# metadata
ARG VERSION=""
ARG VCS_REF=main
ARG BUILD_DATE=""

LABEL summary="Polkadot REST API." \
	name="parity/polkadot-rest-api" \
	maintainer="devops-team@parity.io" \
	version="${VERSION}" \
	description="Polkadot REST API - Rust implementation of Substrate API Sidecar." \
	io.parity.image.vendor="Parity Technologies" \
	io.parity.image.source="https://github.com/paritytech/polkadot-rest-api/blob/${VCS_REF}/Dockerfile" \
	io.parity.image.documentation="https://github.com/paritytech/polkadot-rest-api/blob/${VCS_REF}/README.md" \
	io.parity.image.revision="${VCS_REF}" \
	io.parity.image.created="${BUILD_DATE}"

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
	ca-certificates \
	&& rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/polkadot-rest-api /usr/local/bin/

ENV RUST_LOG=info
ENV SAS_EXPRESS_PORT=8080
ENV SAS_EXPRESS_BIND_HOST=0.0.0.0

USER nobody
EXPOSE 8080
CMD ["polkadot-rest-api"]
