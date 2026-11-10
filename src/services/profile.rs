use super::{ServiceError, ServiceResult};
use crate::domain::*;
use std::sync::Arc;

pub struct ProfileService {
    catalog: Arc<Catalog>,
    runtime: Arc<RuntimeState>,
    _settings: Arc<Settings>,
}

#[allow(unused_variables)]
impl ProfileService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        Self {
            catalog,
            runtime,
            _settings: settings,
        }
    }

    pub fn list(&self) -> ServiceResult<Vec<Profile>> {
        Ok(self.catalog.profiles().into_values().collect())
    }

    pub fn get(&self, profile_id: &str) -> ServiceResult<Profile> {
        self.catalog
            .profiles()
            .remove(profile_id)
            .ok_or_else(|| ServiceError::NotFound(format!("Профиль не найден: {profile_id}")))
    }

    pub fn active(&self) -> ServiceResult<Option<Profile>> {
        self.runtime
            .active_profile()
            .map(|id| self.get(&id))
            .transpose()
    }

    /// Создать профиль и его ветку; каталог игр общий.
    pub fn create(&self, name: &str) -> ServiceResult<Profile> {
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Имя профиля не может быть пустым".into(),
            ));
        }
        let name = name.trim().to_owned();
        let profile = Profile {
            id: crate::utils::utils::slugify(&name),
            branch: crate::utils::utils::name_to_branch(&name),
            name,
            last_played_game: None,
        };
        if self.catalog.profiles().contains_key(&profile.id) {
            return Err(ServiceError::InvalidInput(format!(
                "Профиль уже существует: {}",
                profile.name
            )));
        }
        self.catalog.add_profile(profile.clone());
        Ok(profile)
    }

    /// Переименовать без изменения идентификатора.
    pub fn rename(&self, profile_id: &str, name: &str) -> ServiceResult<()> {
        let mut profile = self.get(profile_id)?;
        if name.trim().is_empty() {
            return Err(ServiceError::InvalidInput(
                "Имя профиля не может быть пустым".into(),
            ));
        }
        profile.name = name.trim().to_owned();
        if !self.catalog.update_profile(profile) {
            return Err(ServiceError::NotFound("Профиль не найден".into()));
        }
        Ok(())
    }

    /// Запретить удаление активного профиля.
    pub fn delete(&self, profile_id: &str) -> ServiceResult<()> {
        if self.runtime.active_profile().as_deref() == Some(profile_id) {
            return Err(ServiceError::InvalidInput(
                "Активный профиль удалить нельзя".into(),
            ));
        }
        if self.catalog.remove_profile(profile_id) {
            Ok(())
        } else {
            Err(ServiceError::NotFound("Профиль не найден".into()))
        }
    }

    /// Зафиксировать активный профиль после успешной смены сохранений.
    pub fn set_active(&self, profile_id: &str) -> ServiceResult<()> {
        if !self.catalog.profiles().contains_key(profile_id) {
            return Err(ServiceError::NotFound("Профиль не найден".into()));
        }
        self.runtime
            .set_active_profile(Some(profile_id.to_owned()))
            .map_err(|error| match error {
                ProfileActivationError::ProcessesUnknown => ServiceError::ProcessesUnknown,
                ProfileActivationError::GamesRunning(games) => ServiceError::GameRunning(games),
            })
    }
}
