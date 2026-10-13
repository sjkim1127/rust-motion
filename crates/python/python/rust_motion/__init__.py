"""
Rust Motion Python SDK — High-Performance Browser-Free Video Rendering Engine.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Union

# Import native PyO3 module
from ._rust_motion import (
    calculate_ducking,
    fit_text,
    get_audio_spectrum,
    get_bounding_box,
    get_safe_area_insets,
    get_tangent_at_length,
    get_version,
    get_video_metadata,
    interpolate,
    interpolate_colors,
    list_compositions,
    make_ellipse,
    parse_srt,
    parse_vtt,
    parse_whisper,
    random,
    render_native,
    reverse_path,
    rotate_path,
    scramble_text,
    spring,
    static_file,
    typewriter_text,
)
from .shorts import CaptionStyle, ShortsVideo

__version__ = get_version()
__all__ = [
    "__version__",
    "CaptionStyle",
    "Composition",
    "ShortsVideo",
    "calculate_ducking",
    "fit_text",
    "get_audio_spectrum",
    "get_bounding_box",
    "get_safe_area_insets",
    "get_tangent_at_length",
    "get_video_metadata",
    "interpolate",
    "interpolate_colors",
    "list_compositions",
    "make_ellipse",
    "parse_srt",
    "parse_vtt",
    "parse_whisper",
    "random",
    "render",
    "render_script",
    "render_still",
    "reverse_path",
    "rotate_path",
    "scramble_text",
    "spring",
    "static_file",
    "typewriter_text",
]


def render(
    composition: str,
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = None,
    width: int = 1920,
    height: int = 1080,
    fps: float = 30.0,
    duration: int = 180,
    backend: str = "native",
    codec: str = "h264",
    frame_start: int = 0,
    frame_end: Optional[int] = None,
    crf: int = 18,
    preset: str = "fast",
    hw_accel: str = "auto",
    sandbox_roots: Optional[List[Union[str, Path]]] = None,
    permissive: bool = False,
    progress_callback: Optional[Callable[[Dict[str, int]], None]] = None,
) -> None:
    """
    Render a registered video composition to an output video or still image.

    Args:
        composition: Name of the registered composition (e.g. 'hello-world')
        output: Destination file path (e.g. 'output.mp4', 'output.webm')
        props: Dynamic input parameters passed to the composition
        width: Video width in pixels (must be even for video codecs)
        height: Video height in pixels (must be even for video codecs)
        fps: Frames per second (default: 30.0)
        duration: Total length in frames (default: 180)
        backend: 'native' (tiny-skia CPU) or 'gpu' (wgpu)
        codec: Output codec ('h264', 'h265', 'vp9', 'av1', 'prores', 'gif', 'png', etc.)
        frame_start: Starting frame number
        frame_end: Ending frame number (inclusive, None for full duration)
        crf: Encoding quality factor (lower is higher quality, default: 18)
        preset: FFmpeg x264 preset ('fast', 'medium', 'slow', etc.)
        hw_accel: Hardware acceleration mode ('auto', 'disabled', 'videotoolbox', 'nvenc')
        sandbox_roots: List of allowed root paths for media security isolation
        permissive: Run in permissive mode without media sandbox jail
        progress_callback: Optional callback receiving completed_frames, total_frames, and frame;
            callback exceptions are raised as RuntimeError after rendering stops
    """
    props_json = json.dumps(props) if props is not None else None
    roots = [str(r) for r in sandbox_roots] if sandbox_roots is not None else None
    render_native(
        output=str(output),
        composition=composition,
        script=None,
        props_json=props_json,
        width=width,
        height=height,
        fps=fps,
        duration=duration,
        backend=backend,
        codec=codec,
        frame_start=frame_start,
        frame_end=frame_end,
        crf=crf,
        preset=preset,
        hw_accel=hw_accel,
        sandbox_roots=roots,
        permissive=permissive,
        progress_callback=progress_callback,
    )


def render_still(
    composition: str,
    frame: int,
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = None,
    width: int = 1920,
    height: int = 1080,
    fps: float = 30.0,
    backend: str = "native",
    sandbox_roots: Optional[List[Union[str, Path]]] = None,
    permissive: bool = False,
) -> None:
    """
    Render a single still image frame (PNG, JPEG, WebP) from a composition.

    Args:
        composition: Name of the registered composition
        frame: Frame number to render
        output: Destination file path (e.g. 'thumbnail.png', 'poster.jpg')
        props: Dynamic input parameters
        width: Image width in pixels
        height: Image height in pixels
        fps: Frame rate context
        backend: 'native' or 'gpu'
        sandbox_roots: List of allowed root paths for media security isolation
        permissive: Run in permissive mode without media sandbox jail
    """
    ext = Path(output).suffix.lstrip(".").lower()
    codec = ext if ext in ("png", "jpeg", "jpg", "webp") else "png"
    props_json = json.dumps(props) if props is not None else None
    roots = [str(r) for r in sandbox_roots] if sandbox_roots is not None else None

    is_script = str(composition).endswith(".rhai") or Path(composition).is_file()
    comp_arg = None if is_script else composition
    script_arg = str(composition) if is_script else None

    render_native(
        output=str(output),
        composition=comp_arg,
        script=script_arg,
        props_json=props_json,
        width=width,
        height=height,
        fps=fps,
        duration=frame + 1,
        backend=backend,
        codec=codec,
        frame_start=frame,
        frame_end=frame,
        crf=18,
        preset="fast",
        hw_accel="disabled",
        sandbox_roots=roots,
        permissive=permissive,
    )


def render_script(
    script_path: Union[str, Path],
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = None,
    width: int = 1920,
    height: int = 1080,
    fps: float = 30.0,
    duration: int = 180,
    backend: str = "native",
    codec: str = "h264",
    crf: int = 18,
    preset: str = "fast",
    hw_accel: str = "auto",
    sandbox_roots: Optional[List[Union[str, Path]]] = None,
    permissive: bool = False,
    progress_callback: Optional[Callable[[Dict[str, int]], None]] = None,
) -> None:
    """
    Render a dynamic Rhai video script (.rhai) without recompiling Rust code.

    Args:
        script_path: Path to the .rhai script file
        output: Destination video file
        props: Dynamic parameters passed to the script
        width: Video width
        height: Video height
        fps: Frames per second
        duration: Total frames
        backend: 'native' or 'gpu'
        codec: Output video codec
        hw_accel: Hardware acceleration mode ('auto', 'disabled', 'videotoolbox', 'nvenc')
        sandbox_roots: List of allowed root paths for media security isolation
        permissive: Run in permissive mode without media sandbox jail
    """
    props_json = json.dumps(props) if props is not None else None
    roots = [str(r) for r in sandbox_roots] if sandbox_roots is not None else None
    render_native(
        output=str(output),
        composition=None,
        script=str(script_path),
        props_json=props_json,
        width=width,
        height=height,
        fps=fps,
        duration=duration,
        backend=backend,
        codec=codec,
        frame_start=0,
        frame_end=None,
        crf=crf,
        preset=preset,
        hw_accel=hw_accel,
        sandbox_roots=roots,
        permissive=permissive,
        progress_callback=progress_callback,
    )


class Composition:
    """
    Object-oriented wrapper for a video composition.
    """

    def __init__(
        self,
        id_or_script: Union[str, Path],
        width: int = 1920,
        height: int = 1080,
        fps: float = 30.0,
        duration: int = 180,
    ):
        self.target = str(id_or_script)
        self.is_script = self.target.endswith(".rhai") or Path(self.target).is_file()
        self.width = width
        self.height = height
        self.fps = fps
        self.duration = duration

    def render(
        self,
        output: Union[str, Path],
        props: Optional[Dict[str, Any]] = None,
        backend: str = "native",
        codec: str = "h264",
        crf: int = 18,
        preset: str = "fast",
        hw_accel: str = "auto",
        sandbox_roots: Optional[List[Union[str, Path]]] = None,
        permissive: bool = False,
        progress_callback: Optional[Callable[[Dict[str, int]], None]] = None,
    ) -> None:
        """Render the complete video."""
        if self.is_script:
            render_script(
                script_path=self.target,
                output=output,
                props=props,
                width=self.width,
                height=self.height,
                fps=self.fps,
                duration=self.duration,
                backend=backend,
                codec=codec,
                crf=crf,
                preset=preset,
                hw_accel=hw_accel,
                sandbox_roots=sandbox_roots,
                permissive=permissive,
                progress_callback=progress_callback,
            )
        else:
            render(
                composition=self.target,
                output=output,
                props=props,
                width=self.width,
                height=self.height,
                fps=self.fps,
                duration=self.duration,
                backend=backend,
                codec=codec,
                crf=crf,
                preset=preset,
                hw_accel=hw_accel,
                sandbox_roots=sandbox_roots,
                permissive=permissive,
                progress_callback=progress_callback,
            )

    def render_still(
        self,
        frame: int,
        output: Union[str, Path],
        props: Optional[Dict[str, Any]] = None,
    ) -> None:
        """Render a single frame as a still image."""
        render_still(
            composition=self.target,
            frame=frame,
            output=output,
            props=props,
            width=self.width,
            height=self.height,
            fps=self.fps,
        )

    def __repr__(self) -> str:
        return (
            f"Composition(id='{self.target}', {self.width}x{self.height} @ "
            f"{self.fps}fps, {self.duration} frames)"
        )
