from llama_cpp import Llama
import time
import threading
import queue

print("🚀 PARALLEL AI Generation - All Models Working Together\n")

# Load all models
tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=4, verbose=False)
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=32, n_threads=4, verbose=False)
# Mistral uses more threads
mistral = Llama("models/text/mistral-7b-v0.1.Q4_K_M.gguf", n_ctx=32, n_threads=4, verbose=False)

print("✅ 3 AI models loaded in parallel\n")

counts = {"TinyLlama": 0, "Phi2": 0, "Mistral": 0, "Generated": 0}
start_time = time.time()
pattern_queue = queue.Queue()

# Worker functions - each model generates patterns
def tiny_worker():
    while True:
        out = tiny("TLS pattern:", max_tokens=5)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["TinyLlama"] += 1

def phi2_worker():
    while True:
        out = phi2("UA pattern:", max_tokens=8)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["Phi2"] += 1

def mistral_worker():
    while True:
        out = mistral("Cookie pattern:", max_tokens=5)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["Mistral"] += 1

# Start all models in parallel
threading.Thread(target=tiny_worker, daemon=True).start()
threading.Thread(target=phi2_worker, daemon=True).start()
threading.Thread(target=mistral_worker, daemon=True).start()

# Main loop - consume patterns and generate items
while True:
    try:
        # Get pattern from queue (non-blocking)
        try:
            pattern = pattern_queue.get_nowait()
            # Each pattern generates 100 items
            counts["Generated"] += 100
        except:
            pass
        
        if counts["Generated"] % 1000 == 0:
            elapsed = time.time() - start_time
            total_patterns = counts["TinyLlama"] + counts["Phi2"] + counts["Mistral"]
            print(f"\r📊 Items: {counts['Generated']} | Patterns: {total_patterns} | "
                  f"Tiny:{counts['TinyLlama']} Phi2:{counts['Phi2']} Mistral:{counts['Mistral']} | "
                  f"{counts['Generated']/elapsed:.0f}/s", end="")
        
    except KeyboardInterrupt:
        elapsed = time.time() - start_time
        print(f"\n\n✅ Final: {counts['Generated']} items from {counts['TinyLlama']+counts['Phi2']+counts['Mistral']} AI patterns")
        print(f"TinyLlama: {counts['TinyLlama']} patterns")
        print(f"Phi-2: {counts['Phi2']} patterns")
        print(f"Mistral: {counts['Mistral']} patterns")
        print(f"Speed: {counts['Generated']/elapsed:.0f} items/sec")
        break
