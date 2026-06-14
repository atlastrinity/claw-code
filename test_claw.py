import subprocess
import time
import sys

p = subprocess.Popen(["claw", "--model", "quick", "--dangerously-skip-permissions"], 
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

time.sleep(2)
p.stdin.write("Привіт\n")
p.stdin.flush()
time.sleep(4)
p.stdin.write("Що ти можеш робити?\n")
p.stdin.flush()
time.sleep(4)

p.terminate()

out, err = p.communicate()
print("STDOUT:", out[-1000:])
print("STDERR:", err)
