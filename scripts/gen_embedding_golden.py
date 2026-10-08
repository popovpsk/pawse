import math
import os
import sys

import essentia.standard as es
import numpy as np
import onnxruntime as ort

OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "audio_embedding", "tests", "data")
CHORD = [1.0, 1.25, 1.5, 1.335]
PATCH, PATCH_HOP, PATCHES, BATCH = 128, 62, 32, 64


def signal(rate, length):
    state = 0x12345678
    out = np.empty(length, dtype=np.float32)
    for i in range(length):
        t = i / rate
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        noise = (state >> 8) / 16777216.0 * 2.0 - 1.0
        k = CHORD[math.floor(t / 2.0) % 4]
        env = 0.6 + 0.4 * math.sin(2.0 * math.pi * 0.5 * t)
        tone = (
            0.3 * math.sin(2.0 * math.pi * 220.0 * k * t)
            + 0.2 * math.sin(2.0 * math.pi * 554.37 * k * t)
            + 0.1 * math.sin(2.0 * math.pi * 1760.0 * k * t + 0.3)
        )
        out[i] = env * tone + 0.05 * noise
    return out


def mel(audio):
    frontend = es.TensorflowInputMusiCNN()
    frames = es.FrameGenerator(audio, frameSize=512, hopSize=256, startFromZero=False)
    return np.array([frontend(f) for f in frames], dtype=np.float32)


def frame_count(samples):
    return 1 if samples <= 256 else 1 + -(-(samples - 256) // 256)


def spread32(frames):
    starts = [0] if frames <= PATCH else [p * PATCH_HOP for p in range((frames - PATCH) // PATCH_HOP + 1)]
    total = len(starts)
    chosen = range(total) if total <= PATCHES else [(i * total + total // 2) // PATCHES for i in range(PATCHES)]
    return [starts[i] for i in chosen]


def embedding(model, audio):
    m = mel(audio)[: frame_count(len(audio))]
    starts = spread32(len(m))
    batch = np.zeros((BATCH, PATCH, 96), dtype=np.float32)
    for p, s in enumerate(starts):
        rows = m[s : s + PATCH]
        batch[p, : len(rows)] = rows
    session = ort.InferenceSession(model, providers=["CPUExecutionProvider"])
    out = session.run(None, {session.get_inputs()[0].name: batch})[0]
    return out[: len(starts)].astype(np.float64).mean(0).astype(np.float32)


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: gen_embedding_golden.py <discogs_multi_embeddings-effnet-bs64-1.onnx>")
    model = sys.argv[1]
    os.makedirs(OUT, exist_ok=True)
    mel(signal(16000, 3 * 16000)).tofile(os.path.join(OUT, "mel_16k.f32"))
    resample = es.Resample(inputSampleRate=44100, outputSampleRate=16000, quality=4)
    resample(signal(44100, 44100)).astype(np.float32).tofile(os.path.join(OUT, "resample_44k.f32"))
    embedding(model, signal(16000, 75 * 16000)).tofile(os.path.join(OUT, "embedding.f32"))


if __name__ == "__main__":
    main()
