"""Opt-in Windows integration smoke test using already downloaded locked archives.

Runs inside artifacts/tg-smoke, never touches the user's installed module/config.
"""
import hashlib
import ctypes
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "artifacts/tg-smoke"
MODULE = WORK / "module"
MODULE.mkdir(parents=True, exist_ok=True)
lock = json.loads((ROOT / "src-tauri/telegram-module.json").read_text())
archives = [ROOT / "artifacts/tg-downloads/python.zip", ROOT / "artifacts/tg-source/source.zip"] + sorted((ROOT / "artifacts/tg-downloads").glob("*.whl"))
for item, archive in zip(lock["artifacts"], archives):
    data = archive.read_bytes()
    assert len(data) == item["size"] and hashlib.sha256(data).hexdigest() == item["sha256"]
    with zipfile.ZipFile(archive) as z:
        if item["kind"] == "source":
            for name in z.namelist():
                relative = Path(*Path(name).parts[1:])
                if not relative.parts or (relative.parts[0] != "proxy" and str(relative) != "LICENSE") or name.endswith("/"):
                    continue
                target = MODULE / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(z.read(name))
        else:
            z.extractall(MODULE)
(MODULE / "python313._pth").write_text("python313.zip\n.\n")
shutil.copy(ROOT / "src-tauri/telegram-runner.py", MODULE / "runner.py")
command = [str(MODULE / "python.exe"), "-I", str(MODULE / "runner.py")]
flags = subprocess.CREATE_NO_WINDOW
subprocess.run(command + ["--check"], check=True, timeout=20, creationflags=flags)
cfg_path = WORK / "config.json"
cfg = json.loads(cfg_path.read_text())
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    cfg["port"] = sock.getsockname()[1]
cfg_path.write_text(json.dumps(cfg))
child = subprocess.Popen(command + [str(os.getpid())], creationflags=flags)
try:
    for attempt in range(100):
        assert child.poll() is None, "Proxy exited early"
        try:
            with socket.create_connection(("127.0.0.1", cfg["port"]), timeout=.2):
                break
        except OSError:
            time.sleep(.1)
    else:
        raise AssertionError("Listener never became ready")
    print("PASS: isolated runtime, crypto, configuration and loopback listener")
    assert cfg["secret"] not in (WORK / "proxy.log").read_text(), "Secret leaked into log"
    print("PASS: proxy secret is redacted")
finally:
    child.terminate()
    child.wait(timeout=10)
subprocess.run(command + ["--check"], check=True, timeout=20, creationflags=flags)
assert json.loads(cfg_path.read_text())["secret"] == cfg["secret"]
print("PASS: configuration survives restart")
parent_code = """
import json, os, subprocess, sys, time
child = subprocess.Popen(json.loads(sys.argv[1]) + [str(os.getpid())], creationflags=subprocess.CREATE_NO_WINDOW)
print(child.pid, flush=True)
time.sleep(2)
"""
parent = subprocess.Popen([sys.executable, "-c", parent_code, json.dumps(command)], stdout=subprocess.PIPE, text=True, creationflags=flags)
pid = int(parent.stdout.readline())
kernel = ctypes.windll.kernel32
kernel.OpenProcess.restype = ctypes.c_void_p
kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
kernel.CloseHandle.argtypes = [ctypes.c_void_p]
handle = kernel.OpenProcess(0x00100000, False, pid)
assert handle, "Watchdog child did not start"
try:
    parent.wait(timeout=10)
    assert kernel.WaitForSingleObject(handle, 5000) == 0, "Proxy survived its parent"
finally:
    kernel.CloseHandle(handle)
print("PASS: proxy exits automatically when its parent exits")
print(f"Installed module bytes: {sum(p.stat().st_size for p in MODULE.rglob('*') if p.is_file()):,}")
