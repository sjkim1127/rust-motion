//! In-memory render job store and its lifecycle transitions.

use super::*;

/// In-memory job registry used by desktop hosts and suitable for replacement
/// by a persistent/server implementation without changing the API contract.
#[derive(Debug, Default)]
pub struct JobStore {
    jobs: BTreeMap<String, RenderJob>,
    next_id: u64,
}

impl JobStore {
    pub fn submit(&mut self, project: Project) -> Result<String, ProjectError> {
        project.validate()?;
        self.next_id += 1;
        let id = format!("job-{}", self.next_id);
        self.jobs.insert(
            id.clone(),
            RenderJob {
                id: id.clone(),
                project,
                status: JobStatus::Queued,
                completed_frames: 0,
                encoded_frames: 0,
                error: None,
                output: None,
                gpu_frames: None,
                cpu_fallback_frames: None,
                gpu_fallback_reason: None,
            },
        );
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Option<&RenderJob> {
        self.jobs.get(id)
    }

    pub fn list(&self) -> Vec<RenderJob> {
        self.jobs.values().cloned().collect()
    }

    pub fn set_output(&mut self, id: &str, output: impl Into<String>) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        job.output = Some(output.into());
        Ok(())
    }

    pub fn set_render_diagnostics(
        &mut self,
        id: &str,
        gpu_frames: u64,
        cpu_fallback_frames: u64,
        fallback_reason: Option<String>,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        job.gpu_frames = Some(gpu_frames);
        job.cpu_fallback_frames = Some(cpu_fallback_frames);
        job.gpu_fallback_reason = fallback_reason;
        Ok(())
    }

    pub fn set_encoding_progress(
        &mut self,
        id: &str,
        encoded_frames: u32,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued | JobStatus::Preparing | JobStatus::Rendering | JobStatus::Encoding
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Encoding,
            });
        }
        if encoded_frames < job.encoded_frames {
            return Err(ProjectError::ProgressRegressed {
                previous: job.encoded_frames,
                next: encoded_frames,
            });
        }
        job.encoded_frames = encoded_frames;
        Ok(())
    }

    /// Advance a successfully rendered job through encoding to completion.
    /// Keeping this transition sequence in the store prevents backend workers
    /// from accidentally omitting or reordering terminal states.
    pub fn complete_render(&mut self, id: &str, completed_frames: u32) -> Result<(), ProjectError> {
        let status = self
            .jobs
            .get(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?
            .status
            .clone();
        if status == JobStatus::Preparing {
            self.try_update(id, JobStatus::Rendering, completed_frames)?;
        } else if status != JobStatus::Rendering {
            return Err(ProjectError::InvalidJobTransition {
                from: status,
                to: JobStatus::Completed,
            });
        }
        self.try_update(id, JobStatus::Encoding, completed_frames)?;
        self.set_encoding_progress(id, completed_frames)?;
        self.try_update(id, JobStatus::Completed, completed_frames)
    }

    pub fn update(&mut self, id: &str, status: JobStatus, completed_frames: u32) -> bool {
        self.try_update(id, status, completed_frames).is_ok()
    }

    pub fn try_update(
        &mut self,
        id: &str,
        status: JobStatus,
        completed_frames: u32,
    ) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        let valid = matches!(
            (&job.status, &status),
            (
                JobStatus::Queued,
                JobStatus::Preparing | JobStatus::Cancelled | JobStatus::Failed
            ) | (
                JobStatus::Preparing,
                JobStatus::Rendering | JobStatus::Cancelled | JobStatus::Failed
            ) | (
                JobStatus::Rendering,
                JobStatus::Rendering
                    | JobStatus::Encoding
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ) | (
                JobStatus::Encoding,
                JobStatus::Encoding
                    | JobStatus::Completed
                    | JobStatus::Cancelled
                    | JobStatus::Failed
            ) | (JobStatus::Completed, JobStatus::Completed)
                | (JobStatus::Failed, JobStatus::Failed)
                | (JobStatus::Cancelled, JobStatus::Cancelled)
        );
        if !valid {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: status,
            });
        }
        if completed_frames < job.completed_frames {
            return Err(ProjectError::ProgressRegressed {
                previous: job.completed_frames,
                next: completed_frames,
            });
        }
        job.status = status;
        job.completed_frames = completed_frames;
        Ok(())
    }

    pub fn fail(&mut self, id: &str, message: impl Into<String>) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued
                | JobStatus::Preparing
                | JobStatus::Rendering
                | JobStatus::Encoding
                | JobStatus::Failed
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Failed,
            });
        }
        job.status = JobStatus::Failed;
        job.error = Some(message.into());
        Ok(())
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), ProjectError> {
        let job = self
            .jobs
            .get_mut(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(
            job.status,
            JobStatus::Queued | JobStatus::Preparing | JobStatus::Rendering | JobStatus::Encoding
        ) {
            return Err(ProjectError::InvalidJobTransition {
                from: job.status.clone(),
                to: JobStatus::Cancelled,
            });
        }
        job.status = JobStatus::Cancelled;
        Ok(())
    }

    pub fn retry(&mut self, id: &str) -> Result<String, ProjectError> {
        let project = self
            .jobs
            .get(id)
            .ok_or_else(|| ProjectError::JobNotFound(id.to_string()))?;
        if !matches!(project.status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err(ProjectError::InvalidJobTransition {
                from: project.status.clone(),
                to: JobStatus::Queued,
            });
        }
        self.submit(project.project.clone())
    }
}
