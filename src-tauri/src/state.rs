use crate::config::{AppConfig, ConfigStore};
use crate::media::MediaState;
use crate::registry::WidgetRegistry;
use std::sync::Mutex;

pub struct AppState {
    pub store: Mutex<ConfigStore>,
    pub config: Mutex<AppConfig>,
    pub registry: Mutex<WidgetRegistry>,
    pub media: MediaState,
}
