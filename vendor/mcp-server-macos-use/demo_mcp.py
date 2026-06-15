import json
import subprocess
import sys
import time

class MCPClient:
    def __init__(self, executable_path):
        self.proc = subprocess.Popen(
            [executable_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=sys.stderr
        )
        self.msg_id = 1
        self.initialize()

    def send_request(self, method, params=None):
        payload = {
            "jsonrpc": "2.0",
            "id": self.msg_id,
            "method": method
        }
        if params is not None:
            payload["params"] = params
        self.msg_id += 1
        body = json.dumps(payload)
        message = f"Content-Length: {len(body)}\r\n\r\n{body}"
        self.proc.stdin.write(message.encode('utf-8'))
        self.proc.stdin.flush()
        return self.read_response()
        
    def send_notification(self, method, params=None):
        payload = {
            "jsonrpc": "2.0",
            "method": method
        }
        if params is not None:
            payload["params"] = params
        body = json.dumps(payload)
        message = f"Content-Length: {len(body)}\r\n\r\n{body}"
        self.proc.stdin.write(message.encode('utf-8'))
        self.proc.stdin.flush()

    def read_response(self):
        content_length = 0
        while True:
            line = self.proc.stdout.readline().decode('utf-8')
            if not line:
                return None
            if not line.strip():
                break
            if line.lower().startswith('content-length:'):
                content_length = int(line.split(':')[1].strip())
        if content_length > 0:
            body = self.proc.stdout.read(content_length).decode('utf-8')
            return json.loads(body)
        return None

    def initialize(self):
        print("Initializing...")
        self.send_request("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        })
        self.send_notification("notifications/initialized")
        print("Initialized.")

    def call_tool(self, name, arguments):
        return self.send_request("tools/call", {
            "name": name,
            "arguments": arguments
        })

    def close(self):
        self.proc.terminate()

if __name__ == "__main__":
    client = MCPClient("./.build/release/mcp-server-macos-use")
    
    print("Analyzing screen with visionTool to find Antigravity input...")
    res = client.call_tool("macos-use_analyze_screen", {"language": "auto", "confidence": False})
    
    # Try to find input coordinates
    if res and "result" in res and "content" in res["result"]:
        content = res["result"]["content"]
        text_output = ""
        for item in content:
            if item["type"] == "text":
                text_output += item["text"]
                
        # print("OCR Output:", text_output)
        
        target_x, target_y = None, None
        
        # In OCR output, elements might look like:
        # text "Type a message" at [100, 200, 300, 50]
        # Let's search for some keyword that might be in the Antigravity chat input!
        # "Message Antigravity", "Type a message", "Message", etc.
        lines = text_output.split('\n')
        for line in lines:
            if "Message" in line or "Type" in line or "Antigravity" in line or "chat" in line.lower():
                print("Found match:", line)
                import re
                match = re.search(r'\[(\d+),\s*(\d+),\s*(\d+),\s*(\d+)\]', line)
                if match:
                    x1, y1, x2, y2 = map(int, match.groups())
                    target_x = (x1 + x2) / 2
                    target_y = (y1 + y2) / 2
                    break
                    
        if target_x and target_y:
            print(f"Clicking at {target_x}, {target_y}")
            client.call_tool("macos-use_click", {"x": target_x, "y": target_y})
            time.sleep(0.5)
            
            print("Typing message...")
            client.call_tool("macos-use_type", {"text": "Привіт! Я знайшов чат за допомогою зору і тепер можу автоматично друкувати! 😎\n"})
            print("Done!")
        else:
            print("Could not find chat input automatically via OCR.")
    
    client.close()
