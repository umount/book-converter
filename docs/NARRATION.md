# Local book narration

Open a book and select **Narration**. Download the Qwen3-TTS 0.6B CustomVoice model
once (approximately 2.5 GB), then select the original or translation, a chapter,
chapter range or the whole book, a voice and a device. Select **Create MP3**.
The model download can be paused and resumed. Every file is checked against a
pinned size and SHA-256 hash before use.

The packaged application includes its speech engine. End users do not need Python,
FFmpeg, an API key or a paid speech service. Internet access is needed to download
the model; synthesis runs locally with offline mode enabled. Book text is not sent
to a speech provider. Translation remains a separate feature with its configured
provider.

## Languages, voices and output

The engine supports Russian, English, Chinese, Japanese, Korean, German, French,
Spanish, Portuguese and Italian. The project language determines the narration
language. An unsupported or unspecified source language must be corrected in
project settings before narrating the original.

Nine preset voices are available: Ryan, Aiden, Serena, Vivian, Uncle_Fu, Dylan, Eric,
Ono_Anna and Sohee. Pronunciation and accent vary by voice and language. Start with
one chapter to check the result. Voice cloning and voice design are not included.

Each nonempty chapter produces one mono MP3 at **128 kbps**, with a sample rate of
24 kHz. The chapter title and text/caption blocks are spoken in source order;
illustrations are skipped. Text is split at sentence or whitespace boundaries into
fragments of at most 400 Unicode characters. There is a short pause between fragments.
A continuous MP3 encoder joins their audio within each chapter.

For a translation, all nonempty text blocks and the title in each selected chapter
must have a translation. Missing text is reported instead of mixing languages or
silently omitting paragraphs. Source editing and translation quality should be
reviewed before narration.

After completion, **Save chapters as MP3** writes a new `audiobook-<job-id>` folder
to the chosen directory, containing `00001.mp3`, `00002.mp3`, etc. and a `book.m3u8`
playlist with chapter names. Existing folders are never overwritten. Incomplete
jobs cannot be exported.

## Devices and recovery

The default runtime uses CPU. CPU generation can take considerably longer than
the resulting audio; begin with one chapter. **Automatic** uses CUDA when the
installed runtime supports it and a compatible NVIDIA GPU is available. Selecting
CUDA explicitly reports an error when it is unavailable. CUDA builds require the
build option described below. GPU memory needs depend on the device and input;
there is no fixed performance or memory guarantee.

Only one narration job runs at a time across projects. **Pause** stops the worker.
Completed fragments and chapters are checked and reused on **Resume saved text**.
An interrupted fragment is regenerated. After an application restart, interrupted
jobs require an explicit resume; opening a book never starts narration.

Each job stores a snapshot of the selected text, voice, language and device.
Resuming uses that snapshot. Create a new job to use subsequent text edits or
different narration settings. Translation can continue independently without
changing an existing audio job's input.

Model files live under the application's `tts-models/` directory. Audio jobs live
under `audiobooks/<project-id>/<job-id>/`, outside the project database. These
caches and audio files are not included in portable `.bcproj` archives. Deleting
a project cancels its worker and removes its audio cache, but leaves explicitly
exported MP3 folders and the shared model download intact.

## Development and packaging

The build machine needs Python **3.11 or 3.12**, including `venv` and `pip`, plus
the normal Rust/Node/Tauri prerequisites. Use `BOOK_TTS_PYTHON` to select a specific
Python executable. The build script creates an isolated `.cache/tts-venv`; it does
not install packages into the system Python.

```bash
npm run tts:prepare  # CPU development runtime; needed once to use Narration in dev
npm run test:tts     # Real MP3 encoding/decoding and fragment recovery tests
npm run tts:bundle  # Freeze and validate the engine for this OS/architecture
```

The normal Tauri build hook runs `tts:bundle` automatically and includes
`src-tauri/tts-runtime/` as a resource. Model weights are downloaded separately by
the application. Allow several GB of free space for dependencies and build copies
in addition to the model and generated audio. The first build downloads large
dependencies; subsequent builds reuse the matching runtime pack.

For an NVIDIA runtime, preserve the CUDA option through the Tauri build hook:

```bash
BOOK_TTS_CUDA=1 npm run tauri -- build
```

```powershell
$env:BOOK_TTS_CUDA = "1"
npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis
```

The CUDA runtime uses PyTorch's CUDA 12.8 wheels. Build on the target OS and
architecture; cross-compiling Rust alone does not produce a compatible Python
runtime. Runtime preparation and the frozen executable both run a dependency and
MP3 encoder self-check. The frozen self-check does not load model weights or prove
speech quality. A real model smoke test must be run separately.

For isolated manual testing, the `prepare_narration` Rust example downloads and
verifies models into explicitly supplied directories. A worker job directory needs
`input.json`, `model-files.json` and the materialized `model/` files. These are
internal versioned formats, not a public document format.

Upstream references: [Qwen3-TTS](https://github.com/QwenLM/Qwen3-TTS),
[0.6B CustomVoice model](https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice).
Licenses and redistribution notes are in [TTS notices](../third-party/TTS-NOTICES.md).
