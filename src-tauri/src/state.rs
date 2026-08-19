use crate::config::{AppConfig, ConfigStore};
use crate::media::MediaState;
use crate::registry::WidgetRegistry;
use crate::window_manager::WindowManager;
use std::sync::Mutex;

pub struct AppState {
    pub store: Mutex<ConfigStore>,
    pub config: Mutex<AppConfig>,
    pub registry: Mutex<WidgetRegistry>,
    pub window_manager: Mutex<WindowManager>,
    pub media: MediaState,
}
