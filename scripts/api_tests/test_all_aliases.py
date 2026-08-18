import json, os, subprocess
from concurrent.futures import ThreadPoolExecutor, as_completed

with open(".claw.json") as f:
    cfg = json.load(f)

aliases = cfg.get("aliases", {})

print(f"Total aliases to test: {len(aliases)}\n", flush=True)

def test_alias(item):
    alias, target = item
    cmd = [
        "./rust/target/release/claw-analog",
        "-m", alias,
        "--max-turns", "1",
        "Reply with exactly: OK"
    ]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=15)
        output = proc.stdout.strip()
        stderr = proc.stderr.strip()
        if proc.returncode == 0:
            status = "✅ ACTIVE"
            snippet = output.replace("\n", " ")[:60]
            return (alias, target, status, snippet)
        else:
            status = "❌ FAILED"
            err_line = ""
            for line in stderr.split("\n"):
                if any(w in line.lower() for w in ["error", "err", "status", "401", "404", "400", "429"]):
                    err_line = line.strip()
            if not err_line:
                err_line = (stderr or output).replace("\n", " ")[:80]
            return (alias, target, status, err_line[:80])
    except subprocess.TimeoutExpired:
        return (alias, target, "⏱️ TIMEOUT", "Timeout > 15s")
    except Exception as e:
        return (alias, target, "⚠️ ERROR", str(e))

results = []
with ThreadPoolExecutor(max_workers=8) as executor:
    futures = {executor.submit(test_alias, item): item[0] for item in aliases.items()}
    for f in as_completed(futures):
        res = f.result()
        results.append(res)
        alias, target, status, detail = res
        print(f"[{status}] {alias:<18} -> {target:<50} | {detail}", flush=True)

results.sort(key=lambda x: x[0])
print("\n" + "="*90, flush=True)
active_count = sum(1 for r in results if "✅" in r[2])
print(f"Summary: {active_count}/{len(results)} active\n", flush=True)

for r in results:
    print(f"  {r[2]}  {r[0]:<18} ({r[1]}): {r[3]}", flush=True)
