use rusqlite::Connection;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

use crate::{auth::LoginThrottle, net::TrustedProxies};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub deploy_semaphore: Arc<Semaphore>,
    pub project_locks: Arc<Mutex<HashMap<i64, Arc<AsyncMutex<()>>>>>,
    pub jwt_secret: Arc<String>,
    pub trusted_proxies: Arc<TrustedProxies>,
    pub login_throttle: Arc<LoginThrottle>,
    pub git_path: Arc<PathBuf>,
    pub workspace_root: Arc<PathBuf>,
}
