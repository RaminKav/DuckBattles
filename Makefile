# Local rapid-test helpers for Duck Battles.
# Run `make help` for targets.
#
# Typical loop:
#   terminal 1:  make server
#   terminal 2:  make client
# Optional second client: make client   (another terminal)
# Web against local server: make web

.PHONY: help server client client2 client-remote web web-remote check check-server

AUTH_LOCAL  ?= http://127.0.0.1:8080
AUTH_REMOTE ?= https://api.duckbattles.com
TRANSPORT_NATIVE ?= native
TRANSPORT_WASM   ?= wasm_ws

PUBLIC_IP ?= 127.0.0.1
BIND_IP   ?= 0.0.0.0

help:
	@echo "Duck Battles — local test commands"
	@echo ""
	@echo "  make server          Headless game server on :8080–8083 (localhost)"
	@echo "  make client          Native client → local server"
	@echo "  make client2         Same as client (second window / second player)"
	@echo "  make client-remote   Native client → live api.duckbattles.com"
	@echo "  make web             Trunk web client → local server (wasm_ws)"
	@echo "  make web-remote      Trunk web client → live API (wasm_ws)"
	@echo "  make check           cargo check (client defaults)"
	@echo "  make check-server    cargo check --features server"
	@echo ""
	@echo "Override examples:"
	@echo "  make client AUTH_LOCAL=http://127.0.0.1:8080"
	@echo "  make server PUBLIC_IP=127.0.0.1"

# Headless multiplayer server (matches deploy env shape, no TLS proxy).
server:
	CHEXY_SERVER_BIND_IP=$(BIND_IP) \
	CHEXY_SERVER_PUBLIC_IP=$(PUBLIC_IP) \
	CHEXY_SERVER_HTTP_PORT=8080 \
	CHEXY_SERVER_NATIVE_PORT=8081 \
	CHEXY_SERVER_WT_PORT=8082 \
	CHEXY_SERVER_WS_PORT=8083 \
	CHEXY_SERVER_HAS_WSS_PROXY=false \
	cargo run --features server

# Native client against local server.
client client2:
	CHEXY_AUTH_BASE_URL=$(AUTH_LOCAL) \
	CHEXY_CLIENT_TRANSPORT=$(TRANSPORT_NATIVE) \
	cargo run

# Native client against the deployed backend.
client-remote:
	CHEXY_AUTH_BASE_URL=$(AUTH_REMOTE) \
	CHEXY_CLIENT_TRANSPORT=$(TRANSPORT_NATIVE) \
	cargo run

# Browser client (Trunk) against local server — uses plain ws:// via wasm_ws.
web:
	CHEXY_AUTH_BASE_URL=$(AUTH_LOCAL) \
	CHEXY_CLIENT_TRANSPORT=$(TRANSPORT_WASM) \
	env -u NO_COLOR trunk serve

# Browser client against live HTTPS/WSS API.
web-remote:
	CHEXY_AUTH_BASE_URL=$(AUTH_REMOTE) \
	CHEXY_CLIENT_TRANSPORT=$(TRANSPORT_WASM) \
	env -u NO_COLOR trunk serve

check:
	cargo check

check-server:
	cargo check --features server
