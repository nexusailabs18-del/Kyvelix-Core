//! SPP Common types and utilities

pub mod ipa;
pub mod points;

use serde::{Deserialize, Serialize};

use crate::layer1::{Layer1Config, DependenceAnalysis};

/// SPP configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SppConfig {
    pub layer1: Layer1Config,
    // pub layer2: Layer2Config,
    // pub layer3: Layer3Config,
    // pub layer4: Layer4Config,
    // pub layer5: Layer5Config,
}

impl Default for SppConfig {
    fn default() -> Self {
        Self {
            layer1: Layer1Config::default(),
        }
    }
}

/// SPP analysis result (combines all layers)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SppResult {
    pub layer1: DependenceAnalysis,
    // pub layer2: Layer2Result,
    // pub layer3: Layer3Result,
    // pub layer4: Layer4Result,
    // pub layer5: Layer5Result,
}

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum SppError {
    #[error("Invalid loop: {0}")]
    InvalidLoop(String),
    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),
    #[error("Certificate verification failed: {0}")]
    CertificateFailed(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

pub type Result<T> = std::result::Result<T, SppError>;

/// Loop stub for when SPP runs independently of UDDP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopStub {
    pub depth: usize,
    pub hash: String,
}

impl LoopStub {
    pub fn new(depth: usize) -> Self {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(format!("loop_{}", depth).as_bytes());
        Self {
            depth,
            hash: format!("{:x}", h.finalize()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::SppConfig;

    #[test]
    fn test_config_default() {
        let config = SppConfig::default();
        assert!(config.layer1.enable_ipa);
    }
}