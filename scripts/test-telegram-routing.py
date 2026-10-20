"""Offline routing regression tests, run by test-telegram-module.py in its isolated runtime."""
import tempfile
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, patch

import runner
from proxy import bridge
from proxy.config import proxy_config
from proxy.pool import ws_pool


def settings(**overrides):
    return dict(port=1443, secret="0" * 32, **overrides)


class RoutingTests(unittest.IsolatedAsyncioTestCase):
    def test_default_matches_upstream_and_explicit_disable_is_preserved(self):
        runner.configure_proxy(settings())
        self.assertTrue(proxy_config.fallback_cfproxy)
        self.assertTrue(proxy_config.h2_enabled)
        runner.configure_proxy(settings(cfproxy=False))
        self.assertFalse(proxy_config.fallback_cfproxy)
        self.assertFalse(proxy_config.h2_enabled)

    async def test_media_uses_h2_when_direct_ws_is_unavailable(self):
        runner.configure_proxy(settings(cfproxy=True))
        channel = object()
        pool = SimpleNamespace(open=AsyncMock(return_value=channel))
        with patch.object(bridge, "bridge_h2", new_callable=AsyncMock) as h2, \
                patch.object(bridge, "_cfproxy_fallback", new_callable=AsyncMock) as cf, \
                patch.object(bridge, "_tcp_fallback", new_callable=AsyncMock) as tcp:
            result = await bridge.do_fallback(None, None, b"", "offline-test",
                1, False, True, " media", None, h2_pool=pool, proto_tag=b"\xdd" * 4)
            self.assertTrue(result)
            pool.open.assert_awaited_once_with(1, "offline-test")
            h2.assert_awaited_once()
            cf.assert_not_awaited()
            tcp.assert_not_awaited()

    async def test_disabled_cf_leaves_media_on_direct_tcp(self):
        runner.configure_proxy(settings(cfproxy=False))
        with patch.object(bridge, "_cfproxy_fallback", new_callable=AsyncMock) as cf, \
                patch.object(bridge, "_tcp_fallback", new_callable=AsyncMock, return_value=False) as tcp:
            result = await bridge.do_fallback(None, None, b"", "offline-test",
                1, False, True, " media", None)
            self.assertFalse(result)
            cf.assert_not_awaited()
            tcp.assert_awaited_once()

    async def test_empty_dc_overrides_disable_direct_websocket_routes(self):
        runner.configure_proxy(settings(dc_ips={}, cfproxy=False))
        self.assertIsNone(await ws_pool.get(2, False))
        self.assertIsNone(await ws_pool.get(4, True))

    async def test_worker_remains_available_when_cf_is_disabled(self):
        runner.configure_proxy(settings(cfproxy=False, worker=True,
            worker_domains=["relay.example.workers.dev"]))
        with patch.object(bridge, "_cfproxy_worker_fallback", new_callable=AsyncMock, return_value=True) as worker, \
                patch.object(bridge, "_tcp_fallback", new_callable=AsyncMock) as tcp:
            self.assertTrue(await bridge.do_fallback(None, None, b"", "offline-test",
                1, False, True, " media", None))
            worker.assert_awaited_once()
            tcp.assert_not_awaited()

    def test_new_config_enables_the_upstream_default(self):
        with tempfile.TemporaryDirectory(prefix="zapret-tg-config-") as directory:
            with patch.object(runner, "CONFIG", Path(directory) / "config.json"):
                runner.initialize()
                import json
                config = json.loads(runner.CONFIG.read_text())
                self.assertTrue(config["cfproxy"])
                self.assertEqual(len(config["secret"]), 32)


if __name__ == "__main__":
    unittest.main()
