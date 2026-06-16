import urllib.request
import json
import time

API_KEY = "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"

def get_free_models():
    try:
        with urllib.request.urlopen("https://openrouter.ai/api/v1/models") as response:
            data = json.loads(response.read().decode('utf-8'))
            return [m['id'] for m in data['data'] if m.get('pricing', {}).get('prompt') == "0" and m.get('pricing', {}).get('completion') == "0"]
    except Exception as e:
        print(f"Error fetching models: {e}")
        return []

def test_model(model_id):
    url = "https://openrouter.ai/api/v1/chat/completions"
    headers = {
        "Authorization": f"Bearer {API_KEY}",
        "Content-Type": "application/json"
    }
    data = json.dumps({
        "model": model_id,
        "messages": [{"role": "user", "content": "Hi"}]
    }).encode('utf-8')
    
    try:
        req = urllib.request.Request(url, data=data, headers=headers, method='POST')
        with urllib.request.urlopen(req, timeout=10) as response:
            return True
    except Exception as e:
        return False

models = get_free_models()
print(f"Testing {len(models)} free models...\n")

for m in models:
    print(f"Testing {m}...", end=" ", flush=True)
    if test_model(m):
        print("✅ WORKING!")
        print(f"\nFOUND WORKING MODEL: {m}")
        exit(0) # Stop as soon as we find one
    else:
        print("❌")
    time.sleep(0.5) # Small delay to avoid aggressive rate limiting

print("\nNo working free models found at the moment.")
