use super::ServiceResult;
use crate::domain::*;
use std::sync::Arc;

pub struct ProfileService;

#[allow(unused_variables)]
impl ProfileService {
    pub fn new(catalog: Arc<Catalog>, runtime: Arc<RuntimeState>, settings: Arc<Settings>) -> Self {
        todo!()
    }

    pub fn list(&self) -> ServiceResult<Vec<Profile>> {
        todo!()
    }

    pub fn get(&self, profile_id: &str) -> ServiceResult<Profile> {
        todo!()
    }

    pub fn active(&self) -> ServiceResult<Option<Profile>> {
        todo!()
    }

    /// Создать профиль и его ветку; каталог игр общий.
    pub fn create(&self, name: &str) -> ServiceResult<Profile> {
        todo!()
    }

    /// Переименовать без изменения идентификатора.
    pub fn rename(&self, profile_id: &str, name: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Запретить удаление активного профиля.
    pub fn delete(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }

    /// Фиксация состояния для SwitchProfile после успешной смены сохранений.
    pub fn set_active(&self, profile_id: &str) -> ServiceResult<()> {
        todo!()
    }
}
