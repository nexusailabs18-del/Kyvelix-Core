//! Runtime configuration for M0 binaries.
//! Priority: CLI > env > config file > auto-detect.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub threads: usize,
    #[serde(default = "default_batch")]
    pub batch_size: usize,
    #[serde(default = "default_pack")]
    pub pack_format: bool,
    #[serde(default)]
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            threads: 0,
            batch_size: default_batch(),
            pack_format: default_pack(),
            verbose: false,
        }
    }
}

fn default_batch() -> usize { 50 }
fn default_pack() -> bool { true }

impl Config {
    pub fn load(cli_threads: Option<usize>, cli_batch: Option<usize>) -> Self {
        let mut cfg = Self::from_file().unwrap_or_default();

        if let Ok(v) = std::env::var("M0_THREADS") {
            if let Ok(n) = v.parse() { cfg.threads = n; }
        }
        if let Ok(v) = std::env::var("M0_BATCH_SIZE") {
            if let Ok(n) = v.parse() { cfg.batch_size = n; }
        }
        if std::env::var("M0_VERBOSE").is_ok() {
            cfg.verbose = true;
        }

        if let Some(n) = cli_threads { cfg.threads = n; }
        if let Some(n) = cli_batch { cfg.batch_size = n; }

        cfg
    }

    fn from_file() -> Option<Self> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        candidates.push(PathBuf::from(".m0.toml"));
        candidates.push(PathBuf::from("/etc/m0/config.toml"));
        if let Some(d) = dirs::config_dir() {
            candidates.push(d.join("m0").join("config.toml"));
        }
        for path in candidates {
            if path.exists() {
                if let Ok(s) = std::fs::read_to_string(&path) {
                    if let Ok(cfg) = toml::from_str::<Self>(&s) {
                        eprintln!("config: loaded {}", path.display());
                        return Some(cfg);
                    }
                }
            }
        }
        None
    }

    pub fn resolved_threads(&self) -> usize {
        if self.threads > 0 { self.threads } else { auto_detect_threads() }
    }

    pub fn show(&self) {
        let n = self.resolved_threads();
        let src = if self.threads > 0 { "user-specified" } else { "auto-detected" };
        eprintln!("m0 config:");
        eprintln!("  threads = {} ({})", n, src);
        eprintln!("  batch_size = {}", self.batch_size);
        eprintln!("  pack_format = {}", self.pack_format);
        eprintln!("  verbose = {}", self.verbose);
    }
}

pub fn auto_detect_threads() -> usize {
    let physical = num_cpus::get_physical().max(1);
    let available = num_cpus::get().max(1);
    let base = physical.min(available);
    // One thread per 2 MiB of L3 (aggressive heuristic)
    let l3_cap = l3_cache_mib().map(|m| (m / 2) as usize).unwrap_or(base);
    base.min(l3_cap).max(1)
}

fn l3_cache_mib() -> Option<u64> {
    if let Ok(s) = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cache/index3/size") {
        let s = s.trim();
        if let Some(k) = s.strip_suffix('K') {
            return k.parse::<u64>().ok().map(|v| v / 1024);
        }
        if let Some(m) = s.strip_suffix('M') {
            return m.parse::<u64>().ok();
        }
    }
    None
}
