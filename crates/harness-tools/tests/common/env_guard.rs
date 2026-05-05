pub struct EnvGuard {
    previous: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    pub fn set(entries: &[(&'static str, Option<&str>)]) -> Self {
        let previous = entries
            .iter()
            .map(|(key, value)| {
                let previous = std::env::var(key).ok();
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
                (*key, previous)
            })
            .collect();
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
