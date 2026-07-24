# KALICUT — common Linux targets
.PHONY: help deps build deb appimage portable macos all clean docker-build docker-out

help:
	@echo "Targets:"
	@echo "  make deps       - install distro build dependencies"
	@echo "  make build      - cargo build --release"
	@echo "  make deb        - .deb with bundled libs (host arch)"
	@echo "  make appimage   - AppImage with bundled libs (host arch)"
	@echo "  make portable   - portable tar.gz with bundled libs (host arch)"
	@echo "  make macos      - macOS package (run on Mac / CI only)"
	@echo "  make all        - binary + portable + .deb + AppImage (Linux host arch)"
	@echo "  make docker-out - build packages inside Docker → ./dist"
	@echo "  make clean      - remove dist/ and cargo target/"
	@echo ""
	@echo "CI builds both Linux arches (x86_64 + aarch64) and Windows (x64 + ARM)."

deps:
	./scripts/install-deps.sh

build:
	./scripts/build.sh

deb:
	./scripts/package-deb.sh

appimage:
	./scripts/package-appimage.sh

portable:
	./scripts/package-portable.sh

macos:
	./scripts/package-macos.sh

all:
	./scripts/package-all.sh

docker-build:
	docker build -t kalicut-builder .

docker-out: docker-build
	mkdir -p dist
	docker run --rm -v "$(CURDIR)/dist:/out" kalicut-builder \
		bash -c 'cp -a dist/*.deb dist/*.AppImage target/release/kalicut /out/ 2>/dev/null; ls -lah /out'

clean:
	rm -rf dist
	cargo clean
