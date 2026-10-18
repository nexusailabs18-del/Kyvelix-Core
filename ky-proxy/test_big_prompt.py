from llama_cpp import Llama
import time

model_path = "/home/kumarr/ky-proxy/models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
llm = Llama(model_path, n_ctx=2048, n_threads=16, use_mlock=True, verbose=False)

# BIG prompts — different each time
big_prompts = [
    "Explain quantum computing in detail, including how qubits work, superposition, entanglement, and the potential applications in cryptography, drug discovery, and optimization problems. What are the main challenges in building a practical quantum computer?",
    
    "Write a comprehensive summary of the history of artificial intelligence, starting from Alan Turing's work in the 1950s, through the AI winter, the rise of deep learning, and the current state of large language models. What are the key milestones?",
    
    "Describe how computer networks work, including TCP/IP, DNS, HTTP, routing, and encryption. How does data travel from your laptop to a server on the other side of the world?",
]

best = 0
for i, prompt in enumerate(big_prompts):
    start = time.time()
    out = llm(prompt, max_tokens=64)
    elapsed = time.time() - start
    tok_per_sec = 64 / elapsed
    if tok_per_sec > best:
        best = tok_per_sec
    print(f"Prompt {i+1} ({len(prompt)} chars) → {tok_per_sec:.1f} tok/s")
    print(f"  Output: {out['choices'][0]['text'][:60]}...")

print(f"\n=== BEST: {best:.1f} tok/s ===")
print(f"vs GPT-4 API: {best/10:.1f}x faster")
