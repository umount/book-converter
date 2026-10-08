"""Isolated offline Qwen3-TTS worker; stdout is a versioned JSON event stream.

PCM checkpoints are private intermediates. One continuous LAME encoder per chapter
produces the final MP3; independently encoded MP3 streams are never concatenated.
"""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import sys
import threading

PIPELINE = "qwen3-06-mp3-v1"


def emit(event, **values):
    print(json.dumps({"event": event, **values}, ensure_ascii=True), flush=True, file=sys.__stdout__)


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def atomic_json(path, value):
    temporary = path.with_suffix(path.suffix + ".part")
    with open(temporary, "w", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


def cached(path, receipt, fingerprint):
    try:
        saved = json.loads(receipt.read_text(encoding="utf-8"))
        if (not path.is_symlink() and path.is_file() and path.stat().st_size > 0
                and saved["fingerprint"] == fingerprint and saved["sha256"] == digest(path)):
            return saved
    except (OSError, ValueError, KeyError):
        pass
    return None


def encode_chapter(parts, destination):
    import lameenc
    encoder = lameenc.Encoder()
    encoder.set_bit_rate(128)
    encoder.set_channels(1)
    encoder.set_in_sample_rate(parts[0][1])
    encoder.set_quality(2)
    encoder.silence()
    temporary = destination.with_suffix(".mp3.part")
    with open(temporary, "wb") as output:
        for index, (path, rate) in enumerate(parts):
            if rate != parts[0][1]:
                raise ValueError("audioOutput")
            with open(path, "rb") as pcm:
                while block := pcm.read(65536):
                    output.write(encoder.encode(block))
            if index + 1 < len(parts):
                output.write(encoder.encode(b"\0\0" * (rate // 4)))
        output.write(encoder.flush())
        output.flush()
        os.fsync(output.fileno())
    if temporary.stat().st_size == 0:
        raise ValueError("audioOutput")
    os.replace(temporary, destination)


def chapter_fingerprint(request, chapter, model_fingerprint):
    identity = json.dumps([PIPELINE, model_fingerprint, request["language"], request["voice"], request["device"], chapter], ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(identity.encode("utf-8")).hexdigest()


def narrate(directory, request, synthesize, model_fingerprint, cancelled=lambda: False):
    """synthesize is lazy: finished checkpoints do not need a loaded model."""
    import numpy as np
    if request["version"] != 1:
        raise ValueError("audioVersion")
    audio = directory / "audio"
    checkpoints = directory / "chunks"
    audio.mkdir(exist_ok=True)
    checkpoints.mkdir(exist_ok=True)
    if audio.is_symlink() or checkpoints.is_symlink():
        raise ValueError("audioStorage")
    completed = 0
    for chapter_index, chapter in enumerate(request["chapters"]):
        if cancelled():
            emit("paused")
            return
        fingerprint = chapter_fingerprint(request, chapter, model_fingerprint)
        target = audio / f"{chapter_index + 1:05}.mp3"
        receipt = target.with_suffix(".json")
        if cached(target, receipt, fingerprint):
            completed += len(chapter["chunks"])
            emit("chunk", completed=completed, chapter=chapter["title"][:300])
            emit("chapter", completed=chapter_index + 1)
            continue
        parts = []
        for chunk_index, text in enumerate(chapter["chunks"]):
            if cancelled():
                emit("paused")
                return
            path = checkpoints / f"{chapter_index:05}-{chunk_index:05}.pcm"
            metadata = path.with_suffix(".json")
            chunk_fingerprint = hashlib.sha256(f"{fingerprint}:{chunk_index}".encode()).hexdigest()
            saved = cached(path, metadata, chunk_fingerprint)
            if saved:
                rate = saved["rate"]
            else:
                samples, rate = synthesize(text)
                samples = np.asarray(samples, dtype=np.float32).reshape(-1)
                if rate != 24000 or samples.size == 0 or not np.isfinite(samples).all():
                    raise ValueError("audioOutput")
                # 2048 acoustic tokens at 12 Hz. Reaching the cap can truncate text.
                if samples.size / rate >= 160:
                    raise ValueError("audioTooLong")
                pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes()
                temporary = path.with_suffix(".pcm.part")
                with open(temporary, "wb") as stream:
                    stream.write(pcm)
                    stream.flush()
                    os.fsync(stream.fileno())
                os.replace(temporary, path)
                atomic_json(metadata, {"fingerprint": chunk_fingerprint, "rate": rate, "sha256": digest(path)})
            parts.append((path, rate))
            completed += 1
            emit("chunk", completed=completed, chapter=chapter["title"][:300])
        if not parts:
            raise ValueError("audioOutput")
        if cancelled():
            emit("paused")
            return
        encode_chapter(parts, target)
        atomic_json(receipt, {"fingerprint": fingerprint, "sha256": digest(target)})
        emit("chapter", completed=chapter_index + 1)
        # A verified chapter MP3 is the checkpoint from this point onwards.
        for path, _ in parts:
            path.unlink(missing_ok=True)
            path.with_suffix(".json").unlink(missing_ok=True)
    emit("done")


def model_specs(directory):
    for name in ["HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_HUB_DISABLE_TELEMETRY"]:
        os.environ[name] = "1"
    specs = json.loads((directory / "model-files.json").read_text(encoding="utf-8"))
    for spec in specs:
        path = directory / "model" / spec["filename"]
        if path.is_symlink() or not path.is_file() or path.stat().st_size != spec["bytes"] or digest(path) != spec["sha256"]:
            raise ValueError("audioModels")
    return hashlib.sha256(json.dumps(specs, sort_keys=True).encode()).hexdigest()


class SpeechModel:
    def __init__(self, directory, device):
        self.fingerprint = model_specs(directory)
        import torch
        from qwen_tts import Qwen3TTSModel
        if device == "auto":
            device = "cuda" if torch.cuda.is_available() else "cpu"
        if device == "cuda" and not torch.cuda.is_available():
            raise ValueError("audioCuda")
        self.device = device
        torch.set_num_threads(max(1, min(8, os.cpu_count() or 1)))
        dtype = torch.float32 if device == "cpu" else (torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16)
        self.model = Qwen3TTSModel.from_pretrained(str(directory / "model"), device_map=device, dtype=dtype, attn_implementation="sdpa", local_files_only=True)

    def synthesize(self, text, request):
        import torch
        with torch.inference_mode():
            waves, rate = self.model.generate_custom_voice(text=text, language=request["language"], speaker=request["voice"], max_new_tokens=2048)
        return waves[0], rate


def run(directory):
    request = json.loads((directory / "input.json").read_text(encoding="utf-8"))
    fingerprint = model_specs(directory)
    model = None

    def synthesize(text):
        nonlocal model
        if model is None:
            model = SpeechModel(directory, request["device"])
        return model.synthesize(text, request)

    narrate(directory, request, synthesize, fingerprint)


class CommandPipe:
    """Read control messages during synthesis without holding Python's stdin lock."""
    def __init__(self):
        self.requests = queue.Queue(maxsize=2)
        self.active = {}
        self.lock = threading.Lock()

    def receive(self, command):
        kind = command["command"]
        if kind == "shutdown":
            os._exit(0)
        if kind == "run":
            cancelled = threading.Event()
            with self.lock:
                self.active[command["id"]] = cancelled
            self.requests.put_nowait((command, cancelled))
        elif kind == "pause":
            with self.lock:
                cancelled = self.active.get(command["id"])
                if cancelled:
                    cancelled.set()
        else:
            raise ValueError("audioWorker")

    def read(self):
        pending = b""
        try:
            while True:
                block = os.read(sys.stdin.fileno(), 4096)
                if not block:
                    os._exit(2)
                pending += block
                if len(pending) > 65536:
                    os._exit(2)
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    self.receive(json.loads(line))
        except Exception:
            os._exit(2)


def serve(directory, device, pipe=None, model_factory=SpeechModel):
    if pipe is None:
        pipe = CommandPipe()
        threading.Thread(target=pipe.read, daemon=True).start()
    model = model_factory(directory, device)
    emit("loaded", device=model.device)
    while True:
        command, cancelled = pipe.requests.get()
        if command is None:
            return
        try:
            job_directory = Path(command["directory"])
            request = json.loads((job_directory / "input.json").read_text(encoding="utf-8"))
            if request["device"] not in ("auto", model.device):
                raise ValueError("audioCuda")
            narrate(job_directory, request, lambda text: model.synthesize(text, request), model.fingerprint, cancelled.is_set)
        except Exception as error:
            report_error(error)
        finally:
            with pipe.lock:
                if pipe.active.get(command["id"]) is cancelled:
                    pipe.active.pop(command["id"], None)


def report_error(error):
    # Keep source text, filesystem details and dependency traces out of IPC.
    reason = str(error)
    if isinstance(error, MemoryError) or "out of memory" in reason.lower():
        reason = "audioMemory"
    elif isinstance(error, OSError):
        reason = "audioStorage"
    if reason not in {"audioCuda", "audioModels", "audioTooLong", "audioOutput", "audioMemory", "audioStorage", "audioVersion", "audioPreviewUnavailable"}:
        reason = "audioWorker"
    emit("error", reason=reason)


def preview(directory, destination):
    """Encode a bounded sample from saved audio without importing or loading Qwen."""
    import numpy as np
    import soundfile as sf
    request = json.loads((directory / "input.json").read_text(encoding="utf-8"))
    if request["version"] != 1:
        raise ValueError("audioVersion")
    specs = json.loads((directory / "model-files.json").read_text(encoding="utf-8"))
    fingerprint = hashlib.sha256(json.dumps(specs, sort_keys=True).encode()).hexdigest()
    limit = 24000 * 30
    # A chapter can finish and replace its PCM checkpoints while preview is read.
    for _ in range(2):
        candidates = []
        for folder in ("chunks", "audio"):
            parent = directory / folder
            if parent.is_symlink():
                raise ValueError("audioStorage")
            for path in parent.glob("*"):
                match = re.fullmatch(r"(\d{5,})-(\d{5,})\.pcm", path.name) if folder == "chunks" else re.fullmatch(r"(\d{5,})\.mp3", path.name)
                if match:
                    chapter = int(match[1]) - (folder == "audio")
                    chunk = int(match[2]) if folder == "chunks" else 2**63
                    if 0 <= chapter < len(request["chapters"]):
                        candidates.append((chapter, chunk, path))
        for chapter_index, chunk_index, path in sorted(candidates, reverse=True):
            chapter = request["chapters"][chapter_index]
            expected = chapter_fingerprint(request, chapter, fingerprint)
            if path.suffix == ".pcm":
                if chunk_index >= len(chapter["chunks"]):
                    continue
                expected = hashlib.sha256(f"{expected}:{chunk_index}".encode()).hexdigest()
            receipt = path.with_suffix(".json")
            if receipt.is_symlink():
                raise ValueError("audioStorage")
            saved = cached(path, receipt, expected)
            if not saved:
                continue
            try:
                if path.suffix == ".pcm":
                    if saved.get("rate") != 24000 or path.stat().st_size > 24000 * 160 * 2:
                        continue
                    with open(path, "rb") as source:
                        pcm = source.read(limit * 2)
                else:
                    with sf.SoundFile(path) as source:
                        if source.samplerate != 24000 or source.channels != 1:
                            continue
                        source.seek(max(0, len(source) - limit))
                        samples = source.read(limit, dtype="float32")
                    pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes()
            except FileNotFoundError:
                continue
            if not pcm or len(pcm) % 2:
                continue
            intermediate = destination / "sample.pcm"
            try:
                intermediate.write_bytes(pcm)
                output = destination / "audio.mp3"
                encode_chapter([(intermediate, 24000)], output)
                atomic_json(destination / "preview.json", {"title": chapter["title"], "durationSeconds": len(pcm) / 48000, "sha256": digest(output)})
            finally:
                intermediate.unlink(missing_ok=True)
            emit("preview")
            return
    raise ValueError("audioPreviewUnavailable")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", type=Path)
    parser.add_argument("--serve", type=Path)
    parser.add_argument("--preview", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--parent-pipe", action="store_true")
    args = parser.parse_args()
    if args.parent_pipe and not args.serve:
        # Parent owns the write end. Exit even if the desktop process crashes or is killed.
        def watch_parent():
            # A daemon blocked on BufferedReader holds its lock during interpreter
            # shutdown, aborting even a successful job. Read the OS pipe directly.
            os.read(sys.stdin.fileno(), 1)
            os._exit(2)
        threading.Thread(target=watch_parent, daemon=True).start()
    with contextlib.redirect_stdout(sys.stderr):
        if args.check:
            import torch
            import lameenc
            from qwen_tts import Qwen3TTSModel  # noqa: F401
            encoder = lameenc.Encoder()
            encoder.set_channels(1)
            encoder.set_in_sample_rate(24000)
            encoder.silence()
            assert encoder.encode(b"\0\0" * 2400) + encoder.flush()
            emit("ready", version=1, cuda=torch.cuda.is_available())
        elif args.run or args.serve or args.preview:
            try:
                if args.preview:
                    if not args.output:
                        parser.error("--preview requires --output")
                    preview(args.preview.resolve(), args.output.resolve())
                elif args.serve:
                    serve(args.serve.resolve(), args.device)
                else:
                    run(args.run.resolve())
            except Exception as error:
                report_error(error)
                return 1
        else:
            parser.error("choose --serve, --run, --preview or --check")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
