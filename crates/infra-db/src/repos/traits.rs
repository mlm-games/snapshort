use crate::DbResult;
use snapshort_domain::prelude::*;
use std::future::Future;

pub trait ProjectRepository: Send + Sync {
    fn create(&self, project: &Project) -> impl Future<Output = DbResult<()>> + Send;
    fn get(&self, id: ProjectId) -> impl Future<Output = DbResult<Option<Project>>> + Send;
    fn get_all(&self) -> impl Future<Output = DbResult<Vec<Project>>> + Send;
    fn update(&self, project: &Project) -> impl Future<Output = DbResult<()>> + Send;
    fn delete(&self, id: ProjectId) -> impl Future<Output = DbResult<()>> + Send;
}

pub trait AssetRepository: Send + Sync {
    fn create(
        &self,
        project_id: ProjectId,
        asset: &Asset,
    ) -> impl Future<Output = DbResult<()>> + Send;
    fn get(&self, id: AssetId) -> impl Future<Output = DbResult<Option<Asset>>> + Send;
    fn get_by_project(
        &self,
        project_id: ProjectId,
    ) -> impl Future<Output = DbResult<Vec<Asset>>> + Send;
    fn update(&self, asset: &Asset) -> impl Future<Output = DbResult<()>> + Send;
    fn delete(&self, id: AssetId) -> impl Future<Output = DbResult<()>> + Send;
    fn update_status(
        &self,
        id: AssetId,
        status: AssetStatus,
    ) -> impl Future<Output = DbResult<()>> + Send;
}

pub trait TimelineRepository: Send + Sync {
    fn create(
        &self,
        project_id: ProjectId,
        timeline: &Timeline,
    ) -> impl Future<Output = DbResult<()>> + Send;
    fn get(&self, id: TimelineId) -> impl Future<Output = DbResult<Option<Timeline>>> + Send;
    fn get_by_project(
        &self,
        project_id: ProjectId,
    ) -> impl Future<Output = DbResult<Vec<Timeline>>> + Send;
    fn update(&self, timeline: &Timeline) -> impl Future<Output = DbResult<()>> + Send;
    fn delete(&self, id: TimelineId) -> impl Future<Output = DbResult<()>> + Send;
}

pub trait JobRepository: Send + Sync {
    fn create(
        &self,
        job: &crate::repos::job_repo::JobRow,
    ) -> impl Future<Output = DbResult<()>> + Send;
    fn get(
        &self,
        id: JobId,
    ) -> impl Future<Output = DbResult<Option<crate::repos::job_repo::JobRow>>> + Send;
    fn list_by_status(
        &self,
        status: JobStatus,
    ) -> impl Future<Output = DbResult<Vec<crate::repos::job_repo::JobRow>>> + Send;
    fn update_status(
        &self,
        id: JobId,
        status: JobStatus,
        progress: Option<u8>,
        error: Option<String>,
        result_json: Option<String>,
    ) -> impl Future<Output = DbResult<()>> + Send;

    fn requeue_running(&self) -> impl Future<Output = DbResult<u64>> + Send;
}
