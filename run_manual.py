import json, os, sys, time
    fn write_manager_mcp_server_script() -> PathBuf {
        let root = temp_dir();
        fs::create_dir_all(&root).expect("temp dir");
        let script_path = root.join("manager-mcp-server.py");
        let script = [
#!/usr/bin/env python3
import json, os, sys, time

LABEL = os.environ.get('MCP_SERVER_LABEL', 'server')
LOG_PATH = os.environ.get('MCP_LOG_PATH')
EXIT_AFTER_TOOLS_LIST = os.environ.get('MCP_EXIT_AFTER_TOOLS_LIST') == '1'
FAIL_ONCE_MODE = os.environ.get('MCP_FAIL_ONCE_MODE')
FAIL_ONCE_MARKER = os.environ.get('MCP_FAIL_ONCE_MARKER')
initialize_count = 0

def log(method):
    if LOG_PATH:
        with open(LOG_PATH, 'a', encoding='utf-8') as handle:
            handle.write(f'{method}\\n')

def should_fail_once():
    if not FAIL_ONCE_MODE or not FAIL_ONCE_MARKER:
        return False
    if os.path.exists(FAIL_ONCE_MARKER):
        return False
    with open(FAIL_ONCE_MARKER, 'w', encoding='utf-8') as handle:
        handle.write(FAIL_ONCE_MODE)
    return True

def read_message():
    line = sys.stdin.buffer.readline()
    if not line:
        return None
    return json.loads(line.decode())

def send_message(message):
    payload = json.dumps(message).encode()
            r"    sys.stdout.buffer.write(payload + b'\n')",
    sys.stdout.buffer.flush()

while True:
    request = read_message()
    if request is None:
        break
    method = request['method']
    log(method)
    if method == 'initialize':
        if FAIL_ONCE_MODE == 'initialize_hang' and should_fail_once():
            log('initialize-hang')
            while True:
                time.sleep(1)
        initialize_count += 1
        send_message({
            'jsonrpc': '2.0',
            'id': request['id'],
            'result': {
                'protocolVersion': request['params']['protocolVersion'],
                'capabilities': {'tools': {}},
                'serverInfo': {'name': LABEL, 'version': '1.0.0'}
            }
        })
    elif method == 'tools/list':
        send_message({
            'jsonrpc': '2.0',
            'id': request['id'],
            'result': {
                'tools': [
                    {
                        'name': 'echo',
                        'description': f'Echo tool for {LABEL}',
                        'inputSchema': {
                            'type': 'object',
                            'properties': {'text': {'type': 'string'}},
                            'required': ['text']
                        }
                    }
                ]
            }
        })
        if EXIT_AFTER_TOOLS_LIST:
            raise SystemExit(0)
    elif method == 'tools/call':
        if FAIL_ONCE_MODE == 'tool_call_disconnect' and should_fail_once():
            log('tools/call-disconnect')
            raise SystemExit(0)
        args = request['params'].get('arguments') or {}
        text = args.get('text', '')
        send_message({
            'jsonrpc': '2.0',
            'id': request['id'],
            'result': {
                'content': [{'type': 'text', 'text': f'{LABEL}:{text}'}],
                'structuredContent': {
                    'server': LABEL,
                    'echoed': text,
                    'initializeCount': initialize_count
                },
                'isError': False
            }
        })
    else:
        send_message({
            'jsonrpc': '2.0',
            'id': request['id'],
            'error': {'code': -32601, 'message': f'unknown method: {method}'},
