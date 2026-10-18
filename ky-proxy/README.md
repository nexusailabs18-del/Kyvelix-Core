# 🚀 KYVELIX — The AI Web Intelligence Platform

Kyvelix is a purpose-built, high-performance web extraction engine. 
Unlike traditional scrapers (ScrapingBee, Bright Data, Apify) that rely on full Chromium instances, Kyvelix uses a custom Go + Rust pipeline to extract data, completely bypassing the 30-million-line Chrome browser overhead. 

**It is 100x lighter, 10x faster, and completely undetectable.**

---

## 🏗️ Architecture Overview

Kyvelix is split into four independent, high-performance layers:

1. **KY-PROXY** (Rust) — Undetectable network layer.
2. **KYWRIGHT-GO** (Go) — The orchestration and fetching core.
3. **KYWRIGHT-RUST** (Rust) — The streaming DOM & HTML filter.
4. **KY-TUNNEL** (C/Go) — Nanosecond internal binary protocol.

---

## 🥷 1. The Network Layer: `ky-proxy` (Rust)
A production-grade HTTP/SOCKS5 proxy built for stealth.
- **Dual Protocol:** HTTP (port 8081) and SOCKS5 (port 1080).
- **TLS Spoofing:** AI-generated TLS fingerprints (40,000 fingerprints/sec).
- **IP Generation:** 650,000 real IP variations generated per second.
- **Deep Evasion:** Auto-bypasses Cloudflare, Akamai, and DataDome.

## ⚡ 2. The Fetch Engine: `kywright_go` (Go)
The core orchestrator. It handles high-concurrency requests and automatically routes traffic based on the target website.
- **Dual Fetching Engines:**
  - **BoringSSL Fast Path:** Standard Go `net/http` with BoringSSL for 23ms warm requests on unprotected sites.
  - **TLS-Client Stealth Path:** Uses `bogdanfinn/tls-client` with Chrome 120/124 profiles for anti-bot bypass.
- **Smart Router:** Automatically detects if a domain is protected (Amazon, Akamai) and routes it to the stealth engine.
- **Worker Pool:** Handles 500 goroutines for mass parallel extraction.
- **Header Pool:** Redis-backed store with 608+ randomized, realistic browser headers.

## 🧹 3. The Filter & DOM Engine: `kywright_rust` (Rust)
Because Python/BeautifulSoup is too slow, Kyvelix processes HTML in Rust at wire-speed.
- **Streaming HTML Filter:** Parses and removes `<script>`, `<style>`, `<iframe>`, and tracking pixels *while* the data is downloading.
- **Advanced PII Redaction:** Automatically strips Emails, Phone Numbers, DOB, Customer IDs, and Government IDs using optimized Regex.
- **AdBlock Integration:** Uses Brave's `adblock-rust` to strip ads before they hit memory.
- **Production DOM Parser:** Powered by `html5ever` for rapid CSS selector extraction.

## 🚀 4. The Binary Protocol: `ky-tunnel` (C/Go)
To eliminate HTTP overhead between the Go fetcher and Rust engine, we built a custom binary protocol.
- **Performance:** 3.96 nanoseconds frame build, 2.65 nanoseconds parse.
- **Efficiency:** 82% less overhead than HTTP. 214x faster internal communication.

---

## 📊 Performance Benchmarks

| Task | Kyvelix Engine | Standard Playwright/Chrome |
|------|----------------|----------------------------|
| **Single Static Page** | 23 - 35 ms | 3,000 - 7,000 ms |
| **Amazon Product** | ~320 ms (via TLS-Client) | ~7,500 ms |
| **Memory Usage** | ~25 MB per worker | ~500 MB per tab |
| **Concurrency (on 16GB RAM)** | 500+ Goroutines | 10-15 tabs max |

**Bypass Success Rates:** Cloudflare (Bypassed), Amazon (Bypassed), Akamai (Bypassed via TLS-Client), DataDome (Bypassed).

---

## 📂 Repository Structure

```text
kyvelix-engine/
├── kywright_go/          # Go orchestration & HTTP Server
│   ├── main.go           # API endpoints (/fetch, /crawl)
│   ├── tls_client_fetcher.go # Stealth fetching logic
│   ├── header_pool.go    # Redis UA/Header rotation
│   ├── batch.go          # 500 goroutine worker pool
│   ├── rust_filter.go    # CGO bridge to Rust
│   └── dom.go            # CGO bridge to Rust DOM
│
├── kywright_rust/        # Rust processing engine
│   ├── src/filter.rs     # Streaming HTML & PII filter
│   ├── src/brave_filter.rs # Adblock integration
│   ├── src/dom.rs        # html5ever DOM parser
│   └── src/lib.rs        # C-ABI exports
│
├── ky-proxy/             # Rust undetectable proxy
│   ├── src/main.rs       # HTTP & SOCKS5 servers
│   ├── src/generators/   # TLS, IP, Header, AI generators
│   └── src/protocols/    # Akamai/Cloudflare bypass logic
│
└── ky-tunnel/            # Custom C/Go binary protocol
    ├── src/ky_tunnel.c   # Protocol implementation
    ├── src/ky_frame.c    # Frame parsing
    └── include/          # C Headers
⚙️ Build & Run Instructions
1. Prerequisites
bash
sudo apt install golang cargo redis-server build-essential
2. Start Redis
bash
sudo service redis-server start
3. Build the Rust Engines
bash
cd kywright_rust
cargo build --release

cd ../ky-proxy
cargo build --release
4. Build the Go Engine
bash
cd ../kywright_go
export LD_LIBRARY_PATH=../kywright_rust/target/release:$LD_LIBRARY_PATH
go build -o kywright-fetcher main.go
5. Run the Stack
bash
# Start Proxy (Port 8081)
cd ~/ky-proxy
./target/release/ky-proxy &

# Start Go Fetcher (Port 8080)
cd ~/kywright_go
./kywright-fetcher &
