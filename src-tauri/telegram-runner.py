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
    from proxy import tg_ws_proxy
    from proxy.config import proxy_config
    assert callable(tg_ws_proxy._run)
    for field in ("host", "port", "secret", "dc_redirects", "fallback_cfproxy", "cfproxy_user_domains", "cfproxy_worker_domains"):
        assert hasattr(proxy_config, field), field


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


def configure_proxy(config):
    from proxy.config import proxy_config, start_cfproxy_domain_refresh
    from proxy import tg_ws_proxy
    proxy_config.host = config.get("host", "127.0.0.1")
    proxy_config.port = config["port"]
    proxy_config.secret = config["secret"]
    proxy_config.dc_redirects = {int(dc): ip for dc, ip in config.get("dc_ips", {2: "149.154.167.220", 4: "149.154.167.220"}).items()}
    proxy_config.fallback_cfproxy = config.get("cfproxy", False)
    proxy_config.cfproxy_user_domains = config.get("cfproxy_domains", [])
    proxy_config.cfproxy_worker_domains = config.get("worker_domains", []) if config.get("worker", False) else []
    tg_ws_proxy.start_cfproxy_domain_refresh = start_cfproxy_domain_refresh if proxy_config.fallback_cfproxy else lambda: None
    return tg_ws_proxy


if __name__ == "__main__":
    initialize()
    if "--rotate-secret" in sys.argv:
        config = json.loads(CONFIG.read_text(encoding="utf-8"))
        config["secret"] = os.urandom(16).hex()
        temporary = CONFIG.with_suffix(".tmp")
        temporary.write_text(json.dumps(config), encoding="utf-8")
        temporary.replace(CONFIG)
        sys.exit(0)
    if "--check" in sys.argv:
        print("ok")
        sys.exit(0)
    threading.Thread(target=watch_parent, args=(int(sys.argv[1]),), daemon=True).start()
    config = json.loads(CONFIG.read_text(encoding="utf-8"))
    tg_ws_proxy = configure_proxy(config)

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
