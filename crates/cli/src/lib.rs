//! CLI options, validation, native composition registry, and render execution.

pub mod args;
pub mod audio;
pub mod composition;
pub(crate) mod composition_shim;
pub mod doctor;
pub mod inspect;
pub mod migrate;
pub mod render;
#[cfg(feature = "rhai")]
pub mod rhai_runtime;
pub mod serve;
pub mod studio;
pub mod validate;

pub use args::{
    resolve_browser_worker_path, resolve_hybrid_backend, Cli, Commands, HwAccelArg, RenderBackend,
    RenderCodec, RenderRequest, ValidationError, EXIT_GENERAL_ERROR, EXIT_INVALID_ARGUMENTS,
    EXIT_MISSING_DEPENDENCY, EXIT_SUCCESS, EXIT_TIMEOUT_OR_CANCELLED, EXIT_VALIDATION_ERROR,
};
pub use audio::{project_audio_assets, project_audio_assets_from_dir};
pub use composition::{
    built_in_registry, BarChartRaceComposition, CodeTerminalComposition,
    ComplexGradientsComposition, Composition, CompositionError, CompositionRegistry,
    CompositionRegistryError, CyberpunkGridComposition, HelloWorldComposition,
    KaraokeCaptionsComposition, NativeComposition, NativeCompositionContext,
    PodcastWaveformComposition, PreparedComposition, ShapesAndFiltersComposition,
    TimeEventSchedule,
};
pub use doctor::{run_doctor, DoctorReport, TempDirCheck, ToolCheck};
pub use inspect::{
    run_inspect_backends, run_inspect_codecs, run_inspect_compositions, BackendCapability,
    CodecCapability,
};
pub use migrate::{transpile_remotion, MigrationStats, MigrationTarget};
pub use render::{
    default_render_control, execute_project_render_command_with_control, execute_render_command,
    execute_render_command_with_control, execute_render_command_with_registry,
    execute_render_command_with_registry_and_control,
};
#[cfg(feature = "rhai")]
pub use rhai_runtime::{RhaiComposition, SceneBuilder};
pub use rust_motion_media::{
    get_audio_metadata, get_video_metadata, parse_media, static_file, AudioMetadata,
    ParsedMediaMetadata, VideoMetadata,
};
pub use validate::{
    run_validate, validate_composition_source, validate_render_params, ValidationReport,
};
