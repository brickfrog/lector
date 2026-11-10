#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = [
#   "kokoro==0.9.4", "torch==2.15.0", "onnx==1.23.2", "onnxscript==0.7.2",
#   "huggingface-hub==2.4.0", "transformers==5.22.2",
# ]
# [tool.uv.sources]
# torch = { index = "pytorch-cpu" }
# [[tool.uv.index]]
# name = "pytorch-cpu"
# url = "https://download.pytorch.org/whl/cpu"
# explicit = true
# ///
"""Export the pinned Kokoro model with waveform and per-token duration outputs.

Run: uv run --locked scripts/export_dur.py --out assets
CPU export; the Python stack is needed only here, not for narration.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile

import torch
from huggingface_hub import hf_hub_download
from kokoro import KModel
from kokoro.model import KModelForONNX

REPOSITORY = "hexgrad/Kokoro-82M"
REVISION = "f3ff3571791e39611d31c381e3a41a3af07b4987"


class DurationOnly(torch.nn.Module):
    def __init__(self, km: KModel):
        super().__init__()
        self.km = km

    @torch.no_grad()
    def forward(self, input_ids, ref_s, speed):
        km = self.km
        il = torch.full((input_ids.shape[0],), input_ids.shape[-1], dtype=torch.long)
        tm = torch.arange(il.max()).unsqueeze(0).expand(il.shape[0], -1).type_as(il)
        tm = torch.gt(tm + 1, il.unsqueeze(1))
        bert_dur = km.bert(input_ids, attention_mask=(~tm).int())
        d_en = km.bert_encoder(bert_dur).transpose(-1, -2)
        s = ref_s[:, 128:]
        d = km.predictor.text_encoder(d_en, s, il, tm)
        x, _ = km.predictor.lstm(d)
        dur = km.predictor.duration_proj(x)
        dur = torch.sigmoid(dur).sum(axis=-1) / speed
        return torch.round(dur).clamp(min=1).long().squeeze()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path(os.environ.get("LECTOR_ASSETS", "assets")))
    parser.add_argument("--duration-only", action="store_true", help="export the experimental timing-only model (not used by Lector)")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    torch.manual_seed(0)
    config = hf_hub_download(REPOSITORY, "config.json", revision=REVISION)
    weights = hf_hub_download(REPOSITORY, "kokoro-v1_0.pth", revision=REVISION)
    kmodel = KModel(repo_id=REPOSITORY, config=config, model=weights, disable_complex=True).eval()
    model = (DurationOnly(kmodel) if args.duration_only else KModelForONNX(kmodel)).eval()
    ids = torch.LongTensor([[0, *torch.randint(1, 100, (48,)).tolist(), 0]])
    style = torch.randn(1, 256)
    speed = torch.IntTensor([1])
    with tempfile.TemporaryDirectory(prefix=".lector-export-", dir=args.out) as temp:
        out = Path(temp) / ("kokoro_duronly.onnx" if args.duration_only else "kokoro_dur.onnx")
        torch.onnx.export(
            model, (ids, style, speed), str(out), dynamo=False,
            input_names=["input_ids", "style", "speed"],
            output_names=["duration"] if args.duration_only else ["waveform", "duration"],
            dynamic_axes={"input_ids": {0: "b", 1: "len"}, "style": {0: "b"}, "speed": {0: "b"}},
            opset_version=17, do_constant_folding=True, external_data=False,
        )
        import onnx
        onnx.checker.check_model(str(out))
        digest = hashlib.sha256(out.read_bytes()).hexdigest()
        dest = args.out / out.name
        os.replace(out, dest)
        metadata = {"repository": REPOSITORY, "revision": REVISION, "sha256": digest}
        dest.with_suffix(".json").write_text(json.dumps(metadata, indent=2) + "\n")
        print(f"wrote {dest} ({dest.stat().st_size / 1e6:.1f} MB), sha256 {digest}")


if __name__ == "__main__":
    main()
