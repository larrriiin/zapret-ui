"""Refresh the reviewed x64 download lock; never runs on end-user machines.

First download binary wheels into artifacts/tg-downloads with pip download
--only-binary=:all: --platform win_amd64 --python-version 313 cryptography certifi.
Review dependency/upstream changes and test the module before committing the lock.
"""
import hashlib
import json
import pathlib
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = "1.10.2"
PYTHON = "3.13.12"


def artifact(path, url, kind):
    data = path.read_bytes()
    return dict(url=url, sha256=hashlib.sha256(data).hexdigest(), size=len(data), kind=kind)


items = [artifact(ROOT / "artifacts/tg-downloads/python.zip",
                  f"https://www.python.org/ftp/python/{PYTHON}/python-{PYTHON}-embed-amd64.zip", "runtime"),
         artifact(ROOT / "artifacts/tg-source/source.zip",
                  f"https://github.com/Flowseal/tg-ws-proxy/archive/refs/tags/v{VERSION}.zip", "source")]
for wheel in sorted((ROOT / "artifacts/tg-downloads").glob("*.whl")):
    name, version = wheel.name.split("-")[:2]
    with urllib.request.urlopen(f"https://pypi.org/pypi/{name}/{version}/json") as response:
        metadata = json.load(response)
    release = next(item for item in metadata["urls"] if item["filename"] == wheel.name)
    item = artifact(wheel, release["url"], "wheel")
    assert item["sha256"] == release["digests"]["sha256"]
    items.append(item)
target = ROOT / "src-tauri/telegram-module.json"
target.write_text(json.dumps(dict(version=VERSION, python=PYTHON, architecture="x86_64",
                                 artifacts=items), indent=2) + "\n", encoding="utf-8")
print(f"Locked {len(items)} artifacts, {sum(x['size'] for x in items):,} download bytes")
