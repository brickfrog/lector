#!/usr/bin/env -S uv run --with kokoro --with onnx --with onnxscript --with torch --script
"""Export a DURATION-ONLY Kokoro ONNX model.

The full model's vocoder (istftnet decoder) is the bulk of the size and the part
that (a) needs disable_complex for ONNX export and (b) falls back to CPU under
ort-web's webgpu backend. For the karaoke pipeline we only need the per-token
durations (pred_dur), which are computed *before* the decoder. Exporting just
that path yields a ~56 MB, webgpu-clean model whose duration is bit-identical to
the full model.

Pair this with onnx-community's stock model for the audio (same weights, so the
durations line up with the generated waveform).

  uv run reference/export_duronly.py
  -> reference/models/kokoro_duronly.onnx
"""

import os

import torch
from kokoro import KModel


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
    km = KModel().eval()  # no disable_complex needed: the decoder isn't traced
    model = DurationOnly(km).eval()
    ids = torch.LongTensor([[0, *torch.randint(1, 100, (48,)).tolist(), 0]])
    style = torch.randn(1, 256)
    speed = torch.IntTensor([1])
    os.makedirs("reference/models", exist_ok=True)
    out = "reference/models/kokoro_duronly.onnx"
    torch.onnx.export(
        model, (ids, style, speed), out, dynamo=False,
        input_names=["input_ids", "style", "speed"], output_names=["duration"],
        dynamic_axes={"input_ids": {0: "b", 1: "len"}, "style": {0: "b"}, "speed": {0: "b"}},
        opset_version=17, do_constant_folding=True,
    )
    print(f"wrote {out}  ({os.path.getsize(out) / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
