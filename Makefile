.PHONY: dev build serve clean install

# ── Install frontend deps ────────────────────────────────────────────────── #
install:
	cd frontend && bun install

# ── Production build ────────────────────────────────────────────────────── #
build: install
	cd frontend && bun run build
	cargo build --release

# ── Run the HTTP server (builds everything first) ────────────────────────── #
serve: build
	./target/release/savecodex serve

# ── Dev: Vite HMR + cargo-watch hot-restart ──────────────────────────────── #
# Frontend:  http://localhost:5173  (proxies /api → :7878)
# Backend:   http://localhost:7878
dev: install
	@cargo watch --version > /dev/null 2>&1 || cargo install cargo-watch
	@trap 'kill 0' INT; \
	  (cd frontend && bun run dev) & \
	  cargo watch -q -c -x 'run -- serve' & \
	  wait

# ── Clean artefacts ─────────────────────────────────────────────────────── #
clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
