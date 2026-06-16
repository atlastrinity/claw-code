import urllib.request
import json

API_KEY = "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"
MODEL_ID = "meta-llama/llama-3.3-70b-instruct:free"

def test_model():
    url = "https://openrouter.ai/api/v1/chat/completions"
    headers = {
        "Authorization": f"Bearer {API_KEY}",
        "Content-Type": "application/json"
    }
    data = json.dumps({
        "model": MODEL_ID,
        "messages": [{"role": "user", "content": "Привіт! Ти працюєш?"}]
    }).encode('utf-8')
    
    try:
        req = urllib.request.Request(url, data=data, headers=headers, method='POST')
        with urllib.request.urlopen(req, timeout=15) as response:
            res_body = response.read().decode('utf-8')
            return f"✅ Відповідає!\nВідповідь: {res_body}"
    except Exception as e:
        return f"❌ Помилка: {str(e)}"

print(test_model())
