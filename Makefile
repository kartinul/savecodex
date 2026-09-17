.PHONY: dev build serve clean install test

install:
	cd frontend && bun install

build: install
	cd frontend && bun run build
	cargo build --release

serve: build
	./target/release/savecodex serve

# Frontend:  http://localhost:5173  (proxies /api → :7878)
# Backend:   http://localhost:7878
dev: install
	@cargo watch --version > /dev/null 2>&1 || cargo install cargo-watch
	@trap 'kill 0' INT; \
	  (cd frontend && bun run dev) & \
	  cargo watch -q -c -x 'run -- serve' & \
	  wait

clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules

test:
	cargo run -- pack ./test --style macos --theme dark --page-break
