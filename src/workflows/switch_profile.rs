use std::sync::{Arc, Mutex};

use crate::services::{
    ServiceResult, game::GameService, process::ProcessService, profile::ProfileService,
    repo::RepoService, save::SaveService,
};

pub struct SwitchProfile;

#[allow(unused_variables)]
impl SwitchProfile {
    /// operations — общий mutex операций с репозиторием и сохранениями.
    pub fn new(
        games: Arc<GameService>,
        profiles: Arc<ProfileService>,
        saves: Arc<SaveService>,
        repo: Arc<RepoService>,
        processes: Arc<ProcessService>,
        operations: Arc<Mutex<()>>,
    ) -> Self {
        todo!()
    }

    /// Захватить operations на весь сценарий; затем перепроверить профиль и процессы.
    /// Не захватывать operations повторно и не держать локи хранилищ во время I/O.
    pub fn execute(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }
}
