# Deploying Duck Battles (code → live)

How a code change reaches the live website and game server.

## Local testing first (recommended)

Use the root [`Makefile`](../Makefile) before deploying:

```bash
# terminal 1
make server

# terminal 2
make client

# optional: second player
make client2

# or browser vs local server
make web
```

Against the live backend without deploying a new client binary:

```bash
make client-remote   # native → api.duckbattles.com
make web-remote      # trunk → api.duckbattles.com
```

See `make help` for overrides. Full deploy steps are below.

## Architecture

```text
[your commit]
    │
    ├─► server Dockerfile ─► image (GHCR or docker load) ─► droplet container
    │                                      ▲
    │                                      └── Caddy :443 (api.duckbattles.com)
    │                                            /auth* → :8080
    │                                            /ws*   → :8083
    │
    └─► trunk WASM (+ baked AUTH URL) ─► gh-pages ─► www.duckbattles.com
                                              │
                                              └── browser ─HTTPS/WSS─► api.duckbattles.com
```

| Piece | Live endpoint |
|-------|----------------|
| Web client | https://www.duckbattles.com/ |
| Auth + WSS | https://api.duckbattles.com (`/auth…`, `/ws`) |
| Droplet | `159.203.58.28` (Docker `chexy-server`, Caddy) |
| Native UDP | `159.203.58.28:8081` |

---

## Server (backend)

### Intended CI path

Once `.github/workflows/deploy-server.yml` is on `main` and GitHub secrets are set (`REGISTRY_*`, `DO_SSH_KEY`, `DO_HOST`, `DO_USER`, `SERVER_PUBLIC_IP`):

1. Change server / shared netcode (`src/server/`, `src/demo/lib.rs`, etc.).
2. Commit and push to `main` on `RaminKav/DuckBattles`.
3. Actions **Build & Deploy Server**:
   - `docker build` (Dockerfile → `cargo build --release --features server`)
   - Push `ghcr.io/<owner>/chexy-server:latest`
   - SSH to the droplet, `docker pull`, recreate `chexy-server` with `CHEXY_*` env
4. Clients hit the new process via IP (native) or `api.duckbattles.com` (web).

### Manual path (what we often use)

1. Build the image locally (Docker Desktop running):

   ```bash
   cd /path/to/chexy-butt-balloons
   docker build -t chexy-server:latest .
   ```

2. Copy to the droplet and load:

   ```bash
   docker save chexy-server:latest | gzip | \
     ssh root@159.203.58.28 'gunzip | docker load'
   ```

3. Recreate the container (WSS proxy env required for the web client):

   ```bash
   ssh root@159.203.58.28 'bash -s' <<'EOF'
   set -e
   docker stop chexy-server 2>/dev/null || true
   docker rm chexy-server 2>/dev/null || true
   docker run -d --name chexy-server --restart unless-stopped \
     -e CHEXY_SERVER_BIND_IP=0.0.0.0 \
     -e CHEXY_SERVER_PUBLIC_IP=159.203.58.28 \
     -e CHEXY_SERVER_HTTP_PORT=8080 \
     -e CHEXY_SERVER_NATIVE_PORT=8081 \
     -e CHEXY_SERVER_WT_PORT=8082 \
     -e CHEXY_SERVER_WS_PORT=8083 \
     -e CHEXY_SERVER_WS_DOMAIN=api.duckbattles.com \
     -e CHEXY_SERVER_HAS_WSS_PROXY=true \
     -e CHEXY_SERVER_WS_PORT_PROXY=443 \
     -p 8080:8080 -p 8081:8081/udp -p 8082:8082 -p 8083:8083 \
     chexy-server:latest
   EOF
   ```

4. Caddy on the droplet usually does **not** need changes; it already terminates TLS for `api.duckbattles.com`. Proxy both `/auth*` and `/status*` to `:8080` (main menu polls `GET /status`).

**Verify:**

```bash
curl -sS -X POST 'https://api.duckbattles.com/auth/1?transport=wasm_ws' | head -c 200
curl -sS 'https://api.duckbattles.com/status'
```

---

## Web client (website)

Auth URL and transport are baked into WASM at **compile time** (`option_env!("CHEXY_AUTH_BASE_URL")`, etc.). Changing them requires a rebuild + Pages deploy.

### Intended CI path

`.github/workflows/deploy-client.yml` on `main`:

1. Change client code (`src/demo/client.rs`, `src/screens/`, theme, etc.).
2. Push to `main`.
3. Actions runs `trunk build --release` with:
   - `CHEXY_AUTH_BASE_URL=https://api.duckbattles.com`
   - `CHEXY_CLIENT_TRANSPORT=wasm_ws`
4. Publishes `target/trunk` to the `gh-pages` branch with CNAME `www.duckbattles.com`.

### Manual path

```bash
cd /path/to/chexy-butt-balloons
rustup target add wasm32-unknown-unknown   # once
cargo install trunk --locked               # once

CHEXY_AUTH_BASE_URL=https://api.duckbattles.com \
CHEXY_CLIENT_TRANSPORT=wasm_ws \
trunk build --release

echo 'www.duckbattles.com' > target/trunk/CNAME
# Publish target/trunk to the gh-pages branch (git worktree / CI / peaceiris action)
```

Live site: **https://www.duckbattles.com/**  
DNS: `www` CNAME → `raminkav.github.io`.

---

## Native client (local only)

Not deployed. Run against the live server:

```bash
CHEXY_AUTH_BASE_URL=http://159.203.58.28:8080 \
CHEXY_CLIENT_TRANSPORT=native \
cargo run
```

Or use HTTPS auth:

```bash
CHEXY_AUTH_BASE_URL=https://api.duckbattles.com \
CHEXY_CLIENT_TRANSPORT=native \
cargo run
```

Native reads env at **runtime**; WASM bakes it at build time.

---

## When you must deploy both

Redeploy **server and web client** together if you change shared protocol types, e.g.:

- `PlayerCommand` / `ServerMessages` in `src/demo/lib.rs`
- Connect / lobby / reset behavior that both sides must understand

UI-only client changes → web (or local native) only.  
Server-only logic with no message format change → server only.

---

## Infra cheat sheet

| Item | Notes |
|------|--------|
| Droplet SSH | `ssh root@159.203.58.28` (key in `~/.ssh/id_rsa` + droplet `authorized_keys`) |
| DNS | DigitalOcean Networking → Domains → `duckbattles.com` |
| `api` A | `159.203.58.28` |
| `www` CNAME | `raminkav.github.io` |
| TLS | Caddy on droplet for `api.duckbattles.com` |
| Pages | GitHub repo Pages source = `gh-pages` branch |
