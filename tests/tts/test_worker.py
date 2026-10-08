import importlib.util
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch
import numpy as np
import soundfile as sf

spec = importlib.util.spec_from_file_location("worker", Path(__file__).resolve().parents[2] / "scripts/tts/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class NarrationTests(unittest.TestCase):
    def request(self):
        return {"version": 1, "language": "Russian", "voice": "Ryan", "device": "cpu", "chapters": [{"title": "Глава", "chunks": ["Первый фрагмент.", "Второй фрагмент."]}]}

    def samples(self, _):
        return np.sin(np.arange(12000) * 2 * np.pi * 440 / 24000) * 0.15, 24000

    def test_mp3_is_decodable_with_full_duration_and_checkpointed(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit") as emit:
            root = Path(tmp)
            worker.narrate(root, self.request(), self.samples, "pinned-model")
            path = root / "audio/00001.mp3"
            decoded, rate = sf.read(path)
            self.assertEqual(rate, 24000)
            self.assertGreater(len(decoded) / rate, 1.2)
            self.assertLess(len(decoded) / rate, 1.5)
            self.assertGreater(np.max(np.abs(decoded)), 0.05)
            self.assertFalse(list((root / "chunks").iterdir()))
            with patch.object(self, "samples", side_effect=AssertionError("must reuse MP3")):
                worker.narrate(root, self.request(), self.samples, "pinned-model")
            self.assertEqual(emit.call_args.args, ("done",))

    def test_resume_reuses_only_completed_fragments(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit"):
            root = Path(tmp)
            calls = []
            def interrupted(text):
                calls.append(text)
                if len(calls) == 2:
                    raise RuntimeError("interrupted")
                return self.samples(text)
            with self.assertRaises(RuntimeError):
                worker.narrate(root, self.request(), interrupted, "model")
            self.assertFalse((root / "audio/00001.mp3").exists())
            calls.clear()
            def resumed(text):
                calls.append(text)
                return self.samples(text)
            worker.narrate(root, self.request(), resumed, "model")
            self.assertEqual(calls, ["Второй фрагмент."])

    def test_corrupt_audio_and_changed_voice_are_not_reused(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit"):
            root = Path(tmp)
            request = self.request()
            worker.narrate(root, request, self.samples, "model")
            (root / "audio/00001.mp3").write_bytes(b"corrupt")
            with patch.object(self, "samples", wraps=self.samples) as generate:
                worker.narrate(root, request, generate, "model")
                self.assertEqual(generate.call_count, 2)
            request["voice"] = "Serena"
            with patch.object(self, "samples", wraps=self.samples) as generate:
                worker.narrate(root, request, generate, "model")
                self.assertEqual(generate.call_count, 2)

    def test_invalid_or_truncated_audio_never_becomes_finished_mp3(self):
        for samples in [np.array([]), np.array([np.nan]), np.zeros(24000 * 160)]:
            with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit"):
                root = Path(tmp)
                with self.assertRaises(ValueError):
                    worker.narrate(root, self.request(), lambda _: (samples, 24000), "model")
                self.assertFalse((root / "audio/00001.mp3").exists())

    def test_resource_failures_report_actionable_errors_without_private_details(self):
        for error, reason in [(OSError(28, "private path: no space"), "audioStorage"),
                              (MemoryError(), "audioMemory"),
                              (RuntimeError("CUDA out of memory"), "audioMemory"),
                              (RuntimeError("private book text"), "audioWorker")]:
            with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit") as emit, \
                    patch.object(worker, "run", side_effect=error), \
                    patch.object(sys, "argv", ["book-tts", "--run", tmp]):
                self.assertEqual(worker.main(), 1)
                emit.assert_called_once_with("error", reason=reason)


if __name__ == "__main__":
    unittest.main()
