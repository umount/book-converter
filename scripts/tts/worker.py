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


def narrate(directory, request, synthesize, model_fingerprint):
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
        identity = json.dumps([PIPELINE, model_fingerprint, request["language"], request["voice"], request["device"], chapter], ensure_ascii=False, sort_keys=True)
        fingerprint = hashlib.sha256(identity.encode("utf-8")).hexdigest()
        target = audio / f"{chapter_index + 1:05}.mp3"
        receipt = target.with_suffix(".json")
        if cached(target, receipt, fingerprint):
            completed += len(chapter["chunks"])
            emit("chunk", completed=completed, chapter=chapter["title"][:300])
            emit("chapter", completed=chapter_index + 1)
            continue
        parts = []
        for chunk_index, text in enumerate(chapter["chunks"]):
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
        encode_chapter(parts, target)
        atomic_json(receipt, {"fingerprint": fingerprint, "sha256": digest(target)})
        emit("chapter", completed=chapter_index + 1)
        # A verified chapter MP3 is the checkpoint from this point onwards.
        for path, _ in parts:
            path.unlink(missing_ok=True)
            path.with_suffix(".json").unlink(missing_ok=True)
    emit("done")


def run(directory):
    for name in ["HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE", "HF_HUB_DISABLE_TELEMETRY"]:
        os.environ[name] = "1"
    request = json.loads((directory / "input.json").read_text(encoding="utf-8"))
    specs = json.loads((directory / "model-files.json").read_text(encoding="utf-8"))
    for spec in specs:
        path = directory / "model" / spec["filename"]
        if path.is_symlink() or not path.is_file() or path.stat().st_size != spec["bytes"] or digest(path) != spec["sha256"]:
            raise ValueError("audioModels")
    model_fingerprint = hashlib.sha256(json.dumps(specs, sort_keys=True).encode()).hexdigest()
    model = None

    def synthesize(text):
        nonlocal model
        import torch
        from qwen_tts import Qwen3TTSModel
        if model is None:
            device = request["device"]
            if device == "auto":
                device = "cuda" if torch.cuda.is_available() else "cpu"
            if device == "cuda" and not torch.cuda.is_available():
                raise ValueError("audioCuda")
            torch.set_num_threads(max(1, min(8, os.cpu_count() or 1)))
            dtype = torch.float32 if device == "cpu" else (torch.bfloat16 if torch.cuda.is_bf16_supported() else torch.float16)
            model = Qwen3TTSModel.from_pretrained(str(directory / "model"), device_map=device, dtype=dtype, attn_implementation="sdpa", local_files_only=True)
        with torch.inference_mode():
            waves, rate = model.generate_custom_voice(text=text, language=request["language"], speaker=request["voice"], max_new_tokens=2048)
        return waves[0], rate

    narrate(directory, request, synthesize, model_fingerprint)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--parent-pipe", action="store_true")
    args = parser.parse_args()
    if args.parent_pipe:
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
        elif args.run:
            try:
                run(args.run.resolve())
            except Exception as error:
                # Keep source text, filesystem details and dependency traces out of IPC.
                reason = str(error)
                if isinstance(error, MemoryError) or "out of memory" in reason.lower():
                    reason = "audioMemory"
                elif isinstance(error, OSError):
                    reason = "audioStorage"
                if reason not in {"audioCuda", "audioModels", "audioTooLong", "audioOutput", "audioMemory", "audioStorage", "audioVersion"}:
                    reason = "audioWorker"
                emit("error", reason=reason)
                return 1
        else:
            parser.error("choose --run or --check")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
