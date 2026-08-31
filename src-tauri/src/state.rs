use crate::config::{AppConfig, ConfigStore};
use crate::media::MediaState;
use crate::page::PageCatalog;
use crate::registry::WidgetRegistry;
use crate::window_manager::WindowManager;
use std::sync::Mutex;
use tauri::menu::MenuItem;
use tauri::Wry;

pub struct AppState {
    pub store: Mutex<ConfigStore>,
    pub config: Mutex<AppConfig>,
    pub registry: Mutex<WidgetRegistry>,
    pub window_manager: Mutex<WindowManager>,
    pub media: MediaState,
    pub tray_desktop_item: Mutex<Option<MenuItem<Wry>>>,
    pub page_html: Mutex<String>,
    pub page_catalog: Mutex<PageCatalog>,
}
