from llama_cpp import Llama
import time

print("🧠 Testing ALL Connected LLMs\n")

models = [
    ("TinyLlama", "models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"),
    ("Phi-2", "models/text/phi-2.Q4_K_M.gguf"),
    ("Mistral 7B", "models/text/mistral-7b-v0.1.Q4_K_M.gguf"),
]

prompt = "Generate a TLS cipher suite order for Chrome browser. List 5 cipher IDs."

for name, path in models:
    print(f"=== {name} ===")
    try:
        start = time.time()
        llm = Llama(model_path=path, n_ctx=128, n_threads=12)
        load_time = time.time() - start
        
        start = time.time()
        output = llm(prompt, max_tokens=30, temperature=0.7)
        gen_time = time.time() - start
        
        print(f"Load: {load_time:.1f}s | Generate: {gen_time:.1f}s")
        print(f"Output: {output['choices'][0]['text'][:100]}")
    except Exception as e:
        print(f"❌ Error: {str(e)[:50]}")
    print()
