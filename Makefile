# KALICUT — common Linux targets
.PHONY: help deps build deb appimage portable all clean docker-build docker-out

help:
	@echo "Targets:"
	@echo "  make deps       - install distro build dependencies"
	@echo "  make build      - cargo build --release"
	@echo "  make deb        - .deb with bundled libs (host: ffmpeg only)"
	@echo "  make appimage   - AppImage with bundled libs (host: ffmpeg only)"
	@echo "  make portable   - portable tar.gz with bundled libs"
	@echo "  make all        - binary + portable + .deb + AppImage"
	@echo "  make docker-out - build packages inside Docker → ./dist"
	@echo "  make clean      - remove dist/ and cargo target/"

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
