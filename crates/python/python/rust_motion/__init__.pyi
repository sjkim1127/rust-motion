from pathlib import Path
from typing import Any, Dict, List, Optional, Union, overload

__version__: str

def random(seed: Union[str, int, float]) -> float: ...

def interpolate(
    input: float,
    input_range: List[float],
    output_range: List[float],
    extrapolate_left: str = "extend",
    extrapolate_right: str = "extend",
) -> float: ...

@overload
def interpolate_colors(
    input: float,
    input_range: List[float],
    output_range: List[str],
) -> str: ...

@overload
def interpolate_colors(
    from_color: str,
    to_color: str,
    progress: float,
) -> str: ...

def interpolate_colors(
    *args: Any,
) -> str: ...

def spring(
    frame: float,
    fps: float = 30.0,
    damping: float = 10.0,
    mass: float = 1.0,
    stiffness: float = 100.0,
    overshoot_clamping: bool = False,
) -> float: ...

def static_file(path: str) -> str: ...

def get_video_metadata(path: str) -> Dict[str, Any]: ...

def list_compositions() -> List[str]: ...

def render(
    composition: str,
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = ...,
    width: int = ...,
    height: int = ...,
    fps: float = ...,
    duration: int = ...,
    backend: str = ...,
    codec: str = ...,
    frame_start: int = ...,
    frame_end: Optional[int] = ...,
    crf: int = ...,
    preset: str = ...,
    hw_accel: str = ...,
) -> None: ...

def render_still(
    composition: str,
    frame: int,
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = ...,
    width: int = ...,
    height: int = ...,
    fps: float = ...,
    backend: str = ...,
) -> None: ...

def render_script(
    script_path: Union[str, Path],
    output: Union[str, Path],
    props: Optional[Dict[str, Any]] = ...,
    width: int = ...,
    height: int = ...,
    fps: float = ...,
    duration: int = ...,
    backend: str = ...,
    codec: str = ...,
    crf: int = ...,
    preset: str = ...,
    hw_accel: str = ...,
) -> None: ...

class Composition:
    target: str
    is_script: bool
    width: int
    height: int
    fps: float
    duration: int

    def __init__(
        self,
        id_or_script: Union[str, Path],
        width: int = 1920,
        height: int = 1080,
        fps: float = 30.0,
        duration: int = 180,
    ) -> None: ...

    def render(
        self,
        output: Union[str, Path],
        props: Optional[Dict[str, Any]] = ...,
        backend: str = ...,
        codec: str = ...,
        crf: int = ...,
        preset: str = ...,
        hw_accel: str = ...,
    ) -> None: ...

    def render_still(
        self,
        frame: int,
        output: Union[str, Path],
        props: Optional[Dict[str, Any]] = ...,
    ) -> None: ...
