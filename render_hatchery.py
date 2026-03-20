#!/usr/bin/env python3
"""
🐔 RayChicken Hatchery Protest — ovrtx RTX Renderer + Physics Simulation
Produces: hatchery_protest.mp4 (30 seconds @ 60fps via RTX path tracing)

Architecture:
  - OpenUSD scene: hatchery_scene.usda
  - Physics: numpy-based projectile/parabolic simulation (eggs + chicken bob)
  - Rendering: ovrtx RTX (NVIDIA Omniverse RTX — Sparky GB10 Blackwell)
  - Output: frame PNGs → ffmpeg → MP4
"""

import math
import os
import sys
import time
import subprocess
import numpy as np
from pathlib import Path

import ovrtx
from PIL import Image

WORKSPACE   = Path("/home/jkh/.openclaw/workspace")
SCENE_FILE  = str(WORKSPACE / "hatchery_scene.usda")
FRAMES_DIR  = WORKSPACE / "hatchery_frames"
OUTPUT_MP4  = str(WORKSPACE / "hatchery_protest.mp4")
PREVIEW_PNG = str(WORKSPACE / "hatchery_preview.png")

FPS         = 60
DURATION    = 6.0          # seconds (360 frames — keep memory sane for first run)
NUM_FRAMES  = int(FPS * DURATION)
DT          = 1.0 / FPS

RENDER_W    = 1280
RENDER_H    = 720

# ─── Utility: build a USD row-vector 4x4 transform ───────────────────────────
# USD convention: translation in last row [3][0..2]
def mat4_identity():
    return np.eye(4, dtype=np.float64)

def mat4_translate(x, y, z):
    m = np.eye(4, dtype=np.float64)
    m[3, 0], m[3, 1], m[3, 2] = x, y, z
    return m

def mat4_rotate_y(angle_rad):
    c, s = math.cos(angle_rad), math.sin(angle_rad)
    m = np.eye(4, dtype=np.float64)
    m[0,0], m[0,2] = c, s
    m[2,0], m[2,2] = -s, c
    return m

def mat4_rotate_z(angle_rad):
    c, s = math.cos(angle_rad), math.sin(angle_rad)
    m = np.eye(4, dtype=np.float64)
    m[0,0], m[0,1] = c, s
    m[1,0], m[1,1] = -s, c
    return m

def mat4_trs(tx, ty, tz, ry=0.0, rz=0.0, sx=1.0, sy=1.0, sz=1.0):
    """Translate + Y-rotate + Z-rotate + uniform scale (good enough for protest chickens)."""
    scale = np.diag([sx, sy, sz, 1.0]).astype(np.float64)
    rot   = mat4_rotate_y(ry) @ mat4_rotate_z(rz)
    rot[3,:] = [0,0,0,1]  # zero translation in rot
    tr    = mat4_translate(tx, ty, tz)
    return scale @ rot @ tr

# ─── Physics simulation ───────────────────────────────────────────────────────
GRAVITY = -980.0  # cm/s² (scene units are ~1cm each)

class Egg:
    def __init__(self, path, x0, y0, z0, vx, vy, vz, spin=0.0):
        self.path  = path
        self.pos   = np.array([x0, y0, z0], dtype=np.float64)
        self.vel   = np.array([vx, vy, vz], dtype=np.float64)
        self.angle = 0.0
        self.spin  = spin       # rad/s rotation around Y
        self.alive = True
        self.splat = False

    def step(self, dt):
        if not self.alive:
            return
        self.vel[1] += GRAVITY * dt
        self.pos    += self.vel * dt
        self.angle  += self.spin * dt
        # floor collision
        if self.pos[1] <= 8:
            self.pos[1] = 8
            self.vel    = np.zeros(3)
            self.splat  = True
            self.alive  = False
        # back-wall collision  (z ~ -350)
        if self.pos[2] <= -350:
            self.pos[2] = -349
            self.vel    = np.zeros(3)
            self.splat  = True
            self.alive  = False

    def transform(self):
        x, y, z = self.pos
        return mat4_translate(x, y, z) @ mat4_rotate_y(self.angle)

    def splat_transform(self):
        x, y, z = self.pos
        # Flatten egg into a yolk puddle
        scale = np.diag([2.2, 0.25, 2.2, 1.0]).astype(np.float64)
        tr    = mat4_translate(x, y, z)
        return scale @ tr


def build_egg_salvo(t_offset):
    """Throw a volley of eggs. t_offset staggers the launches."""
    eggs = [
        Egg("/World/FlyingEgg0", 50, 145, 120,  480, 520, -620, spin=3.5),
        Egg("/World/FlyingEgg1", 50, 145, 120,  320, 580, -610, spin=-4.2),
        Egg("/World/FlyingEgg2", 50, 145, 120,  560, 490, -590, spin=5.0),
        Egg("/World/FlyingEgg3", 50, 145, 120,  400, 540, -640, spin=-3.0),
    ]
    return eggs


def chicken_bob_transform(t):
    """Protest chicken bobs up and down and rotates slightly, indignant."""
    bob_y  = 8.0  * math.sin(t * 3.0)
    lean_z = 0.06 * math.sin(t * 3.0 + 0.5)   # rock forward-back
    sway_y = 0.04 * math.sin(t * 1.2)           # slow sway left-right
    return mat4_translate(0, bob_y, 130) @ mat4_rotate_y(sway_y) @ mat4_rotate_z(lean_z)


def throwing_wing_transform(t):
    """The throwing wing windmills during protest."""
    throw_angle = 0.7 * math.sin(t * 4.0)
    base = mat4_translate(35, 72, 18)
    rot  = mat4_rotate_z(math.radians(55) + throw_angle)
    return base @ rot


