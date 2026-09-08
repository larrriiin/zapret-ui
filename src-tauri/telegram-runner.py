"""Headless adapter for the pinned, unmodified Flowseal proxy package."""
import asyncio
import ctypes
import json
import logging
from logging.handlers import RotatingFileHandler
import os
from pathlib import Path
import sys
import threading

ROOT = Path(__file__).resolve().parent
CONFIG = ROOT.parent / "config.json"


def initialize():
    if not CONFIG.exists():
        CONFIG.write_text(json.dumps({"port": 1443, "secret": os.urandom(16).hex()}), encoding="utf-8")
    from proxy._aes import Cipher, algorithms, modes
    import certifi
    assert Path(certifi.where()).is_file()
    assert len(Cipher(algorithms.AES(bytes(32)), modes.CTR(bytes(16))).encryptor().update(bytes(16))) == 16


def watch_parent(pid):
    kernel = ctypes.windll.kernel32
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.OpenProcess(0x00100000, False, pid)
    if not handle:
        os._exit(0)
    kernel.WaitForSingleObject(handle, 0xFFFFFFFF)
    kernel.CloseHandle(handle)
    os._exit(0)


if __name__ == "__main__":
    initialize()
    if "--check" in sys.argv:
        print("ok")
        sys.exit(0)
    threading.Thread(target=watch_parent, args=(int(sys.argv[1]),), daemon=True).start()
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    from proxy.config import proxy_config
    from proxy import tg_ws_proxy
    proxy_config.host = "127.0.0.1"
    proxy_config.port = config["port"]
    proxy_config.secret = config["secret"]
    # Keep the default route entirely on Telegram infrastructure.
    proxy_config.fallback_cfproxy = False
    tg_ws_proxy.start_cfproxy_domain_refresh = lambda: None

    class RedactSecret(logging.Filter):
        def filter(self, record):
            record.msg = record.getMessage().replace(config["secret"], "[secret]")
            record.args = ()
            return True

    handler = RotatingFileHandler(ROOT.parent / "proxy.log", maxBytes=1024 * 1024, backupCount=1, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
    handler.addFilter(RedactSecret())
    logging.basicConfig(level=logging.INFO, handlers=[handler])
    asyncio.run(tg_ws_proxy._run())
