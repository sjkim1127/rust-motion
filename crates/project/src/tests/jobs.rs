//! Render job store lifecycle tests.

use super::*;

#[test]
fn job_store_validates_and_tracks_progress() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    assert_eq!(store.get(&id).unwrap().status, JobStatus::Queued);
    assert!(store.update(&id, JobStatus::Preparing, 0));
    assert!(store.update(&id, JobStatus::Rendering, 12));
    assert_eq!(store.get(&id).unwrap().completed_frames, 12);
}

#[test]
fn invalid_job_transition_is_rejected() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    assert!(store.try_update(&id, JobStatus::Encoding, 0).is_err());
    assert!(store.try_update(&id, JobStatus::Preparing, 0).is_ok());
}

#[test]
fn progress_regression_and_failure_are_recorded() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store.try_update(&id, JobStatus::Preparing, 4).unwrap();
    assert!(matches!(
        store.try_update(&id, JobStatus::Rendering, 3),
        Err(ProjectError::ProgressRegressed { .. })
    ));
    store.fail(&id, "worker exited").unwrap();
    let job = store.get(&id).unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.error.as_deref(), Some("worker exited"));
}

#[test]
fn cancellation_preserves_progress() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store.update(&id, JobStatus::Preparing, 0);
    store.update(&id, JobStatus::Rendering, 12);
    store.cancel(&id).unwrap();
    let job = store.get(&id).unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
    assert_eq!(job.completed_frames, 12);
}

#[test]
fn cancellation_is_terminal_against_late_worker_updates() {
    let mut store = JobStore::default();
    let completed_id = store.submit(project()).unwrap();
    store.update(&completed_id, JobStatus::Preparing, 0);
    store.update(&completed_id, JobStatus::Rendering, 4);
    store.cancel(&completed_id).unwrap();
    assert!(matches!(
        store.try_update(&completed_id, JobStatus::Completed, 4),
        Err(ProjectError::InvalidJobTransition {
            from: JobStatus::Cancelled,
            to: JobStatus::Completed,
        })
    ));

    let failed_id = store.submit(project()).unwrap();
    store.update(&failed_id, JobStatus::Preparing, 0);
    store.update(&failed_id, JobStatus::Rendering, 4);
    store.cancel(&failed_id).unwrap();
    assert!(matches!(
        store.fail(&failed_id, "late worker failure"),
        Err(ProjectError::InvalidJobTransition {
            from: JobStatus::Cancelled,
            to: JobStatus::Failed,
        })
    ));
    assert_eq!(store.get(&failed_id).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn render_diagnostics_are_stored_and_serde_compatible() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store
        .set_render_diagnostics(
            &id,
            120,
            3,
            Some("scene contains GPU-unsupported nodes or effects".into()),
        )
        .unwrap();
    let job = store.get(&id).unwrap();
    assert_eq!(job.gpu_frames, Some(120));
    assert_eq!(job.cpu_fallback_frames, Some(3));
    assert_eq!(
        job.gpu_fallback_reason.as_deref(),
        Some("scene contains GPU-unsupported nodes or effects")
    );

    let encoded = serde_json::to_value(job).unwrap();
    let decoded: RenderJob = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, *job);
}

#[test]
fn render_job_lifecycle_keeps_diagnostics_until_completion() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store.try_update(&id, JobStatus::Preparing, 0).unwrap();
    store.try_update(&id, JobStatus::Rendering, 8).unwrap();
    store.set_render_diagnostics(&id, 8, 0, None).unwrap();
    store.set_encoding_progress(&id, 6).unwrap();
    assert_eq!(store.get(&id).unwrap().encoded_frames, 6);
    store.complete_render(&id, 8).unwrap();

    let job = store.get(&id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.completed_frames, 8);
    assert_eq!(job.encoded_frames, 8);
    assert_eq!(job.gpu_frames, Some(8));
    assert_eq!(job.cpu_fallback_frames, Some(0));
    assert_eq!(job.gpu_fallback_reason, None);
}

#[test]
fn complete_render_rejects_cancelled_job_without_reviving_it() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store.try_update(&id, JobStatus::Preparing, 0).unwrap();
    store.try_update(&id, JobStatus::Rendering, 4).unwrap();
    store.cancel(&id).unwrap();
    assert!(matches!(
        store.complete_render(&id, 4),
        Err(ProjectError::InvalidJobTransition {
            from: JobStatus::Cancelled,
            to: JobStatus::Completed,
        })
    ));
    assert_eq!(store.get(&id).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn late_encoding_progress_cannot_mutate_terminal_job() {
    let mut store = JobStore::default();
    let id = store.submit(project()).unwrap();
    store.try_update(&id, JobStatus::Preparing, 0).unwrap();
    store.try_update(&id, JobStatus::Rendering, 4).unwrap();
    store.cancel(&id).unwrap();

    assert!(matches!(
        store.set_encoding_progress(&id, 4),
        Err(ProjectError::InvalidJobTransition {
            from: JobStatus::Cancelled,
            to: JobStatus::Encoding,
        })
    ));
    assert_eq!(store.get(&id).unwrap().encoded_frames, 0);
}

#[test]
fn list_returns_jobs_in_submission_order() {
    let mut store = JobStore::default();
    let first = store.submit(project()).unwrap();
    let second = store.submit(project()).unwrap();
    let jobs = store.list();
    assert_eq!(
        jobs.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
        vec![first.as_str(), second.as_str()]
    );
}

#[test]
fn retry_creates_a_fresh_queued_job() {
    let mut store = JobStore::default();
    let original = store.submit(project()).unwrap();
    store.cancel(&original).unwrap();
    let retry = store.retry(&original).unwrap();
    assert_eq!(retry, "job-2");
    let retry_job = store.get(&retry).unwrap();
    assert_eq!(retry_job.status, JobStatus::Queued);
    assert_eq!(retry_job.project, project());
    assert_eq!(retry_job.gpu_frames, None);
    assert_eq!(retry_job.cpu_fallback_frames, None);
    assert_eq!(retry_job.gpu_fallback_reason, None);
}
