from llama_cpp import Llama

# Load TinyLlama
print("📥 Loading TinyLlama...")
llm = Llama(
    model_path="models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf",
    n_ctx=2048,  # Context window
    n_threads=12,  # Use all 12 threads
    n_gpu_layers=0,  # CPU only
)

print("✅ Model loaded! Type 'quit' to exit.\n")

while True:
    user_input = input("You: ")
    if user_input.lower() == 'quit':
        break
    
    output = llm(
        user_input,
        max_tokens=200,
        temperature=0.7,
        top_p=0.9,
        repeat_penalty=1.1,
    )
    
    print(f"AI: {output['choices'][0]['text']}\n")
