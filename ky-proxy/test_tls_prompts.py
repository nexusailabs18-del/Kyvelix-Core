from llama_cpp import Llama
import time

print("🎯 Testing Models with TLS-Specific Prompts\n")

# Load models once
print("Loading TinyLlama...")
tinyllama = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=256, n_threads=12)

print("Loading Phi-2...")
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=256, n_threads=12)

print("✅ Models loaded!\n")

# Test prompts
prompts = [
    "List cipher suites: 1301, 1302, 1303, C02B, C02F. Now list 5 more:",
    "Chrome uses these TLS ciphers:",
    "Generate realistic browser TLS fingerprint pattern:",
]

for i, prompt in enumerate(prompts, 1):
    print(f"=== PROMPT {i}: '{prompt}' ===")
    
    # TinyLlama
    start = time.time()
    out1 = tinyllama(prompt, max_tokens=20, temperature=0.1)
    t1 = time.time() - start
    
    # Phi-2
    start = time.time()
    out2 = phi2(prompt, max_tokens=20, temperature=0.1)
    t2 = time.time() - start
    
    print(f"TinyLlama ({t1:.1f}s): {out1['choices'][0]['text'][:80]}")
    print(f"Phi-2 ({t2:.1f}s): {out2['choices'][0]['text'][:80]}")
    print()
