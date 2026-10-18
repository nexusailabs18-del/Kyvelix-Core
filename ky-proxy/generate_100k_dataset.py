import json
import random

print("📊 Generating 100K TLS fingerprint dataset...")

# Cipher suites database
cipher_suites = {
    0x1301: "TLS_AES_128_GCM_SHA256",
    0x1302: "TLS_AES_256_GCM_SHA384",
    0x1303: "TLS_CHACHA20_POLY1305_SHA256",
    0xC02B: "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
    0xC02F: "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
    0xC02C: "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
    0xC030: "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
    0xCCA9: "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
    0xCCA8: "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
    0x009C: "TLS_RSA_WITH_AES_128_GCM_SHA256",
    0x009D: "TLS_RSA_WITH_AES_256_GCM_SHA384",
}

browsers = [
    "Chrome 120 on Windows 10",
    "Chrome 119 on Windows 11",
    "Firefox 121 on Ubuntu Linux",
    "Safari 17 on macOS Sonoma",
    "Edge 120 on Windows 11",
    "Opera 100 on Windows 10",
    "Chrome 120 on macOS",
    "Firefox 120 on Windows 10",
]

extensions = [
    "SNI", "Supported Versions", "Supported Groups",
    "Signature Algorithms", "ALPN", "Session Ticket",
    "Extended Master Secret", "PSK Key Exchange Modes",
]

finetuning_data = []

for i in range(100_000):
    browser = random.choice(browsers)
    num_ciphers = random.randint(5, 10)
    selected = random.sample(list(cipher_suites.keys()), num_ciphers)
    cipher_names = [cipher_suites[c] for c in selected]
    
    num_ext = random.randint(4, 8)
    selected_ext = random.sample(extensions, num_ext)
    
    prompt = f"Generate TLS fingerprint for {browser}"
    completion = f"Cipher suites: {', '.join(cipher_names)}\nExtensions: {', '.join(selected_ext)}\nTLS version: TLS 1.3"
    
    finetuning_data.append({
        "prompt": prompt,
        "completion": completion
    })
    
    if (i+1) % 10000 == 0:
        print(f"Generated {i+1} samples...")

# Save as JSONL
with open("datasets/tls_fingerprints/finetuning_100k.jsonl", "w") as f:
    for item in finetuning_data:
        f.write(json.dumps(item) + "\n")

print(f"\n✅ Generated {len(finetuning_data)} samples!")
print(f"File: datasets/tls_fingerprints/finetuning_100k.jsonl")

# Check file size
import os
size = os.path.getsize("datasets/tls_fingerprints/finetuning_100k.jsonl")
print(f"Size: {size / (1024*1024):.1f} MB")
