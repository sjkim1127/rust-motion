use clap::Parser;
use dioxuscut_cli::{
    default_render_control, execute_render_command_with_control, serve::ServeConfig, Cli, Commands,
    RenderRequest,
};
use std::time::Instant;

fn project_codec(output: &std::path::Path) -> anyhow::Result<dioxuscut_cli::RenderCodec> {
    match output.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => Ok(dioxuscut_cli::RenderCodec::Png),
        "jpg" | "jpeg" => Ok(dioxuscut_cli::RenderCodec::Jpeg),
        "webp" => Ok(dioxuscut_cli::RenderCodec::Webp),
        "mp4" => Ok(dioxuscut_cli::RenderCodec::H264),
        "webm" => Ok(dioxuscut_cli::RenderCodec::Vp9),
        "mov" => Ok(dioxuscut_cli::RenderCodec::ProRes),
        "gif" => Ok(dioxuscut_cli::RenderCodec::Gif),
        _ => anyhow::bail!("Cannot infer project codec from output extension; use .mp4, .webm, .mov, .gif, .png, .jpg, or .webp"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,dioxuscut_renderer=debug,dioxuscut_rasterizer=debug")
        .init();

    let cli = Cli::parse();

    match &cli.command {
        Commands::Render {
            composition,
            script,
            props,
            output,
            audio,
            width,
            height,
            scale,
            fps,
            duration,
            backend,
            codec,
            frame_start,
            frame_end,
            frame_step,
            concurrency,
            timeout_seconds,
            crf,
            preset,
            hw_accel,
            sandbox_roots,
            permissive,
            profile,
        } => {
            if *profile {
                std::env::set_var("DIOXUSCUT_PROFILE", "1");
            }
            let request = RenderRequest {
                composition: composition.clone(),
                script: script.clone(),
                props: props.clone(),
                output: output.clone(),
                audio: audio.clone(),
                width: *width,
                height: *height,
                scale: *scale,
                fps: *fps,
                duration: *duration,
                backend: *backend,
                codec: *codec,
                frame_start: *frame_start,
                frame_end: *frame_end,
                frame_step: *frame_step,
                concurrency: *concurrency,
                timeout_seconds: *timeout_seconds,
                crf: *crf,
                preset: preset.clone(),
                hw_accel: (*hw_accel).into(),
                sandbox_roots: sandbox_roots.clone(),
                permissive: *permissive,
            };
            let control = default_render_control(&request);
            let cancellation = control.cancellation_token();
            let signal_task = tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::warn!("Cancellation requested; stopping render");
                    cancellation.cancel();
                }
            });
            let result = execute_render_command_with_control(&request, control).await;
            signal_task.abort();
            if let Err(error) = result {
                if std::env::var_os("DIOXUSCUT_JSON").is_some() {
                    println!(
                        "{}",
                        serde_json::json!({
                            "ok": false,
                            "error": error.to_string(),
                            "output": request.output,
                            "backend": format!("{:?}", request.backend).to_ascii_lowercase(),
                            "codec": format!("{:?}", request.codec).to_ascii_lowercase(),
                        })
                    );
                }
                return Err(error);
            }
        }
        Commands::Migrate {
            input,
            target,
            output,
            experimental: _,
        } => {
            eprintln!(
                "⚠️  [EXPERIMENTAL] dioxuscut migrate generates a starting scaffold template."
            );
            eprintln!(
                "    Complex TypeScript logic and full CSS layouts require manual adaptation.\n"
            );

            let target_mode: dioxuscut_cli::MigrationTarget =
                target.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
            let source = std::fs::read_to_string(input)
                .map_err(|e| anyhow::anyhow!("Failed to read '{input:?}': {e}"))?;

            let (code, stats) = dioxuscut_cli::transpile_remotion(&source, target_mode)
                .map_err(|e| anyhow::anyhow!("Migration failed: {e}"))?;

            if let Some(out_path) = output {
                std::fs::write(out_path, &code)
                    .map_err(|e| anyhow::anyhow!("Failed to write to '{out_path:?}': {e}"))?;
                println!("✨ Successfully migrated {input:?} -> {out_path:?}");
            } else {
                println!("{code}");
            }

            eprintln!(
                "📊 Migration summary: {} hooks, {} interpolations, {} springs, {} sequences, {} loops converted",
                stats.hooks_converted,
                stats.interpolations_converted,
                stats.springs_converted,
                stats.sequences_converted,
                stats.loops_converted
            );
        }
        Commands::ListCompositions => {
            let compositions = dioxuscut_cli::built_in_registry()
                .ids()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if std::env::var_os("DIOXUSCUT_JSON").is_some() {
                println!(
                    "{}",
                    serde_json::json!({"ok": true, "compositions": compositions})
                );
            } else {
                for composition in compositions {
                    println!("{composition}");
                }
            }
        }
        Commands::WebcodecsDrift {
            worker,
            node,
            url,
            composition,
            width,
            height,
            fps,
            frame_start,
            frames,
            concurrency,
            repetitions,
            output,
            csv,
        } => {
            if *frames == 0 {
                anyhow::bail!("--frames must be greater than zero");
            }
            if *concurrency == 0 {
                anyhow::bail!("--concurrency must be greater than zero");
            }
            if *repetitions == 0 {
                anyhow::bail!("--repetitions must be greater than zero");
            }
            if !fps.is_finite() || *fps <= 0.0 {
                anyhow::bail!("--fps must be finite and greater than zero");
            }
            let backend = dioxuscut_rasterizer::BrowserFrameBackend::with_concurrency(
                node,
                worker,
                url.clone(),
                *concurrency,
            )?;
            backend.set_composition(composition.clone())?;
            let frame_numbers = (0..*frames)
                .map(|offset| {
                    frame_start
                        .checked_add(offset)
                        .ok_or_else(|| anyhow::anyhow!("frame range overflows u32"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let render_sequence = || -> anyhow::Result<(
                dioxuscut_rasterizer::WebFrameDriftReport,
                std::time::Duration,
            )> {
                let started = Instant::now();
                let mut samples = Vec::with_capacity(*frames as usize);
                for batch in frame_numbers.chunks(*concurrency) {
                    let backend_ref = &backend;
                    let batch_results = std::thread::scope(|scope| {
                        let handles = batch.iter().map(|&frame| {
                            let backend = backend_ref;
                            scope.spawn(move || {
                                let request = dioxuscut_rasterizer::WebFrameRequest {
                                    composition: Some(composition.clone()),
                                    frame,
                                    fps: *fps,
                                    width: *width,
                                    height: *height,
                                    props: serde_json::json!({}),
                                    assets: Vec::new(),
                                    timeline: Vec::new(),
                                    time_events: Vec::new(),
                                    image_format: None,
                                    jpeg_quality: None,
                                    transparent: true,
                                    transport: Some("rgba".into()),
                                };
                                let (_, timing) = backend
                                    .render_web_frame_with_timing(&request)
                                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                                let timing = timing.ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "frame {frame} did not return WebCodecs timing metadata"
                                    )
                                })?;
                                Ok::<_, anyhow::Error>((frame, timing))
                            })
                        });
                        handles
                            .map(|handle| {
                                handle.join().map_err(|_| {
                                    anyhow::anyhow!("WebCodecs frame worker panicked")
                                })?
                            })
                            .collect::<anyhow::Result<Vec<_>>>()
                    })?;
                    samples.extend(batch_results);
                }
                samples.sort_by_key(|(frame, _)| *frame);
                let report = dioxuscut_rasterizer::WebFrameDriftReport::from_samples(&samples)
                    .ok_or_else(|| anyhow::anyhow!("no WebCodecs timing samples were collected"))?;
                Ok((report, started.elapsed()))
            };
            let mut runs = Vec::with_capacity(*repetitions);
            for _ in 0..*repetitions {
                // Repetitions are intended to measure the same uncached
                // workload, not cache-hit latency from the previous run.
                backend.clear_frame_cache();
                runs.push(render_sequence()?);
            }
            let report = runs[0].0;
            let mut elapsed_ms = runs
                .iter()
                .map(|(_, duration)| duration.as_secs_f64() * 1000.0)
                .collect::<Vec<_>>();
            elapsed_ms.sort_by(f64::total_cmp);
            let percentile = |rank: usize| {
                let nearest_rank = (elapsed_ms.len() * rank).div_ceil(100).max(1);
                elapsed_ms[nearest_rank - 1]
            };
            let artifact = serde_json::json!({
                "drift": report,
                "repetitions": elapsed_ms.len(),
                "elapsed_ms": elapsed_ms,
                "p50_ms": percentile(50),
                "p95_ms": percentile(95),
            });
            std::fs::write(output, serde_json::to_string_pretty(&artifact)?)?;
            if let Some(csv_path) = csv {
                let mut csv_output = String::from(
                    "repetition,elapsed_ms,sample_count,mean_abs_drift_frames,max_abs_drift_frames,non_contiguous_samples\n",
                );
                for (index, elapsed) in elapsed_ms.iter().enumerate() {
                    csv_output.push_str(&format!(
                        "{},{:.3},{}\n",
                        index + 1,
                        elapsed,
                        report.to_csv_row()
                    ));
                }
                std::fs::write(csv_path, csv_output)?;
            }
            println!(
                "validated {} WebCodecs frames across {} repetitions (p50 {:.2}ms, p95 {:.2}ms)",
                report.sample_count,
                elapsed_ms.len(),
                percentile(50),
                percentile(95)
            );
        }
        Commands::ValidateProject { input } => {
            let project = dioxuscut_project::Project::load(input)
                .map_err(|error| anyhow::anyhow!("Project validation failed: {error}"))?;
            if let Some(parent) = input.parent() {
                project.validate_asset_files(parent)?;
            }
            println!("{}", serde_json::to_string_pretty(&project)?);
        }
        Commands::RenderProject {
            input,
            output,
            asset_cache_dir,
            max_total_asset_bytes,
        } => {
            let mut project = dioxuscut_project::Project::load(input)
                .map_err(|error| anyhow::anyhow!("Project validation failed: {error}"))?;
            if let Some(parent) = input.parent() {
                project.validate_asset_files(parent)?;
                project.resolve_local_asset_paths(parent);
            }
            if let Some(cache_dir) = asset_cache_dir {
                project.materialize_remote_assets(
                    cache_dir,
                    256 * 1024 * 1024,
                    *max_total_asset_bytes,
                )?;
            }
            let props_path = std::env::temp_dir().join(format!(
                "dioxuscut-project-props-{}.json",
                std::process::id()
            ));
            std::fs::write(&props_path, serde_json::to_vec(&project.props)?)?;
            let request = RenderRequest {
                composition: Some(project.composition.clone()),
                script: None,
                props: Some(props_path.clone()),
                output: output.clone(),
                audio: dioxuscut_cli::project_audio_assets_from_dir(
                    &project,
                    input.parent().unwrap_or_else(|| std::path::Path::new(".")),
                ),
                width: project.settings.width,
                height: project.settings.height,
                scale: project.settings.scale,
                fps: project.settings.fps,
                duration: project.settings.duration,
                backend: match project.settings.backend {
                    dioxuscut_project::BackendKind::Native => dioxuscut_cli::RenderBackend::Native,
                    dioxuscut_project::BackendKind::Browser => {
                        dioxuscut_cli::RenderBackend::Browser
                    }
                    dioxuscut_project::BackendKind::Gpu => dioxuscut_cli::RenderBackend::Gpu,
                },
                codec: project_codec(output)?,
                frame_start: project.settings.frame_start.unwrap_or(0),
                frame_end: project.settings.frame_end,
                frame_step: project.settings.frame_step,
                concurrency: project.settings.concurrency.map(|value| value as usize),
                timeout_seconds: None,
                crf: project.settings.crf.unwrap_or(18),
                preset: project
                    .settings
                    .preset
                    .clone()
                    .unwrap_or_else(|| "fast".into()),
                hw_accel: dioxuscut_rasterizer::HwAccel::Auto,
                sandbox_roots: vec![],
                permissive: true,
            };
            let previous_browser_assets = std::env::var_os("DIOXUSCUT_BROWSER_ASSETS");
            let previous_browser_timeline = std::env::var_os("DIOXUSCUT_BROWSER_TIMELINE");
            let previous_browser_image_format = std::env::var_os("DIOXUSCUT_BROWSER_IMAGE_FORMAT");
            let previous_browser_jpeg_quality = std::env::var_os("DIOXUSCUT_BROWSER_JPEG_QUALITY");
            let previous_browser_frame_timeout =
                std::env::var_os("DIOXUSCUT_BROWSER_FRAME_TIMEOUT_MS");
            let previous_browser_transport = std::env::var_os("DIOXUSCUT_BROWSER_TRANSPORT");
            let previous_browser_transport_retries =
                std::env::var_os("DIOXUSCUT_BROWSER_TRANSPORT_RETRIES");
            if request.backend == dioxuscut_cli::RenderBackend::Browser {
                let asset_separator = if cfg!(windows) { ';' } else { ':' };
                std::env::set_var(
                    "DIOXUSCUT_BROWSER_ASSETS",
                    project
                        .assets
                        .iter()
                        .map(|asset| asset.path.as_str())
                        .collect::<Vec<_>>()
                        .join(&asset_separator.to_string()),
                );
                let timeline = project
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .map(|clip| dioxuscut_rasterizer::WebTimelineClip {
                        id: clip.id.clone(),
                        composition: clip.composition.clone(),
                        start: clip.start,
                        duration: clip.duration,
                        props: clip.props.clone(),
                    })
                    .collect::<Vec<_>>();
                std::env::set_var(
                    "DIOXUSCUT_BROWSER_TIMELINE",
                    serde_json::to_string(&timeline)?,
                );
                set_optional_browser_env(
                    "DIOXUSCUT_BROWSER_IMAGE_FORMAT",
                    project.settings.browser_image_format.as_deref(),
                );
                set_optional_browser_env(
                    "DIOXUSCUT_BROWSER_JPEG_QUALITY",
                    project
                        .settings
                        .browser_jpeg_quality
                        .map(|value| value.to_string())
                        .as_deref(),
                );
                set_optional_browser_env(
                    "DIOXUSCUT_BROWSER_FRAME_TIMEOUT_MS",
                    project
                        .settings
                        .browser_frame_timeout_ms
                        .map(|value| value.to_string())
                        .as_deref(),
                );
                set_optional_browser_env(
                    "DIOXUSCUT_BROWSER_TRANSPORT_RETRIES",
                    project
                        .settings
                        .browser_transport_retries
                        .map(|value| value.to_string())
                        .as_deref(),
                );
                set_optional_browser_env(
                    "DIOXUSCUT_BROWSER_TRANSPORT",
                    project
                        .settings
                        .browser_transport
                        .as_deref()
                        .map(str::trim)
                        .map(str::to_ascii_lowercase)
                        .as_deref(),
                );
            }
            let result = dioxuscut_cli::execute_project_render_command_with_control(
                &request,
                &project,
                dioxuscut_cli::default_render_control(&request),
            )
            .await;
            match previous_browser_assets {
                Some(value) => std::env::set_var("DIOXUSCUT_BROWSER_ASSETS", value),
                None => std::env::remove_var("DIOXUSCUT_BROWSER_ASSETS"),
            }
            match previous_browser_timeline {
                Some(value) => std::env::set_var("DIOXUSCUT_BROWSER_TIMELINE", value),
                None => std::env::remove_var("DIOXUSCUT_BROWSER_TIMELINE"),
            }
            restore_browser_env(
                "DIOXUSCUT_BROWSER_IMAGE_FORMAT",
                previous_browser_image_format,
            );
            restore_browser_env(
                "DIOXUSCUT_BROWSER_JPEG_QUALITY",
                previous_browser_jpeg_quality,
            );
            restore_browser_env(
                "DIOXUSCUT_BROWSER_FRAME_TIMEOUT_MS",
                previous_browser_frame_timeout,
            );
            restore_browser_env(
                "DIOXUSCUT_BROWSER_TRANSPORT_RETRIES",
                previous_browser_transport_retries,
            );
            restore_browser_env("DIOXUSCUT_BROWSER_TRANSPORT", previous_browser_transport);
            let _ = std::fs::remove_file(props_path);
            result?;
        }
        Commands::Probe { path, full } => {
            if *full {
                let metadata = dioxuscut_cli::parse_media(path, 30.0)?;
                println!("{}", serde_json::to_string_pretty(&metadata)?);
                return Ok(());
            }
            match dioxuscut_cli::get_video_metadata(path) {
                Ok(meta) => {
                    println!("{}", serde_json::to_string_pretty(&meta)?);
                }
                Err(e) => {
                    // Try audio probe
                    match dioxuscut_cli::get_audio_metadata(path, 30.0) {
                        Ok(audio_meta) => {
                            println!("{}", serde_json::to_string_pretty(&audio_meta)?);
                        }
                        Err(_) => {
                            anyhow::bail!("Probe failed for {path:?}: {e}");
                        }
                    }
                }
            }
        }
        Commands::Serve {
            script,
            props,
            port,
            frame,
            width,
            height,
            fps,
            duration,
        } => {
            dioxuscut_cli::serve::run(ServeConfig {
                script: script.clone(),
                props: props.clone(),
                port: *port,
                default_frame: *frame,
                width: *width,
                height: *height,
                fps: *fps,
                duration: *duration,
            })
            .await?;
        }
    }

    Ok(())
}

fn set_optional_browser_env(name: &str, value: Option<&str>) {
    if let Some(value) = value {
        std::env::set_var(name, value);
    } else {
        std::env::remove_var(name);
    }
}

fn restore_browser_env(name: &str, value: Option<std::ffi::OsString>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}
