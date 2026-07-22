# KALICUT — reproducible Linux build image (Ubuntu 22.04 for broader glibc reach).
#
# Build artifacts into ./dist on the host:
#   docker build -t kalicut-builder .
#   docker run --rm -v "$PWD/dist:/out" kalicut-builder
#
# Or build only:
#   docker build --target export -o dist .

FROM ubuntu:22.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_TERM_COLOR=always
ENV APPIMAGE_EXTRACT_AND_RUN=1

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    curl \
    ca-certificates \
    file \
    git \
    dpkg-dev \
    libmpv-dev \
    libasound2-dev \
    libx11-dev \
    libxkbcommon-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libgtk-3-dev \
    ffmpeg \
    imagemagick \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /src
COPY . .

# Icons may already be PNGs in repo; regenerate if needed
RUN if [ ! -f packaging/icons/kalicut.png ] && [ -f packaging/icons/kalicut.svg ]; then \
      convert -background none packaging/icons/kalicut.svg -resize 256x256 packaging/icons/kalicut.png; \
    fi

RUN chmod +x scripts/*.sh \
 && ./scripts/build.sh \
 && ./scripts/package-deb.sh \
 && (./scripts/package-appimage.sh || echo "AppImage step skipped/failed — .deb still available")

# Collect artifacts
RUN mkdir -p /out \
 && cp -a dist/*.deb /out/ 2>/dev/null || true \
 && cp -a dist/*.AppImage /out/ 2>/dev/null || true \
 && cp -a target/release/kalicut /out/kalicut 2>/dev/null || true \
 && ls -lah /out

FROM scratch AS export
COPY --from=builder /out /

# Default: copy artifacts to mounted /out
FROM builder AS runtime
CMD ["bash", "-c", "mkdir -p /out && cp -a /out-src/. /out/ 2>/dev/null; cp -a dist/*.deb dist/*.AppImage target/release/kalicut /out/ 2>/dev/null; ls -lah /out"]
