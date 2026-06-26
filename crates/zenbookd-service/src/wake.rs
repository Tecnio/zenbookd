use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

#[derive(Default)]
pub struct Wake {
    generation: Mutex<u64>,
    condvar: Condvar,
}

impl Wake {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notify(&self) {
        let mut generation = self.generation.lock().unwrap();

        *generation = generation.wrapping_add(1);

        self.condvar.notify_all();
    }

    pub fn wait_timeout(&self, last_seen: &mut u64, timeout: Duration) {
        let generation = self.generation.lock().unwrap();

        if *generation != *last_seen {
            *last_seen = *generation;
            return;
        }

        let (generation, _) = self.condvar.wait_timeout(generation, timeout).unwrap();

        *last_seen = *generation;
    }
}
