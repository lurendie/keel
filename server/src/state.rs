use rusqlite::Connection;
use std::{path::PathBuf, sync::{Arc, Mutex}};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub deploy_lock: Arc<Mutex<()>>,
    pub jwt_secret: Arc<String>,
    pub git_path: Arc<PathBuf>,
}
