"""
Render NVIDIA logo in the sky using ovrtx (NVIDIA RTX hardware ray tracing).
Uses the ovrtx Python bindings with a USD scene.
"""

import sys
import os
from pathlib import Path

# Ensure ovrtx C library is findable
ovrtx_bin = Path(os.path.expanduser("~/.local/lib/python3.12/site-packages/ovrtx/bin"))
if ovrtx_bin.exists():
    ld_path = os.environ.get("LD_LIBRARY_PATH", "")
    os.environ["LD_LIBRARY_PATH"] = f"{ovrtx_bin}:{ld_path}"

import numpy as np
from PIL import Image

import ovrtx
from ovrtx import Renderer, RendererConfig, Device

print(f"ovrtx version: {ovrtx.__version__}")

# Scene and output paths
scene_path = Path(__file__).parent / "nvidia_sky_rtx.usda"
output_path = Path(__file__).parent / "nvidia_sky_rtx.png"

# Create renderer
print("Creating RTX renderer...")
config = RendererConfig()
renderer = Renderer(config)
print(f"Renderer config: {renderer.config}")

# Load USD scene
print(f"Loading scene: {scene_path}")
handle = renderer.add_usd(str(scene_path))
if handle is None:
    print("ERROR: Failed to load USD scene!")
    sys.exit(1)
print(f"Scene loaded, handle: {handle}")

render_product = "/Render/Products/MainView"

# Warm up (path tracer needs frames to converge)
WARMUP = 32
print(f"Warming up renderer ({WARMUP} frames)...")
for i in range(WARMUP):
    products = renderer.step(render_products={render_product}, delta_time=0.016)
    if products is None:
        print(f"WARNING: step returned None at frame {i}")
    if i % 8 == 0:
        print(f"  frame {i}/{WARMUP}")

# Final render
print("Rendering final frame...")
products = renderer.step(render_products={render_product}, delta_time=0.016)

if products is None or render_product not in products:
    print(f"ERROR: No output for render product '{render_product}'")
    print(f"Available products: {list(products.keys()) if products else 'None'}")
    sys.exit(1)

product = products[render_product]
print(f"Got product with {len(product.frames)} frames")

for frame in product.frames:
    if "LdrColor" not in frame.render_vars:
        print(f"WARNING: LdrColor not in render vars: {list(frame.render_vars.keys())}")
        continue

    render_var = frame.render_vars["LdrColor"]
    with render_var.map(device=Device.CPU) as mapping:
        np_array = np.from_dlpack(mapping.tensor)
        print(f"Image array shape: {np_array.shape}, dtype: {np_array.dtype}")
        img = Image.fromarray(np_array)
        img.save(output_path)
        print(f"\n✅ Saved: {output_path}")
        print(f"   Size: {img.size[0]}x{img.size[1]}")

print("Done!")
