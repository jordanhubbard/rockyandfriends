#!/usr/bin/env python3
"""
HORDE Dispatcher - Factory Floor Render (High Quality)
Generates a 1920x1080 photorealistic industrial factory floor scene
with 6 GPU agent nodes in parallel processing lanes.
Uses full SDXL for higher quality output.
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

device = "cuda" if torch.cuda.is_available() else "cpu"
dtype = torch.float16 if device == "cuda" else torch.float32
print(f"Using: {device} / {dtype}")

from diffusers import StableDiffusionXLPipeline, DPMSolverMultistepScheduler
import gc

# Refined prompt for maximum photorealism
PROMPT = (
    "photorealistic industrial server farm factory floor, "
    "six parallel processing lanes stretching into distance, "
    "each lane contains tall black server racks with blue LED lights glowing, "
    "overhead industrial lighting casting dramatic shadows, "
    "dark concrete floor with reflections, volumetric light shafts, "
    "deep perspective corridor, clean modern data center aesthetic, "
    "NVIDIA DGX infrastructure, professional architectural photography, "
    "cinematic composition, f/2.8 depth of field, ultra sharp, 8k, "
    "no people, no text, no logos"
)

NEGATIVE_PROMPT = (
    "painting, drawing, illustration, cartoon, anime, sketch, "
    "blurry, noisy, grainy, low quality, artifacts, "
    "humans, people, robots, humanoids, "
    "text, watermark, signature, "
    "overexposed, washed out, flat lighting, "
    "fantasy, science fiction aesthetic, "
    "distorted, deformed"
)

print("\nLoading SDXL base pipeline...")
pipe = StableDiffusionXLPipeline.from_pretrained(
    "stabilityai/stable-diffusion-xl-base-1.0",
    torch_dtype=dtype,
    use_safetensors=True,
    variant="fp16" if dtype == torch.float16 else None,
)

# Use DPM++ 2M Karras for crisp, fast convergence
pipe.scheduler = DPMSolverMultistepScheduler.from_config(
    pipe.scheduler.config,
    use_karras_sigmas=True,
    algorithm_type="dpmsolver++"
)

pipe = pipe.to(device)
pipe.vae.enable_slicing()

print(f"\nGenerating at 1024x576 (16:9 native SDXL)...")
generator = torch.Generator(device=device).manual_seed(1337)

with torch.inference_mode():
    result = pipe(
        prompt=PROMPT,
        negative_prompt=NEGATIVE_PROMPT,
        width=1024,
        height=576,
        num_inference_steps=30,
        guidance_scale=7.5,
        generator=generator,
    )

image = result.images[0]

# Save intermediate
mid_path = "/home/jkh/.openclaw/workspace/renders/horde_factory_floor_mid.png"
image.save(mid_path, "PNG")
print(f"Intermediate saved: {mid_path} ({image.size})")

# Upscale to 1920x1080
from PIL import Image
image_hd = image.resize((1920, 1080), Image.LANCZOS)

output_path = "/home/jkh/.openclaw/workspace/renders/horde_factory_floor_hq.png"
image_hd.save(output_path, "PNG", optimize=False)

print(f"\n✅ HQ Render complete!")
print(f"Output: {output_path}")
print(f"Size: {image_hd.size}")

del pipe, result
gc.collect()
if device == "cuda":
    torch.cuda.empty_cache()
print("GPU memory released.")