# ─── Main ─────────────────────────────────────────────────────────────────────
def main():
    FRAMES_DIR.mkdir(exist_ok=True)

    print("🐔 RayChicken Hatchery — RTX Render starting on Sparky GB10 Blackwell")
    print(f"   Scene:    {SCENE_FILE}")
    print(f"   Frames:   {NUM_FRAMES} @ {FPS}fps  ({DURATION}s)")
    print(f"   Output:   {OUTPUT_MP4}")
    print()

    # ── Create renderer ──────────────────────────────────────────────
    print("Creating ovrtx Renderer (first run compiles+caches shaders — be patient)...", flush=True)
    t0 = time.time()
    renderer = ovrtx.Renderer()
    print(f"  Renderer ready in {time.time()-t0:.1f}s", flush=True)

    # ── Load scene ───────────────────────────────────────────────────
    print(f"Loading USD scene...", flush=True)
    renderer.add_usd(SCENE_FILE)
    print("  Scene loaded.", flush=True)

    # ── Physics state ────────────────────────────────────────────────
    sim_time = 0.0
    eggs = build_egg_salvo(t_offset=0)
    # Respawn salvo periodically
    salvo_timer = 0.0
    SALVO_PERIOD = 2.0   # new volley every 2 seconds

    RENDER_PRODUCT = "/Render/Camera"

    print(f"\nRendering {NUM_FRAMES} frames...", flush=True)
    frame_times = []

    for frame_idx in range(NUM_FRAMES):
        ft0 = time.time()

        # ── Physics tick ────────────────────────────────────────────
        for egg in eggs:
            egg.step(DT)

        salvo_timer += DT
        if salvo_timer >= SALVO_PERIOD:
            salvo_timer = 0.0
            eggs = build_egg_salvo(sim_time)

        # ── Write transforms ────────────────────────────────────────

        # Protest chicken body bob
        renderer.write_attribute(
            prim_paths= ["/World/ProtestChicken"],
            attribute_name= "omni:xform",
            tensor= chicken_bob_transform(sim_time).reshape(1,4,4),
        )

        # Throwing wing animation
        renderer.write_attribute(
            prim_paths= ["/World/ProtestChicken/WingR"],
            attribute_name= "omni:xform",
            tensor= throwing_wing_transform(sim_time).reshape(1,4,4),
        )

        # Camera gentle orbit
        cam_angle  = 0.12 * math.sin(sim_time * 0.3)
        cam_dist   = 500.0
        cam_x      = cam_dist * math.sin(cam_angle) + 100
        cam_z      = cam_dist * math.cos(cam_angle)
        cam_xform  = mat4_translate(cam_x, 180, cam_z) @ mat4_rotate_y(-cam_angle)
        renderer.write_attribute(
            prim_paths= ["/World/MainCam"],
            attribute_name= "omni:xform",
            tensor= cam_xform.reshape(1,4,4),
        )

        # Flying eggs
        egg_paths = [e.path for e in eggs]
        egg_xforms = np.stack([e.transform() for e in eggs])
        if egg_paths:
            renderer.write_attribute(
                prim_paths= egg_paths,
                attribute_name= "omni:xform",
                tensor= egg_xforms,
            )

        # ── Render step ─────────────────────────────────────────────
        products = renderer.step(
            render_products = {RENDER_PRODUCT},
            delta_time      = DT,
        )

        # ── Capture frame ───────────────────────────────────────────
        for _name, product in products.items():
            for frame_out in product.frames:
                with frame_out.render_vars["LdrColor"].map(device=ovrtx.Device.CPU) as rv:
                    pixels = rv.tensor.numpy()
                    img = Image.fromarray(pixels)
                    frame_path = FRAMES_DIR / f"frame_{frame_idx:05d}.png"
                    img.save(str(frame_path))
                    # Save first frame as preview
                    if frame_idx == 0:
                        img.save(PREVIEW_PNG)
                        print(f"  Preview saved: {PREVIEW_PNG}", flush=True)

        sim_time += DT
        elapsed = time.time() - ft0
        frame_times.append(elapsed)

        if frame_idx % 30 == 0:
            avg_fps = 1.0 / (sum(frame_times[-30:]) / min(30, len(frame_times)))
            eta     = (NUM_FRAMES - frame_idx) / max(avg_fps, 0.01)
            print(f"  Frame {frame_idx:4d}/{NUM_FRAMES}  render={elapsed*1000:.0f}ms  avg={avg_fps:.1f}fps  ETA={eta:.0f}s", flush=True)

    print(f"\n✅ All {NUM_FRAMES} frames rendered.", flush=True)

    # ── Encode video ─────────────────────────────────────────────────
    print(f"Encoding MP4 → {OUTPUT_MP4}...", flush=True)
    cmd = [
        "/home/jkh/.local/lib/python3.12/site-packages/imageio_ffmpeg/binaries/ffmpeg-linux-aarch64-v7.0.2", "-y",
        "-framerate", str(FPS),
        "-i",    str(FRAMES_DIR / "frame_%05d.png"),
        "-c:v",  "libx264",
        "-pix_fmt", "yuv420p",
        "-crf",  "18",
        "-preset", "fast",
        OUTPUT_MP4,
    ]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode == 0:
        size_mb = os.path.getsize(OUTPUT_MP4) / 1_048_576
        print(f"✅ Video encoded: {OUTPUT_MP4}  ({size_mb:.1f} MB)", flush=True)
    else:
        print(f"❌ ffmpeg failed:\n{result.stderr}", flush=True)
        sys.exit(1)

    total = time.time() - t0  # reuse t0 from renderer creation
    print(f"\n🎬 Done in {total:.0f}s total. RayChicken protests his conditions.")


if __name__ == "__main__":
    main()
