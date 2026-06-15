import json

with open('.claw/sessions/eb87729211fe113f/session-1781462079785-0.jsonl', 'r') as f:
    for line in f:
        data = json.loads(line)
        if 'message' in data and data['message']['role'] == 'user':
            pass
