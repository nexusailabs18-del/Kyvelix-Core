from llama_cpp import Llama
import time
import threading
import queue

print("🚀 FAST Parallel AI (No Mistral - Using TinyLlama + Phi-2 only)\n")

# Load fast models only
tiny1 = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=6, verbose=False)
tiny2 = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=6, verbose=False)
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=32, n_threads=6, verbose=False)

print("✅ 2x TinyLlama + Phi-2 loaded (no Mistral!)\n")

counts = {"Tiny1": 0, "Tiny2": 0, "Phi2": 0, "Generated": 0}
start_time = time.time()
pattern_queue = queue.Queue()

def tiny1_worker():
    while True:
        out = tiny1("TLS ciphers:", max_tokens=5)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["Tiny1"] += 1

def tiny2_worker():
    while True:
        out = tiny2("Headers:", max_tokens=5)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["Tiny2"] += 1

def phi2_worker():
    while True:
        out = phi2("User-Agent:", max_tokens=8)
        pattern_queue.put(out['choices'][0]['text'].strip())
        counts["Phi2"] += 1

# Start all in parallel
threading.Thread(target=tiny1_worker, daemon=True).start()
threading.Thread(target=tiny2_worker, daemon=True).start()
threading.Thread(target=phi2_worker, daemon=True).start()

while True:
    try:
        try:
            pattern = pattern_queue.get_nowait()
            counts["Generated"] += 100
        except:
            pass
        
        if counts["Generated"] % 5000 == 0:
            elapsed = time.time() - start_time
            total_p = counts["Tiny1"] + counts["Tiny2"] + counts["Phi2"]
            print(f"\r📊 Items: {counts['Generated']} | Patterns: {total_p} | "
                  f"Tiny1:{counts['Tiny1']} Tiny2:{counts['Tiny2']} Phi2:{counts['Phi2']} | "
                  f"{counts['Generated']/elapsed:.0f}/s", end="")
        
    except KeyboardInterrupt:
        elapsed = time.time() - start_time
        print(f"\n\n✅ Final: {counts['Generated']} items")
        print(f"Speed: {counts['Generated']/elapsed:.0f} items/sec")
        print(f"Tiny1: {counts['Tiny1']} | Tiny2: {counts['Tiny2']} | Phi2: {counts['Phi2']}")
        break
