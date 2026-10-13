"""
Rust Motion Shorts & Reels Video Builder.

Provides a high-level, AI-friendly Python API to generate 9:16 vertical videos
(TikTok, Instagram Reels, YouTube Shorts) with word-by-word kinetic highlight subtitles,
color emojis, Lottie vector animations, and background audio/video in ~5 lines of code.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

from . import (
    calculate_ducking,
    get_audio_spectrum,
    get_video_metadata,
    parse_srt,
    parse_vtt,
    parse_whisper,
    render_native,
)


class CaptionStyle(str, Enum):
    """Popular short-form caption styling presets."""

    HORMOZI = "hormozi"  # Bold uppercase, active word pop 1.15x, dark pill background
    BOUNCE = "bounce"  # High energy scale bounce 1.25x, yellow highlight, no box
    MINIMAL_PILL = "minimal_pill"  # Clean semi-bold, subtle translucent dark pill
    CLEAN = "clean"  # Pure text, no background box


@dataclass
class SubtitleConfig:
    tokens: List[Dict[str, Any]]
    style: CaptionStyle = CaptionStyle.HORMOZI
    active_color: str = "#ffe600"
    inactive_color: str = "#ffffff"
    font_size: float = 56.0
    max_words_per_line: int = 3
    center_x: float = 540.0
    baseline_y: float = 1280.0
    bg_color: Optional[str] = "rgba(0,0,0,0.75)"
    bg_padding_x: float = 24.0
    bg_padding_y: float = 12.0
    bg_radius: float = 14.0
    active_scale: float = 1.15


@dataclass
class LottieSticker:
    src: str
    x: float
    y: float
    width: float
    height: float
    start_sec: float = 0.0
    duration_sec: Optional[float] = None


@dataclass
class EmojiSticker:
    emoji: str
    x: float
    y: float
    size: float
    start_sec: float = 0.0
    duration_sec: Optional[float] = None


@dataclass
class AudioVisualizerConfig:
    src: str
    x: float
    y: float
    width: float
    height: float
    style: str = "bars"  # "bars", "wave", "radial"
    color: str = "#00e5ff"
    start_sec: float = 0.0
    duration_sec: Optional[float] = None


@dataclass
class AudioTrackConfig:
    src: str
    volume: float = 1.0
    looped: bool = False
    volume_keyframes: List[Tuple[float, float]] = field(default_factory=list)


class ShortsVideo:
    """
    High-level Builder for automated vertical short-form video generation (1080x1920).
    """

    def __init__(
        self,
        width: int = 1080,
        height: int = 1920,
        fps: float = 30.0,
        duration_in_frames: Optional[int] = None,
        bg_color: str = "#0b0d19",
    ):
        self.width = width
        self.height = height
        self.fps = fps
        self.duration_in_frames = duration_in_frames
        self.bg_color = bg_color
        self.bg_video: Optional[str] = None
        self.bg_video_loop: bool = True
        self.bg_image: Optional[str] = None

        self.audio_tracks: List[AudioTrackConfig] = []
        self.visualizers: List[AudioVisualizerConfig] = []
        self.speech_intervals: List[Tuple[float, float]] = []
        self.subtitles: Optional[SubtitleConfig] = None
        self.lottie_stickers: List[LottieSticker] = []
        self.emoji_stickers: List[EmojiSticker] = []

    def set_background_video(self, video_path: Union[str, Path], loop: bool = True) -> ShortsVideo:
        """Set a background looping or static video."""
        self.bg_video = str(Path(video_path).resolve())
        self.bg_video_loop = loop
        return self

    def set_background_image(self, image_path: Union[str, Path]) -> ShortsVideo:
        """Set a static background image."""
        self.bg_image = str(Path(image_path).resolve())
        return self

    def add_audio(
        self,
        audio_path: Union[str, Path],
        volume: float = 1.0,
        loop: bool = False,
    ) -> ShortsVideo:
        """Add a general audio track."""
        resolved = str(Path(audio_path).resolve())
        self.audio_tracks.append(
            AudioTrackConfig(src=resolved, volume=volume, looped=loop)
        )
        return self

    def add_voiceover(
        self,
        audio_path: Union[str, Path],
        volume: float = 1.0,
    ) -> ShortsVideo:
        """
        Add the primary speech / narration voiceover track.
        """
        resolved = str(Path(audio_path).resolve())
        self.audio_tracks.append(
            AudioTrackConfig(src=resolved, volume=volume, looped=False)
        )
        return self

    def add_background_music(
        self,
        audio_path: Union[str, Path],
        volume: float = 0.35,
        duck_on_voice: bool = True,
        duck_volume: float = 0.08,
        attack_sec: float = 0.25,
        release_sec: float = 0.40,
        loop: bool = True,
    ) -> ShortsVideo:
        """
        Add background music track with smart auto-ducking during speech.

        Args:
            audio_path: Path to background music file (MP3, WAV, AAC, M4A)
            volume: Normal music volume in 0.0..1.0 (default 0.35)
            duck_on_voice: Automatically lower volume when speech is active (default True)
            duck_volume: Attenuated volume during speech (default 0.08)
            attack_sec: Fade down duration in seconds (default 0.25s)
            release_sec: Fade up duration in seconds (default 0.40s)
            loop: Repeat music if video is longer than the track (default True)
        """
        resolved = str(Path(audio_path).resolve())
        self.audio_tracks.append(
            AudioTrackConfig(
                src=resolved,
                volume=volume,
                looped=loop,
                volume_keyframes=[],
            )
        )
        return self

    def add_audio_visualizer(
        self,
        audio_path: Union[str, Path],
        x: float,
        y: float,
        width: float = 400.0,
        height: float = 120.0,
        style: str = "bars",
        color: str = "#00e5ff",
        start_sec: float = 0.0,
        duration_sec: Optional[float] = None,
    ) -> ShortsVideo:
        """
        Add a dynamic frequency spectrum or waveform visualizer.

        Args:
            audio_path: Audio file (MP3, WAV, AAC, M4A)
            x, y, width, height: Bounding box in canvas pixels
            style: 'bars' (classic spectrum), 'wave' (oscilloscope), or 'radial' (circular podcast avatar)
            color: Color hex or rgba (e.g. '#00e5ff', '#ff007f')
            start_sec: Video start time offset
            duration_sec: Optional duration on timeline
        """
        resolved = str(Path(audio_path).resolve())
        self.visualizers.append(
            AudioVisualizerConfig(
                src=resolved,
                x=x,
                y=y,
                width=width,
                height=height,
                style=style.lower(),
                color=color,
                start_sec=start_sec,
                duration_sec=duration_sec,
            )
        )
        return self

    def add_subtitles(
        self,
        data: Union[str, Path, List[Dict[str, Any]]],
        style: Union[CaptionStyle, str] = CaptionStyle.HORMOZI,
        active_color: str = "#ffe600",
        inactive_color: str = "#ffffff",
        font_size: float = 56.0,
        max_words_per_line: int = 3,
        center_x: Optional[float] = None,
        baseline_y: Optional[float] = None,
        bg_color: Optional[str] = None,
        active_scale: Optional[float] = None,
    ) -> ShortsVideo:
        """
        Add word-by-word kinetic highlight subtitles.

        Args:
            data: File path (.srt, .vtt, .json), raw JSON string, or Whisper words list
            style: 'hormozi', 'bounce', 'minimal_pill', or 'clean'
            active_color: Color of active spoken word (e.g. '#ffe600')
            inactive_color: Color of unhighlighted words (default '#ffffff')
            font_size: Pixel size of subtitle text (default 56.0)
            max_words_per_line: Max words grouped per subtitle page (default 3)
            center_x: Horizontal center position (default width / 2)
            baseline_y: Vertical baseline position (default height * 0.65)
        """
        tokens = self._load_tokens(data)
        if isinstance(style, str):
            style = CaptionStyle(style.lower())

        resolved_cx = center_x if center_x is not None else (self.width / 2.0)
        resolved_by = baseline_y if baseline_y is not None else (self.height * 0.68)

        # Style presets
        if style == CaptionStyle.HORMOZI:
            default_bg = "rgba(0,0,0,0.78)" if bg_color is None else bg_color
            default_scale = 1.15 if active_scale is None else active_scale
        elif style == CaptionStyle.BOUNCE:
            default_bg = None if bg_color is None else bg_color
            default_scale = 1.25 if active_scale is None else active_scale
        elif style == CaptionStyle.MINIMAL_PILL:
            default_bg = "rgba(15,23,42,0.65)" if bg_color is None else bg_color
            default_scale = 1.05 if active_scale is None else active_scale
        else:  # CLEAN
            default_bg = None if bg_color is None else bg_color
            default_scale = 1.0 if active_scale is None else active_scale

        for t in tokens:
            s_sec = t["start_ms"] / 1000.0
            e_sec = t["end_ms"] / 1000.0
            if e_sec > s_sec:
                self.speech_intervals.append((s_sec, e_sec))

        self.subtitles = SubtitleConfig(
            tokens=tokens,
            style=style,
            active_color=active_color,
            inactive_color=inactive_color,
            font_size=font_size,
            max_words_per_line=max_words_per_line,
            center_x=resolved_cx,
            baseline_y=resolved_by,
            bg_color=default_bg,
            active_scale=default_scale,
        )
        return self

    def add_lottie(
        self,
        lottie_path: Union[str, Path],
        x: float,
        y: float,
        size: float = 200.0,
        start_sec: float = 0.0,
        duration_sec: Optional[float] = None,
    ) -> ShortsVideo:
        """Add a Lottie sticker on the video."""
        resolved = str(Path(lottie_path).resolve())
        self.lottie_stickers.append(
            LottieSticker(
                src=resolved,
                x=x,
                y=y,
                width=size,
                height=size,
                start_sec=start_sec,
                duration_sec=duration_sec,
            )
        )
        return self

    def add_emoji(
        self,
        emoji: str,
        x: float,
        y: float,
        size: float = 96.0,
        start_sec: float = 0.0,
        duration_sec: Optional[float] = None,
    ) -> ShortsVideo:
        """Add a standalone color emoji sticker (e.g. '🔥', '🚀', '💡')."""
        self.emoji_stickers.append(
            EmojiSticker(
                emoji=emoji,
                x=x,
                y=y,
                size=size,
                start_sec=start_sec,
                duration_sec=duration_sec,
            )
        )
        return self

    def get_safe_area(self, platform: str = "tiktok") -> Dict[str, float]:
        """
        Get unobstructed screen bounds for TikTok, Instagram Reels, or YouTube Shorts.

        Returns:
            Dict containing 'top', 'bottom', 'left', 'right', 'safe_x', 'safe_y', 'safe_width', 'safe_height'.
        """
        from . import get_safe_area_insets

        return get_safe_area_insets(platform, float(self.width), float(self.height))

    def render(
        self,
        output: Union[str, Path],
        hw_accel: str = "auto",
        codec: str = "h264",
        crf: int = 18,
        preset: str = "fast",
        sandbox_roots: Optional[List[Union[str, Path]]] = None,
        permissive: bool = False,
    ) -> None:
        """
        Renders the complete short-form video to an output MP4/WebM file.
        """
        output_path = Path(output).resolve()
        duration_frames = self._resolve_duration_in_frames()

        rhai_code = self._generate_rhai_script()
        temp_dir = Path(std_env_temp_dir())
        script_file = temp_dir / f"shorts_{output_path.stem}_{id(self)}.rhai"
        script_file.write_text(rhai_code, encoding="utf-8")

        # Automatically collect roots for declared media assets
        roots: List[str] = []
        if sandbox_roots:
            roots.extend(str(Path(r).resolve()) for r in sandbox_roots)
        if self.bg_video:
            roots.append(str(Path(self.bg_video).resolve().parent))
        if self.bg_image:
            roots.append(str(Path(self.bg_image).resolve().parent))
        for track in self.audio_tracks:
            roots.append(str(Path(track.src).resolve().parent))
        for viz in self.visualizers:
            roots.append(str(Path(viz.src).resolve().parent))
        for lottie in self.lottie_stickers:
            roots.append(str(Path(lottie.src).resolve().parent))
        roots.append(str(output_path.parent))
        roots.append(str(temp_dir.resolve()))

        try:
            render_native(
                output=str(output_path),
                composition=None,
                script=str(script_file),
                props_json=None,
                width=self.width,
                height=self.height,
                fps=self.fps,
                duration=duration_frames,
                backend="native",
                codec=codec,
                frame_start=0,
                frame_end=None,
                crf=crf,
                preset=preset,
                hw_accel=hw_accel,
                sandbox_roots=roots if roots else None,
                permissive=permissive,
            )
        finally:
            if script_file.exists():
                try:
                    script_file.unlink()
                except OSError:
                    pass

    def _resolve_duration_in_frames(self) -> int:
        if self.duration_in_frames is not None and self.duration_in_frames > 0:
            return self.duration_in_frames

        # Auto-detect from subtitles
        max_ms = 0
        if self.subtitles and self.subtitles.tokens:
            max_ms = max(t["end_ms"] for t in self.subtitles.tokens)

        # Or auto-detect from first audio track
        if self.audio_tracks:
            try:
                meta = get_video_metadata(self.audio_tracks[0].src)
                audio_dur_ms = int(meta["duration_in_seconds"] * 1000)
                max_ms = max(max_ms, audio_dur_ms)
            except Exception:
                pass

        if max_ms > 0:
            return int((max_ms / 1000.0) * self.fps)

        return int(5.0 * self.fps)  # Default 5 seconds fallback

    def _load_tokens(
        self, data: Union[str, Path, List[Dict[str, Any]]]
    ) -> List[Dict[str, Any]]:
        if isinstance(data, list):
            # Already list of tokens/words
            processed = []
            for item in data:
                text = item.get("text") or item.get("word") or ""
                start_ms = item.get("start_ms")
                end_ms = item.get("end_ms")
                if start_ms is None and "start" in item:
                    start_ms = int(float(item["start"]) * 1000.0)
                if end_ms is None and "end" in item:
                    end_ms = int(float(item["end"]) * 1000.0)
                processed.append(
                    {
                        "text": str(text).strip(),
                        "start_ms": int(start_ms or 0),
                        "end_ms": int(end_ms or (start_ms or 0) + 300),
                    }
                )
            return processed

        path_obj = Path(data)
        if path_obj.is_file():
            content = path_obj.read_text(encoding="utf-8")
            ext = path_obj.suffix.lower()
            if ext == ".srt":
                return parse_srt(content)
            elif ext in (".vtt", ".webvtt"):
                return parse_vtt(content)
            else:
                return parse_whisper(content)

        # Raw string
        content = str(data).strip()
        if content.startswith("[") or content.startswith("{"):
            return parse_whisper(content)
        elif "-->" in content:
            if content.startswith("WEBVTT"):
                return parse_vtt(content)
            return parse_srt(content)

        raise ValueError(
            "Unsupported subtitle input format. Expected path to .srt/.vtt/.json or JSON string."
        )

    def _generate_rhai_script(self) -> str:
        lines = [
            "fn render(ctx, props) {",
            "    let frame = ctx.frame.to_float();",
            "    let fps = ctx.fps;",
            "    let time_sec = frame / fps;",
            "    let time_ms = (time_sec * 1000.0);",
            "    let output = scene();",
            f'    output.rect(0.0, 0.0, {float(self.width)}, {float(self.height)}, "{self.bg_color}");',
        ]

        # Background video
        if self.bg_video:
            looped_str = "true" if self.bg_video_loop else "false"
            clean_path = self.bg_video.replace("\\", "/")
            lines.append(
                f'    output.video(0.0, 0.0, {float(self.width)}, {float(self.height)}, "{clean_path}", time_sec, "cover", 1.0, {looped_str});'
            )
        elif self.bg_image:
            clean_path = self.bg_image.replace("\\", "/")
            lines.append(
                f'    output.image(0.0, 0.0, {float(self.width)}, {float(self.height)}, "{clean_path}", "cover", 1.0);'
            )

        # Auto-ducking resolution
        duration_sec = self._resolve_duration_in_frames() / self.fps
        for track in self.audio_tracks:
            if not track.volume_keyframes and self.speech_intervals and track.volume < 0.9:
                track.volume_keyframes = calculate_ducking(
                    self.speech_intervals,
                    duration_sec,
                    base_volume=1.0,
                    duck_volume=0.25,
                )

        # Audio tracks
        for track in self.audio_tracks:
            clean_path = track.src.replace("\\", "/")
            if track.volume_keyframes:
                kfs_json = json.dumps([[round(t, 4), round(v, 4)] for t, v in track.volume_keyframes])
                lines.append(
                    f'    output.audio_ducked("{clean_path}", {float(track.volume)}, {kfs_json});'
                )
            else:
                lines.append(
                    f'    output.audio("{clean_path}", {float(track.volume)});'
                )

        # Audio visualizers
        for viz in self.visualizers:
            clean_src = viz.src.replace("\\", "/")
            conds = [f"time_sec >= {viz.start_sec}"]
            if viz.duration_sec is not None:
                conds.append(f"time_sec <= {viz.start_sec + viz.duration_sec}")
            cond_expr = " && ".join(conds)
            lines.append(
                f'    if {cond_expr} {{ output.audio_visualizer({viz.x}, {viz.y}, {viz.width}, {viz.height}, "{clean_src}", time_sec, "{viz.color}", "{viz.style}"); }}'
            )

        # Lottie stickers
        for lottie in self.lottie_stickers:
            clean_src = lottie.src.replace("\\", "/")
            lines.append(f"    if time_sec >= {lottie.start_sec} {{")
            if lottie.duration_sec is not None:
                end_sec = lottie.start_sec + lottie.duration_sec
                lines.append(f"        if time_sec <= {end_sec} {{")
                lines.append(
                    f'            output.lottie({lottie.x}, {lottie.y}, {lottie.width}, {lottie.height}, "{clean_src}", time_sec - {lottie.start_sec});'
                )
                lines.append("        }")
            else:
                lines.append(
                    f'        output.lottie({lottie.x}, {lottie.y}, {lottie.width}, {lottie.height}, "{clean_src}", time_sec - {lottie.start_sec});'
                )
            lines.append("    }")

        # Standalone Emoji stickers
        for emoji in self.emoji_stickers:
            lines.append(f"    if time_sec >= {emoji.start_sec} {{")
            if emoji.duration_sec is not None:
                end_sec = emoji.start_sec + emoji.duration_sec
                lines.append(f"        if time_sec <= {end_sec} {{")
                lines.append(
                    f'            output.emoji({emoji.x}, {emoji.y}, {emoji.size}, "{emoji.emoji}");'
                )
                lines.append("        }")
            else:
                lines.append(
                    f'        output.emoji({emoji.x}, {emoji.y}, {emoji.size}, "{emoji.emoji}");'
                )
            lines.append("    }")

        # Subtitles
        if self.subtitles and self.subtitles.tokens:
            pages = self._paginate_subtitles(
                self.subtitles.tokens, self.subtitles.max_words_per_line
            )
            for page in pages:
                p_start = page["start_ms"]
                p_end = page["end_ms"]
                lines.append(
                    f"    if time_ms >= {p_start}.0 && time_ms <= {p_end}.0 {{"
                )

                # Background pill
                if self.subtitles.bg_color:
                    total_chars = sum(len(t["text"]) for t in page["tokens"])
                    approx_w = (
                        total_chars * self.subtitles.font_size * 0.58
                        + len(page["tokens"]) * 14.0
                    )
                    pill_w = approx_w + self.subtitles.bg_padding_x * 2.0
                    pill_h = (
                        self.subtitles.font_size * 1.15
                        + self.subtitles.bg_padding_y * 2.0
                    )
                    pill_x = self.subtitles.center_x - (pill_w / 2.0)
                    pill_y = (
                        self.subtitles.baseline_y
                        - (self.subtitles.font_size * 0.85)
                        - self.subtitles.bg_padding_y
                    )
                    lines.append(
                        f'        output.round_rect({pill_x}, {pill_y}, {pill_w}, {pill_h}, "{self.subtitles.bg_color}", {self.subtitles.bg_radius});'
                    )

                # Render words
                total_chars = sum(len(t["text"]) for t in page["tokens"])
                row_w = (
                    total_chars * self.subtitles.font_size * 0.58
                    + len(page["tokens"]) * 14.0
                )
                cur_x = self.subtitles.center_x - (row_w / 2.0)

                for t in page["tokens"]:
                    w_text = (
                        t["text"].upper()
                        if self.subtitles.style == CaptionStyle.HORMOZI
                        else t["text"]
                    )
                    w_text_escaped = w_text.replace('"', '\\"')
                    w_start = t["start_ms"]
                    w_end = t["end_ms"]
                    w_len = len(w_text)
                    w_width = w_len * self.subtitles.font_size * 0.58

                    lines.append(
                        f"        if time_ms >= {w_start}.0 && time_ms <= {w_end}.0 {{"
                    )
                    lines.append(
                        f'            output.text_bold({cur_x}, {self.subtitles.baseline_y}, "{w_text_escaped}", {self.subtitles.font_size * self.subtitles.active_scale}, "{self.subtitles.active_color}");'
                    )
                    lines.append("        } else {")
                    lines.append(
                        f'            output.text_bold({cur_x}, {self.subtitles.baseline_y}, "{w_text_escaped}", {self.subtitles.font_size}, "{self.subtitles.inactive_color}");'
                    )
                    lines.append("        }")
                    cur_x += w_width + 14.0

                lines.append("    }")

        lines.append("    output")
        lines.append("}")
        return "\n".join(lines)

    def _paginate_subtitles(
        self, tokens: List[Dict[str, Any]], max_words: int
    ) -> List[Dict[str, Any]]:
        pages = []
        limit = max(1, max_words)
        for i in range(0, len(tokens), limit):
            chunk = tokens[i : i + limit]
            if not chunk:
                continue
            pages.append(
                {
                    "tokens": chunk,
                    "start_ms": chunk[0]["start_ms"],
                    "end_ms": chunk[-1]["end_ms"],
                }
            )
        return pages


def std_env_temp_dir() -> str:
    import tempfile

    return tempfile.gettempdir()
