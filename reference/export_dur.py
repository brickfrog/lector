#!/usr/bin/env -S uv run --with kokoro --with onnx --with onnxscript --with torch --script
"""Re-export the full Kokoro model with the duration tensor exposed.

onnx-community's stock export drops the duration output; this re-exports
KModelForONNX (which returns both waveform and duration) so the browser can read
per-token durations. Requires disable_complex (the iSTFT uses complex ops the
legacy ONNX exporter can't trace) and the legacy exporter (dynamo=False).

Caveat learned the hard way: this fp32 model is the ONLY format that produces
audio under ort-web (q8 and fp16 come back silent in wasm/webgpu), but the
disable_complex iSTFT falls back to CPU on webgpu, so it's slow (~16s/8s audio).
The fast path is stock fp32 for audio (webgpu-clean) + export_duronly.py for timing.

  uv run reference/export_dur.py   ->   reference/models/kokoro_dur.onnx  (~311MB)
"""

import os

import torch
from kokoro import KModel
from kokoro.model import KModelForONNX


def main():
    kmodel = KModel(disable_complex=True).eval()
    model = KModelForONNX(kmodel).eval()
    ids = torch.LongTensor([[0, *torch.randint(1, 100, (48,)).tolist(), 0]])
    style = torch.randn(1, 256)
    speed = torch.IntTensor([1])
    os.makedirs("reference/models", exist_ok=True)
    out = "reference/models/kokoro_dur.onnx"
    torch.onnx.export(
        model, (ids, style, speed), out, dynamo=False,
        input_names=["input_ids", "style", "speed"],
        output_names=["waveform", "duration"],
        dynamic_axes={"input_ids": {0: "b", 1: "len"}, "style": {0: "b"}, "speed": {0: "b"}},
        opset_version=17, do_constant_folding=True,
    )
    print(f"wrote {out}  ({os.path.getsize(out) / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
