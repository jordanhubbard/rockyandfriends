#!/usr/bin/env python3
"""
HORDE Dispatcher - Factory Floor Render
Generates a 1920x1080 photorealistic industrial factory floor scene
with 6 GPU agent nodes in parallel processing lanes.
"""

import torch
import sys
import os

# Use writable HF cache (system HF cache is root-owned)
os.environ["HF_HOME"] = "/home/jkh/.openclaw/workspace/renders/hf_cache"
os.environ["HUGGINGFACE_HUB_CACHE"] = "/home/jkh/.openclaw/workspace/renders/hf_cache/hub"
os.makedirs(os.environ["HUGGINGFACE_HUB_CACHE"], exist_ok=True)

print(f"PyTorch: {torch.__version__}")
print(f"CUDA available: {torch.cuda.is_available()}")

# Use GPU if available, fall back to CPU
if torch.cuda.is_available():
    device = "cuda"
    dtype = torch.float16
    print(f"Using GPU: {torch.cuda.get_device_name(0)}")
else:
    device = "cpu"
    dtype = torch.float32
    print("WARNING: Falling back to CPU — will be slow")

from diffusers import StableDiffusionXLPipeline, EulerAncestralDiscreteScheduler
import gc

# Prompt engineered for the HORDE dispatcher slide
PROMPT = (
    "photorealistic industrial factory floor, six parallel GPU server rack lanes, "
    "dramatic cinematic lighting, dark background, blue and orange accent lighting, "
    "each lane has glowing computational nodes, server racks with pulsing LEDs, "
    "volumetric fog between lanes, clean high-tech facility, orthographic perspective, "
    "NVIDIA Omniverse aesthetic, Unreal Engine render quality, 8k ultra detail, "
    "professional architectural visualization, no humans, no text"
)

NEGATIVE_PROMPT = (
    "cartoon, anime, illustration, painting, low quality, blurry, noisy, "
    "science fiction fantasy, robots, humans, text, watermark, oversaturated, "
    "distorted, deformed, ugly, cluttered"
)

print("\nLoading SDXL-Turbo pipeline...")
print("(Model will be downloaded from HuggingFace if not cached — ~7GB)")

pipe = StableDiffusionXLPipeline.from_pretrained(
    "stabilityai/sdxl-turbo",
    torch_dtype=dtype,
    use_safetensors=True,
    variant="fp16" if dtype == torch.float16 else None,
)

pipe = pipe.to(device)

# Enable memory optimizations
pipe.enable_vae_slicing()
if hasattr(pipe, 'enable_xformers_memory_efficient_attention'):
    try:
        pipe.enable_xformers_memory_efficient_attention()
        print("xformers attention enabled")
    except Exception as e:
        print(f"xformers not available: {e}")

print(f"\nGenerating 1920x1080 render...")
print(f"Device: {device}, dtype: {dtype}")

# SDXL-Turbo works best at 512x512 then upscaled, but we'll try native
# For a slide, we generate at 1024x576 (16:9 native) then scale
generator = torch.Generator(device=device).manual_seed(42)

with torch.inference_mode():
    result = pipe(
        prompt=PROMPT,
        negative_prompt=NEGATIVE_PROMPT,
        width=1024,
        height=576,
        num_inference_steps=4,   # SDXL-Turbo is designed for 1-4 steps
        guidance_scale=0.0,      # Turbo uses CFG=0
        generator=generator,
    )

image = result.images[0]

# Upscale to 1920x1080 with high-quality Lanczos
from PIL import Image
image_hd = image.resize((1920, 1080), Image.LANCZOS)

output_path = "/home/jkh/.openclaw/workspace/renders/horde_factory_floor.png"
image_hd.save(output_path, "PNG", optimize=False)

print(f"\n✅ Render complete!")
print(f"Output: {output_path}")
print(f"Size: {image_hd.size}")

# Cleanup
del pipe, result
gc.collect()
if device == "cuda":
    torch.cuda.empty_cache()

print("GPU memory released.")
