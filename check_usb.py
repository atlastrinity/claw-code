#!/usr/bin/env python3
import subprocess

# Run system_profiler and filter for USB devices
result = subprocess.run(
    ['system_profiler', 'SPUSBDataType'],
    capture_output=True,
    text=True
)

lines = result.stdout.split('\n')

# Filter for lines containing USB-related keywords
for line in lines:
    if any(keyword in line.lower() for keyword in ['wifi', 'wireless', 'airport', 'network', 'usb', 'product', 'vendor', 'id']):
        print(line)
