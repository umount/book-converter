# Speech runtime

The application and the speech worker source remain under the repository's MIT
license. The prepared speech runtime contains separately licensed dependencies.
Its `package-notices` directory preserves installed package metadata and license
texts; consult those files for the exact contents of a generated pack.

- Qwen3-TTS code and Qwen3-TTS-12Hz-0.6B-CustomVoice weights: Apache-2.0.
  Code: https://github.com/QwenLM/Qwen3-TTS
  Weights: https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice
- PyTorch: BSD-3-Clause. https://github.com/pytorch/pytorch
- Transformers: Apache-2.0. https://github.com/huggingface/transformers
- lameenc 1.8.1: LGPL-3.0. https://github.com/chrisstaite/lameenc
  Source distributions: https://pypi.org/project/lameenc/1.8.1/#files
- LAME: LGPL-2.0-or-later. https://lame.sourceforge.io/
- PyInstaller: GPL with its distribution exception for bundled applications.
  https://pyinstaller.org/en/stable/license.html

The MP3 encoder is a separate native Python extension in the runtime directory.
Model weights are downloaded on demand, verified against the pinned manifest,
and are not included in the application installer. Generated audio is MP3,
128 kbps, mono; intermediate PCM files are deleted after a chapter is completed.
