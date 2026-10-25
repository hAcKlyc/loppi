# Bundled offline components

hear includes X-ASR Zipformer transducer (Apache-2.0), Silero VAD (MIT), and CampPlus / 3D-Speaker (Apache-2.0). Their exact assets, sources and license files are listed in `config/models.lock.json`; model licenses are included inside the Core model resource directory.

The native speech runtime is sherpa-onnx 2.0.0 (Apache-2.0) with ONNX Runtime 1.28.2 (MIT). It includes kaldi-decoder, kaldi-native-fbank, kaldifst/OpenFst, KissFFT, PortAudio, SentencePiece and utf8proc. Upstream license texts and ONNX Runtime third-party notices are preserved in `licenses/`; `licenses/sources.json` records the retrieved immutable source URLs and hashes. These notices cover the native offline components and do not grant a license to hear itself.

Weights remain independent of executable architecture and are bundled once. Neither these notices nor the build cache contain user recordings.
