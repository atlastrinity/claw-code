import os
import requests
import json

api_key = "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"
url = "https://openrouter.ai/api/v1/chat/completions"
headers = {
    "Authorization": f"Bearer {api_key}",
    "Content-Type": "application/json"
}

models = [
    "meta-llama/llama-3.3-70b-instruct:free",
    "google/gemma-4-31b-it:free",
    "google/gemma-4-26b-a4b-it:free",
    "qwen/qwen3-coder:free",
    "meta-llama/llama-3.2-3b-instruct:free",
    "liquid/lfm-2.5-1.2b-thinking:free",
    "cognitivecomputations/dolphin-mistral-24b-venice-edition:free"
]

results = {}

for model in models:
    data = {
        "model": model,
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 5
    }
    try:
        response = requests.post(url, headers=headers, json=data)
        if response.status_code == 200:
            results[model] = "✅ Працює"
        else:
            res_json = response.json()
            err_msg = res_json.get("error", {}).get("message", "Error")
            results[model] = f"❌ Помилка: {response.status_code} - {err_msg}"
    except Exception as e:
        results[model] = f"❌ Помилка: {str(e)}"

for m, r in results.items():
    print(f"{m}: {r}")

