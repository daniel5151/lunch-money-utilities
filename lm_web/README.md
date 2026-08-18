# `lm-web`
> Embedded Web GUI for Lunch Money Utilities.

`lm-web` hosts a local web server providing a dynamic 2-column web interface for executing any Lunch Money utility tool.

## Features
- **Dynamic Clap Introspection**: The web UI automatically discovers and builds forms for all tools, subcommands, arguments, flags, and options at runtime.
- **2-Column Split View**:
  - **Left**: Tool & subcommand selector, dynamic form inputs, file upload dropzones, dedicated **Dry Run** and **Run (Live)** buttons, and live CLI syntax preview.
  - **Right**: Streaming ANSI terminal window showing the command executed and real-time stdout/stderr output.
- **Zero Frontend Build Dependencies**: The single-page web app is embedded directly into the binary (`include_str!`). No Node or npm is required to build or run.
- **Local & Private**: Runs locally on `http://127.0.0.1:3000` and proxies external API calls with zero CORS issues.

---

## Local Usage

```console
# Launch the Web GUI (auto-opens default browser at http://127.0.0.1:3000)
$ lm-utils web

# Specify a custom port or disable auto-opening the browser
$ lm-utils web --port 8080 --no-open

# Bind to all network interfaces (e.g. for access on a local network or server)
$ lm-utils web --host 0.0.0.0 --port 3000 --no-open
```

---

## Building a Statically Linked MUSL Binary

Because `lm-utils` uses Rustls with `ring` crypto (no dynamic OpenSSL or libc dependencies), it compiles into a **100% standalone, statically linked binary** using `musl`. The resulting binary can be copied to and executed on **any Linux distribution** (Alpine, Debian, Ubuntu, Arch, CentOS, or a minimal `scratch` Docker container) with zero external runtime dependencies.

### 1. Prerequisites

Ensure the `musl` target and toolchain are installed:

```console
# Add the musl Rust target
$ rustup target add x86_64-unknown-linux-musl

# Install musl tools (Debian / Ubuntu)
$ sudo apt-get update && sudo apt-get install -y musl-tools

# Or on Alpine Linux
$ apk add musl-dev gcc
```

### 2. Build the Static Release Binary

```console
$ cargo build --release --target x86_64-unknown-linux-musl
```

The compiled binary will be located at:
```
target/x86_64-unknown-linux-musl/release/lm-utils
```

### 3. (Optional) Strip Binary for Minimal Size

Strip debug symbols to reduce binary size to ~7 MB:

```console
$ strip --strip-all target/x86_64-unknown-linux-musl/release/lm-utils
```

Verify that the binary is completely statically linked:

```console
$ file target/x86_64-unknown-linux-musl/release/lm-utils
# Output: target/.../lm-utils: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), statically linked

$ ldd target/x86_64-unknown-linux-musl/release/lm-utils
# Output: statically linked (not a dynamic executable)
```

---

## Server Deployment Examples

### Option A: Direct Deployment with `systemd`

1. Copy the binary and your `lm_utils.toml` configuration to the server:
   ```console
   $ scp target/x86_64-unknown-linux-musl/release/lm-utils user@your-server:/usr/local/bin/
   $ scp lm_utils.toml user@your-server:/etc/lm_utils/lm_utils.toml
   ```

2. Create a systemd service file at `/etc/systemd/system/lm-web.service`:
   ```ini
   [Unit]
   Description=Lunch Money Utilities Web Server
   After=network.target

   [Service]
   Type=simple
   User=www-data
   Group=www-data
   WorkingDirectory=/etc/lm_utils
   ExecStart=/usr/local/bin/lm-utils web --host 0.0.0.0 --port 3000 --no-open --config /etc/lm_utils/lm_utils.toml
   Restart=always
   RestartSec=5

   [Install]
   WantedBy=multi-user.target
   ```

3. Enable and start the service:
   ```console
   $ sudo systemctl daemon-reload
   $ sudo systemctl enable --now lm-web.service
   ```

---

### Option B: Minimal Docker Container (`scratch` / `alpine`)

Create a `Dockerfile`:

```dockerfile
# Option 1: Ultra-minimal scratch container
FROM scratch

# Copy the statically linked binary and CA certificates for HTTPS requests
COPY target/x86_64-unknown-linux-musl/release/lm-utils /lm-utils
COPY /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Mount configuration directory
VOLUME /config
WORKDIR /config

EXPOSE 3000

ENTRYPOINT ["/lm-utils", "web", "--host", "0.0.0.0", "--port", "3000", "--no-open", "--config", "/config/lm_utils.toml"]
```

Run with Docker:

```console
$ docker run -d \
    --name lm-web \
    -p 3000:3000 \
    -v $(pwd)/lm_utils.toml:/config/lm_utils.toml:ro \
    lm-web
```
