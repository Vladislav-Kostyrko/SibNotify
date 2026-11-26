use crate::config::Config;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};

pub(crate) struct AppState {
    pub(crate) cfg: Mutex<Config>,
    pub(crate) sys: Mutex<System>,
    pub(crate) disks: Mutex<Disks>,
    pub(crate) nets: Mutex<Networks>,
    pub(crate) net_prev: Mutex<(u64, u64, Instant)>,
    pub(crate) chats: Mutex<std::collections::HashMap<i64, i64>>,
    pub(crate) users: Mutex<std::collections::HashMap<i64, String>>,
    pub(crate) blocked: AtomicBool,
    pub(crate) dragging: AtomicBool,
    pub(crate) last_toast: Mutex<serde_json::Value>,
}
