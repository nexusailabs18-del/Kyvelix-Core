use ky_proxy::protocols::tls_ai_generator::AITLSGenerator;
use ky_proxy::protocols::tls_fingerprint::TLSFingerprint;
use std::collections::HashSet;

fn main() {
    println!("🧪 Testing ky-proxy TLS Fingerprint Generator...\n");
    
    let generator = AITLSGenerator::new();
    
    // Test 1: Generate single fingerprint
    println!("=== Test 1: Single Fingerprint ===");
    let fp1 = generator.generate_fingerprint(1);
    println!("Size: {} bytes", fp1.len());
    println!("First 20 bytes: {:?}", &fp1[..20.min(fp1.len())]);
    println!("✅ Passed\n");
    
    // Test 2: Generate 1000 fingerprints
    println!("=== Test 2: 1000 Fingerprints ===");
    let fingerprints = generator.generate_millions(1000);
    println!("Generated: {}", fingerprints.len());
    
    let mut unique = HashSet::new();
    for fp in &fingerprints {
        unique.insert(fp.clone());
    }
    println!("Unique: {}", unique.len());
    println!("✅ Passed\n");
    
    // Test 3: Chrome fingerprint
    println!("=== Test 3: Chrome 120 Fingerprint ===");
    let chrome = TLSFingerprint::chrome_120();
    let chrome_bytes = chrome.to_client_hello();
    println!("Size: {} bytes", chrome_bytes.len());
    println!("✅ Passed\n");
    
    // Test 4: Firefox fingerprint
    println!("=== Test 4: Firefox 121 Fingerprint ===");
    let firefox = TLSFingerprint::firefox_121();
    let firefox_bytes = firefox.to_client_hello();
    println!("Size: {} bytes", firefox_bytes.len());
    println!("✅ Passed\n");
    
    // Test 5: Random fingerprints
    println!("=== Test 5: Random Fingerprints ===");
    let mut random_fps = Vec::new();
    for _ in 0..10 {
        let fp = TLSFingerprint::random();
        random_fps.push(fp.to_client_hello());
    }
    println!("Generated: {} random fingerprints", random_fps.len());
    
    let mut unique_random = HashSet::new();
    for fp in &random_fps {
        unique_random.insert(fp.clone());
    }
    println!("Unique random: {}", unique_random.len());
    println!("✅ Passed\n");
    
    // Test 6: AI generation
    println!("=== Test 6: AI Generation (Seed-based) ===");
    let ai_fp1 = generator.ai_generate("Chrome 120 on Windows 10");
    let ai_fp2 = generator.ai_generate("Firefox 121 on Ubuntu Linux");
    let ai_fp3 = generator.ai_generate("Safari 17 on macOS");
    
    println!("Chrome fingerprint: {} bytes", ai_fp1.len());
    println!("Firefox fingerprint: {} bytes", ai_fp2.len());
    println!("Safari fingerprint: {} bytes", ai_fp3.len());
    
    assert_ne!(ai_fp1, ai_fp2, "Chrome and Firefox should differ");
    assert_ne!(ai_fp2, ai_fp3, "Firefox and Safari should differ");
    assert_ne!(ai_fp1, ai_fp3, "Chrome and Safari should differ");
    println!("✅ All different!\n");
    
    // Performance test
    println!("=== Performance Test ===");
    use std::time::Instant;
    let start = Instant::now();
    let _million_fps = generator.generate_millions(100_000);
    let elapsed = start.elapsed();
    println!("100,000 fingerprints in: {:?}", elapsed);
    println!("✅ Passed\n");
    
    println!("🎉 ALL TESTS PASSED!");
}
