//! Opt-in download of remote HTTP(S) assets into a local cache.

use super::*;

impl Project {
    /// Download remote HTTP(S) assets into `cache_dir` and rewrite the project
    /// to use the downloaded local files. Both per-asset and total downloaded
    /// byte limits are required. This is opt-in so Browser projects can
    /// continue to let Chromium fetch remote media directly.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn materialize_remote_assets(
        &mut self,
        cache_dir: impl AsRef<std::path::Path>,
        max_asset_bytes: usize,
        max_total_bytes: usize,
    ) -> Result<(), ProjectError> {
        self.materialize_remote_assets_with_resolver(
            cache_dir,
            max_asset_bytes,
            max_total_bytes,
            &PublicRemoteAssetResolver,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn materialize_remote_assets_with_resolver(
        &mut self,
        cache_dir: impl AsRef<std::path::Path>,
        max_asset_bytes: usize,
        max_total_bytes: usize,
        resolver: &impl RemoteAssetResolver,
    ) -> Result<(), ProjectError> {
        if max_asset_bytes == 0 {
            return Err(ProjectError::AssetRead {
                asset: "remote".into(),
                reason: "per-asset remote byte limit must be greater than zero".into(),
            });
        }
        if max_total_bytes == 0 {
            return Err(ProjectError::AssetRead {
                asset: "remote".into(),
                reason: "total remote asset byte limit must be greater than zero".into(),
            });
        }
        let cache_dir = cache_dir.as_ref();
        std::fs::create_dir_all(cache_dir)
            .map_err(|error| ProjectError::File(error.to_string()))?;
        let mut replacements = Vec::new();
        let mut downloaded_bytes = 0usize;
        for asset in &mut self.assets {
            let source = asset.path.trim();
            if !has_http_scheme(source) {
                continue;
            }
            let mut current_url =
                reqwest::Url::parse(source).map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!("invalid remote asset URL: {error}"),
                })?;
            validate_remote_url(&current_url).map_err(|reason| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason,
            })?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut response = None;
            for redirect_count in 0..=5 {
                let addresses =
                    resolver
                        .resolve(&current_url)
                        .map_err(|reason| ProjectError::AssetRead {
                            asset: asset.id.clone(),
                            reason,
                        })?;
                if addresses.is_empty() {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: "remote asset hostname resolved to no addresses".into(),
                    });
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: "remote asset download timed out".into(),
                    });
                }
                let mut builder = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .timeout(remaining);
                if let Some(host) = current_url.host_str() {
                    if parse_url_ip_literal(host).is_none() {
                        builder = builder.resolve_to_addrs(host, &addresses);
                    }
                }
                let client = builder.build().map_err(|error| ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: error.to_string(),
                })?;
                let result = client.get(current_url.clone()).send().map_err(|error| {
                    ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: error.to_string(),
                    }
                })?;
                if result.status().is_redirection() {
                    if let Some(location) = result.headers().get(reqwest::header::LOCATION) {
                        let location =
                            location.to_str().map_err(|error| ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: format!("invalid remote asset redirect: {error}"),
                            })?;
                        if redirect_count == 5 {
                            return Err(ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: "remote asset exceeded the 5 redirect limit".into(),
                            });
                        }
                        current_url = current_url.join(location).map_err(|error| {
                            ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: format!("invalid remote asset redirect URL: {error}"),
                            }
                        })?;
                        validate_remote_url(&current_url).map_err(|reason| {
                            ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason,
                            }
                        })?;
                        continue;
                    }
                }
                response =
                    Some(
                        result
                            .error_for_status()
                            .map_err(|error| ProjectError::AssetRead {
                                asset: asset.id.clone(),
                                reason: error.to_string(),
                            })?,
                    );
                break;
            }
            let response = response.ok_or_else(|| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: "remote asset redirect did not produce a response".into(),
            })?;
            let remaining_total_bytes = max_total_bytes.saturating_sub(downloaded_bytes);
            if let Some(length) = response.content_length() {
                if length > max_asset_bytes as u64 {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: format!(
                            "remote asset exceeds the {max_asset_bytes} byte per-asset limit"
                        ),
                    });
                }
                if length > remaining_total_bytes as u64 {
                    return Err(ProjectError::AssetRead {
                        asset: asset.id.clone(),
                        reason: format!(
                            "project remote assets exceed the {max_total_bytes} byte total limit"
                        ),
                    });
                }
            }
            let read_limit = max_asset_bytes.min(remaining_total_bytes);
            let mut bytes = Vec::with_capacity(read_limit.min(1024 * 1024));
            std::io::Read::read_to_end(
                &mut response.take((read_limit as u64).saturating_add(1)),
                &mut bytes,
            )
            .map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: error.to_string(),
            })?;
            if bytes.len() > max_asset_bytes {
                return Err(ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!(
                        "remote asset exceeds the {max_asset_bytes} byte per-asset limit"
                    ),
                });
            }
            if bytes.len() > remaining_total_bytes {
                return Err(ProjectError::AssetRead {
                    asset: asset.id.clone(),
                    reason: format!(
                        "project remote assets exceed the {max_total_bytes} byte total limit"
                    ),
                });
            }
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if let Some(expected) = &asset.sha256 {
                if !expected.eq_ignore_ascii_case(&actual) {
                    return Err(ProjectError::AssetHashMismatch {
                        asset: asset.id.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
            // Use only the digest in the cache filename. Asset IDs are user
            // controlled metadata and must never influence filesystem paths.
            let filename = format!("{actual}.asset");
            let local = cache_dir.join(filename);
            std::fs::write(&local, &bytes).map_err(|error| ProjectError::AssetRead {
                asset: asset.id.clone(),
                reason: error.to_string(),
            })?;
            let reference = format!("asset://{}", asset.id);
            replacements.push((source.to_string(), local.to_string_lossy().into_owned()));
            replacements.push((reference, local.to_string_lossy().into_owned()));
            asset.path = local.to_string_lossy().into_owned();
            downloaded_bytes += bytes.len();
        }
        fn rewrite(value: &mut serde_json::Value, replacements: &[(String, String)]) {
            match value {
                serde_json::Value::String(text) => {
                    if let Some((_, replacement)) =
                        replacements.iter().find(|(source, _)| source == text)
                    {
                        *text = replacement.clone();
                    }
                }
                serde_json::Value::Array(values) => values
                    .iter_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Object(values) => values
                    .values_mut()
                    .for_each(|value| rewrite(value, replacements)),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => {}
            }
        }
        rewrite(&mut self.props, &replacements);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                rewrite(&mut clip.props, &replacements);
            }
        }
        Ok(())
    }
}
