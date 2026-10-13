#!/usr/bin/env python3
"""
High-Performance Headless Video Rendering Microservice powered by Rust Motion & FastAPI.

Run with:
    uvicorn examples.python.fastapi_render_server:app --port 8000 --reload
"""

import time
import uuid
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from pydantic import BaseModel, Field

import rust_motion

app = FastAPI(
    title="Rust Motion Headless Video Microservice",
    description="Render programmatic videos with zero Chromium overhead in milliseconds.",
    version=rust_motion.__version__,
)

ROOT = Path(__file__).resolve().parent.parent.parent
TEMPLATE_PATH = ROOT / "examples/templates/shorts_caption.rhai"
OUTPUT_DIR = ROOT / "target/server_renders"
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


class RenderRequest(BaseModel):
    headline: str = Field(..., example="Breaking News:\nAI Video Infrastructure")
    caption: str = Field(..., example="Created dynamically via FastAPI & Rust Motion.")
    tag: str = Field(default="FASTAPI ENGINE", example="FINTECH DAILY")
    background: str = Field(default="#0b0d19", example="#0b0d19")
    accent: str = Field(default="#ff007f33", example="#00f0ff33")
    duration_sec: float = Field(default=3.0, ge=0.5, le=60.0)
    fps: float = Field(default=30.0)


@app.get("/health")
def health_check():
    return {
        "status": "healthy",
        "engine": "Rust Motion Native",
        "version": rust_motion.__version__,
        "compositions": rust_motion.list_compositions(),
    }


@app.post("/v1/render/shorts")
def render_shorts_video(req: RenderRequest):
    """
    Renders a vertical 9:16 (1080x1920) video and returns the MP4 file directly.
    """
    job_id = uuid.uuid4().hex[:12]
    out_file = OUTPUT_DIR / f"short_{job_id}.mp4"
    total_frames = int(req.duration_sec * req.fps)

    props = {
        "headline": req.headline,
        "caption": req.caption,
        "tag": req.tag,
        "background": req.background,
        "accent": req.accent,
        "text_color": "#ffffff",
    }

    start = time.time()
    try:
        rust_motion.render_script(
            script_path=TEMPLATE_PATH,
            output=out_file,
            props=props,
            width=1080,
            height=1920,
            fps=req.fps,
            duration=total_frames,
            codec="h264",
            crf=18,
            preset="fast",
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Rendering failed: {str(e)}")

    elapsed = time.time() - start
    return FileResponse(
        path=out_file,
        media_type="video/mp4",
        filename=f"short_{job_id}.mp4",
        headers={
            "X-Render-Time-Sec": f"{elapsed:.3f}",
            "X-Render-FPS": f"{total_frames / elapsed:.1f}",
        },
    )


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=8000)
