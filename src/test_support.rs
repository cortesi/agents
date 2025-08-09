use std::env as std_env;
use std::ffi::OsStr;

pub struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvGuard {
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            prev: std_env::var(key).ok(),
        }
    }
    pub fn set<S: AsRef<OsStr>>(&self, val: S) {
        unsafe { std_env::set_var(self.key, val) };
    }
    pub fn unset(&self) {
        unsafe { std_env::remove_var(self.key) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => unsafe { std_env::set_var(self.key, v) },
            None => unsafe { std_env::remove_var(self.key) },
        }
    }
}
