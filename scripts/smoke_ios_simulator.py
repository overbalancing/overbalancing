"""Launch the generated application on an available iPhone simulator."""
import json
import subprocess
import time

def run(*args, **kwargs):
    return subprocess.run(args, check=True, timeout=300, **kwargs)

result = run("xcrun", "simctl", "list", "devices", "available", "--json", capture_output=True, text=True)
devices = json.loads(result.stdout)["devices"]
candidates = [device for runtime, rows in devices.items() if "iOS" in runtime
              for device in rows if device.get("isAvailable") and "iPhone" in device["name"]]
if not candidates:
    raise SystemExit("No available iPhone simulator runtime on this runner")
device = candidates[0]
udid = device["udid"]
if device["state"] != "Booted":
    run("xcrun", "simctl", "boot", udid)
run("xcrun", "simctl", "bootstatus", udid, "-b")
run("xcrun", "simctl", "install", udid, "target/ios-simulator/Overbalancing.app")
run("xcrun", "simctl", "launch", "--terminate-running-process", udid, "dev.overbalancing.game")
time.sleep(5)
run("xcrun", "simctl", "io", udid, "screenshot", "target/ios-simulator/launch.png")
print(f"Installed and launched on {device['name']}; screenshot saved (visual review remains required)")
