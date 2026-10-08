import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import sys
import subprocess
import queue
import threading
import unittest
from unittest.mock import patch
import numpy as np
import soundfile as sf

spec = importlib.util.spec_from_file_location("worker", Path(__file__).resolve().parents[2] / "scripts/tts/worker.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class NarrationTests(unittest.TestCase):
    def test_worker_exits_cleanly_while_parent_pipe_stays_open(self):
        self.check_parent_pipe(close=False, expected=0)

    def test_worker_stops_when_parent_pipe_closes(self):
        self.check_parent_pipe(close=True, expected=2)

    def check_parent_pipe(self, close, expected):
        script = """
import runpy, sys, time
scope = runpy.run_path(sys.argv[1])
delay = float(sys.argv[2])
scope['main'].__globals__['run'] = lambda _: time.sleep(delay)
sys.argv = ['book-tts', '--parent-pipe', '--run', sys.argv[3]]
raise SystemExit(scope['main']())
"""
        with tempfile.TemporaryDirectory() as tmp, subprocess.Popen(
            [sys.executable, "-I", "-c", script, str(Path(worker.__file__)), "30" if close else "0.1", tmp],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        ) as child:
            try:
                if close:
                    child.stdin.close()
                self.assertEqual(child.wait(timeout=10), expected, child.stderr.read())
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()

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

    def preview_input(self, root):
        request = self.request()
        (root / "input.json").write_text(json.dumps(request))
        (root / "model-files.json").write_text("[]")
        return request, hashlib.sha256(b"[]").hexdigest()

    def test_preview_plays_saved_fragment_before_chapter_finishes_without_model(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit"):
            root = Path(tmp)
            request, fingerprint = self.preview_input(root)
            count = 0
            def generate(text):
                nonlocal count
                count += 1
                return self.samples(text)
            worker.narrate(root, request, generate, fingerprint, cancelled=lambda: count == 1)
            self.assertFalse((root / "audio/00001.mp3").exists())
            output = root / "preview"
            output.mkdir()
            with patch.object(worker, "SpeechModel", side_effect=AssertionError("must not load model")):
                worker.preview(root, output)
            decoded, rate = sf.read(output / "audio.mp3")
            self.assertEqual(rate, 24000)
            self.assertGreater(np.max(np.abs(decoded)), 0.05)
            self.assertAlmostEqual(len(decoded) / rate, 0.5, delta=0.15)
            receipt = json.loads((output / "preview.json").read_text())
            self.assertEqual(receipt["title"], "Глава")
            self.assertEqual(receipt["durationSeconds"], 0.5)
            self.assertEqual(receipt["sha256"], worker.digest(output / "audio.mp3"))
            self.assertEqual(len(list((root / "chunks").glob("*.pcm"))), 1)
            # A preview neither consumes checkpoints nor changes the resume position.
            with patch.object(self, "samples", wraps=self.samples) as resumed:
                worker.narrate(root, request, resumed, fingerprint)
                resumed.assert_called_once_with("Второй фрагмент.")

    def test_preview_uses_finished_chapter_and_limits_duration(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit"):
            root = Path(tmp)
            request, fingerprint = self.preview_input(root)
            worker.narrate(root, request, lambda _: (np.tile(self.samples("")[0], 40), 24000), fingerprint)
            self.assertFalse(list((root / "chunks").iterdir()))
            output = root / "preview"
            output.mkdir()
            worker.preview(root, output)
            audio, rate = sf.read(output / "audio.mp3")
            self.assertAlmostEqual(len(audio) / rate, 30, delta=0.15)
            self.assertLess((output / "audio.mp3").stat().st_size, 500_000)
            # Receipts prevent playing damaged or unrelated audio.
            (root / "audio/00001.mp3").write_bytes(b"corrupt")
            with self.assertRaisesRegex(ValueError, "audioPreviewUnavailable"):
                worker.preview(root, output)

    def test_one_loaded_model_survives_pause_resume_and_new_voice_jobs(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(worker, "emit") as emit:
            root = Path(tmp)
            first, second = root / "first", root / "second"
            first.mkdir()
            second.mkdir()
            request = self.request()
            (first / "input.json").write_text(json.dumps(request))
            (second / "input.json").write_text(json.dumps({**request, "voice": "Aiden", "language": "English"}))
            paused = threading.Event()
            calls = []
            samples = self.samples

            class Model:
                device = "cpu"
                fingerprint = "model"

                def synthesize(self, text, request):
                    calls.append((text, request["voice"], request["language"]))
                    if len(calls) == 1:
                        paused.set()
                    return samples(text)

            pipe = worker.CommandPipe()
            pipe.requests = queue.Queue()
            for path, cancel in [(first, paused), (first, threading.Event()), (second, threading.Event())]:
                pipe.requests.put(({"id": path.name, "directory": str(path)}, cancel))
            pipe.requests.put((None, None))
            with patch.object(worker, "SpeechModel", return_value=Model()) as factory:
                worker.serve(root, "cpu", pipe=pipe, model_factory=factory)
                factory.assert_called_once_with(root, "cpu")
            self.assertEqual(calls, [
                ("Первый фрагмент.", "Ryan", "Russian"),
                ("Второй фрагмент.", "Ryan", "Russian"),
                ("Первый фрагмент.", "Aiden", "English"),
                ("Второй фрагмент.", "Aiden", "English"),
            ])
            events = [call.args[0] for call in emit.call_args_list]
            self.assertEqual(events.count("loaded"), 1)
            self.assertEqual(events.count("paused"), 1)
            self.assertEqual(events.count("done"), 2)
            for directory in (first, second):
                audio, rate = sf.read(directory / "audio/00001.mp3")
                self.assertEqual(rate, 24000)
                self.assertGreater(len(audio), 24000)

    def test_persistent_worker_stops_on_parent_exit_even_during_model_loading(self):
        script = """
import runpy, sys, time
scope = runpy.run_path(sys.argv[1])
class Model:
    def __init__(self, *args):
        time.sleep(30)
scope['serve'](scope['Path'](sys.argv[2]), 'cpu', model_factory=Model)
"""
        with tempfile.TemporaryDirectory() as tmp, subprocess.Popen(
            [sys.executable, "-I", "-c", script, str(Path(worker.__file__)), tmp],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        ) as child:
            try:
                child.stdin.close()
                self.assertEqual(child.wait(timeout=10), 2, child.stderr.read())
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait()


if __name__ == "__main__":
    unittest.main()
