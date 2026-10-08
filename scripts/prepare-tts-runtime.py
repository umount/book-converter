"""Build-time preparation, adapted from Manga Converter's isolated runtime pack.

Python 3.11/3.12 is needed only on the build machine. Model weights are downloaded
separately by the desktop app. Use --cuda for NVIDIA wheels, --bundle for installers.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import venv

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cuda", action="store_true", default=os.environ.get("BOOK_TTS_CUDA") == "1")
    parser.add_argument("--bundle", action="store_true")
    args = parser.parse_args()
    if sys.version_info[:2] not in [(3, 11), (3, 12)]:
        parser.error("Use Python 3.11 or 3.12 to prepare the TTS runtime")
    cache = ROOT / ".cache" / "tts-venv"
    python = cache / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    requirements = ROOT / "scripts/tts/requirements.txt"
    mode = "cu128" if args.cuda else "cpu"
    ready = cache / "ready.json"
    if not python.exists() or not (cache / "pyvenv.cfg").exists():
        venv.create(cache, with_pip=True)
    # The launcher may change while an existing, supported virtualenv is reused.
    # Record the runtime's actual Python version, including its license directory.
    runtime_python = json.loads(subprocess.check_output([str(python), "-I", "-c",
        "import json, sys; print(json.dumps(list(sys.version_info[:2])))"], text=True))
    if runtime_python not in [[3, 11], [3, 12]]:
        parser.error("Remove .cache/tts-venv and prepare it with Python 3.11 or 3.12")
    stamp = {"version": 1, "torch": "2.8.0", "mode": mode, "requirements": hashlib.sha256(requirements.read_bytes()).hexdigest(), "python": runtime_python}
    try:
        installed = json.loads(ready.read_text())
    except (OSError, ValueError):
        installed = None
    if installed != json.loads(json.dumps(stamp)):
        ready.unlink(missing_ok=True)
        version = "2.8.0" if sys.platform == "darwin" else f"2.8.0+{mode}"
        index = [] if sys.platform == "darwin" else ["--index-url", f"https://download.pytorch.org/whl/{mode}"]
        subprocess.run([str(python), "-m", "pip", "install", f"torch=={version}", f"torchaudio=={version}", *index], check=True)
        subprocess.run([str(python), "-m", "pip", "install", "-r", str(requirements)], check=True)
        subprocess.run([str(python), "-I", str(ROOT / "scripts/tts/worker.py"), "--check"], check=True)
        ready.write_text(json.dumps(stamp))
    if not args.bundle:
        print(f"TTS development runtime ready ({mode}). Download weights in Narration.")
        return
    source_hash = hashlib.sha256(Path(__file__).read_bytes() + (ROOT / "scripts/tts/worker.py").read_bytes() + json.dumps(stamp).encode()).hexdigest()
    destination = ROOT / "src-tauri/tts-runtime"
    try:
        if json.loads((destination / "manifest.json").read_text())["sourceHash"] == source_hash:
            executable = destination / ("book-tts.exe" if os.name == "nt" else "book-tts")
            subprocess.run([str(executable), "--check"], check=True)
            print("TTS runtime pack is current.")
            return
    except (OSError, ValueError, KeyError):
        pass
    work = ROOT / ".cache/tts-build"
    subprocess.run([str(python), "-m", "PyInstaller", "--noconfirm", "--clean", "--onedir", "--name", "book-tts",
                    "--distpath", str(work / "dist"), "--workpath", str(work / "work"), "--specpath", str(work),
                    "--collect-all", "qwen_tts", "--collect-all", "transformers", "--collect-all", "torchaudio",
                    "--collect-all", "librosa",
                    "--collect-submodules", "scipy._external.array_api_compat",
                    "--copy-metadata", "qwen-tts", "--copy-metadata", "accelerate",
                    str(ROOT / "scripts/tts/worker.py")], check=True)
    built = work / "dist/book-tts"
    executable = built / ("book-tts.exe" if os.name == "nt" else "book-tts")
    subprocess.run([str(executable), "--check"], check=True)
    machine = {"AMD64": "x86_64", "arm64": "aarch64"}.get(platform.machine(), platform.machine())
    manifest = {"version": 1, "sourceHash": source_hash, "platform": {"Darwin": "macos", "Windows": "windows", "Linux": "linux"}[platform.system()], "arch": machine,
                "sha256": hashlib.sha256(executable.read_bytes()).hexdigest(), "mode": mode}
    (built / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    previous = destination.with_suffix(".previous")
    next_pack = destination.with_suffix(".next")
    shutil.rmtree(next_pack, ignore_errors=True)
    shutil.copytree(built, next_pack)
    shutil.copytree(ROOT / "third-party", next_pack / "notices")
    # Preserve installed package metadata and license texts beside the runtime.
    packages = cache / ("Lib/site-packages" if os.name == "nt" else f"lib/python{runtime_python[0]}.{runtime_python[1]}/site-packages")
    for metadata in packages.glob("*.dist-info"):
        shutil.copytree(metadata, next_pack / "package-notices" / metadata.name)
    (next_pack / ".gitkeep").touch()
    shutil.rmtree(previous, ignore_errors=True)
    if destination.exists():
        destination.rename(previous)
    try:
        next_pack.rename(destination)
    except Exception:
        if previous.exists():
            previous.rename(destination)
        raise
    shutil.rmtree(previous, ignore_errors=True)
    print(f"TTS runtime bundled ({mode}); end users do not need Python.")


if __name__ == "__main__":
    main()
