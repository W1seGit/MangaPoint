"""Make the AI panel finder's model file from its published weights.

    pip install ultralytics onnx onnxslim huggingface_hub
    python tools/export-panel-model.py

Downloads ShadowB's YOLO26s manga segmentation model from Hugging Face and
exports it to ONNX at 1024 x 1024, the size `crates/panel-model` feeds it.
The file it writes is the one the app downloads (`PANELS` in
crates/panel-model/src/lib.rs); ONNX export is not byte-for-byte repeatable,
so a new export needs its checksum updated there.

Model: https://huggingface.co/ShadowB/Manga109-panel-balloon-text-yolov26-segmentation
Trained on Manga109-derived data (Manga109-s); built on Ultralytics YOLO26.
"""

import hashlib
import os
import shutil

from huggingface_hub import hf_hub_download
from ultralytics import YOLO

NAME = "panels-yolo26s-seg-1024-v1.onnx"

weights = hf_hub_download(
    "ShadowB/Manga109-panel-balloon-text-yolov26-segmentation", "best.pt"
)
shutil.copy(weights, "panels.pt")
exported = YOLO("panels.pt").export(
    format="onnx", imgsz=1024, opset=17, simplify=True, dynamic=False
)
shutil.move(exported, NAME)
os.remove("panels.pt")
with open(NAME, "rb") as f:
    digest = hashlib.sha256(f.read()).hexdigest()
print(f"{NAME}: {os.path.getsize(NAME)} bytes, sha256 {digest}")
