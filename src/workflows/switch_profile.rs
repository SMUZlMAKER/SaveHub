use std::sync::{Arc, Mutex};

use crate::services::{
    ServiceResult, game::GameService, process::ProcessService, profile::ProfileService,
    repo::RepoService, save::SaveService,
};
use crate::domain::ProfileId;

pub struct SwitchProfile {
    games: Arc<GameService>,
    profiles: Arc<ProfileService>,
    saves: Arc<SaveService>,
    repo: Arc<RepoService>,
    processes: Arc<ProcessService>,
    operations: Arc<Mutex<()>>,
}

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
        Self {
            games,
            profiles,
            saves,
            repo,
            processes,
            operations,
        }
    }

    /// Захватить operations на весь сценарий; затем перепроверить профиль и процессы.
    /// Не захватывать operations повторно и не держать локи хранилищ во время I/O.
    pub fn execute(&self, profile_id: &ProfileId) -> ServiceResult<()> {
        let _guard = self.operations.lock().map_err(|_| {
            crate::services::ServiceError::Conflict("Операция уже выполняется".into())
        })?;
        let profile = self.profiles.get(profile_id)?;
        self.processes.refresh_running_games()?;
        if let Some(games) = self.processes.running_games() {
            if !games.is_empty() {
                return Err(crate::services::ServiceError::GameRunning(games));
            }
        }
        self.repo.fetch()?;
        for game in self.games.list()? {
            self.saves.create_save(
                game.id,
                &profile_id.to_string(),// TODO заменить на текущий профиль, вместо нового
                crate::domain::SaveKind::BeforeSwitch,
            )?;
            self.saves.bind(game.id, &profile_id.to_string())?;
        }
        self.repo.checkout_profile(&profile.branch)?;
        self.profiles.set_active(profile_id)
    }
}
