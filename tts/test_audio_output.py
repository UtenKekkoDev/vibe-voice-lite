"""Device routing regressions. No hardware, model or audio packages required."""
import unittest
from unittest.mock import Mock, patch

from audio_output import DefaultOutput


class RoutingTests(unittest.TestCase):
    def setUp(self):
        self.sd = Mock(_initialized=1)
        self.device = 4
        self.cached = -1

        def refresh():
            self.cached = self.device

        self.sd._initialize.side_effect = refresh
        self.sd.query_hostapis.side_effect = lambda: [
            {"name": "Windows WASAPI", "default_output_device": self.cached}
        ]
        self.sd.OutputStream.side_effect = lambda **kwargs: Mock(active=True)
        self.output = DefaultOutput(self.sd, Mock())
        patcher = patch("audio_output.default_endpoint_id", side_effect=lambda: str(self.device))
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_reuses_warm_stream_and_reopens_on_changed_endpoint(self):
        self.output.ensure()
        original = self.output.stream
        self.output.ensure()
        self.assertEqual(self.sd._initialize.call_count, 1)
        self.assertIs(self.output.stream, original)
        self.device = 9
        self.output.ensure()
        original.abort.assert_called_once()
        original.close.assert_called_once()
        self.assertEqual(self.sd.OutputStream.call_args.kwargs["device"], 9)
        self.assertEqual(self.sd._initialize.call_count, 2)

    def test_disconnected_output_then_reconnection_is_retryable(self):
        self.output.ensure()
        self.device = -1
        with self.assertRaises(RuntimeError):
            self.output.ensure()
        self.assertIsNone(self.output.stream)
        self.device = 6
        self.output.ensure()
        self.assertEqual(self.sd.OutputStream.call_args.kwargs["device"], 6)
        self.output.stream.active = False
        self.output.ensure()
        self.assertTrue(self.output.stream.active)


if __name__ == "__main__":
    unittest.main()
