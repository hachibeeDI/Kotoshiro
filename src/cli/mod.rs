pub mod check;
pub mod digest;
pub mod hook;
pub mod init;
pub mod link;
pub mod list;
pub mod new;
pub mod note;
pub mod show;
pub mod start;
pub mod status;

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use jiff::Timestamp;
use serde::Serialize;

use crate::store::digest::Digest;
use crate::store::graph;
use crate::store::paths::{dir_bytes, file_bytes};
use crate::store::todo::{Meta, Status, Todo};
use crate::store::Store;

pub struct Ctx {
    pub store: Store,
    pub json: bool,
    pub now: Timestamp,
}

impl Ctx {
    pub fn load(&self, id: u64) -> Result<Todo> {
        if !self.store.exists(id) {
            bail!("TODO #{id} not found");
        }
        Todo::load(&self.store.todo_md(id))
    }

    pub fn save(&self, todo: &Todo) -> Result<()> {
        todo.save(&self.store.todo_md(todo.meta.id))
    }

    pub fn status_map(&self) -> Result<HashMap<u64, Status>> {
        let mut m = HashMap::new();
        for id in self.store.ids()? {
            if let Ok(t) = self.load(id) {
                m.insert(id, t.meta.status);
            }
        }
        Ok(m)
    }

    pub fn blocked(&self, meta: &Meta, map: &HashMap<u64, Status>) -> bool {
        graph::is_blocked(&meta.blocked_by, &|id| map.get(&id).copied())
    }

    /// (digest, stale)
    pub fn digest_state(&self, id: u64) -> Result<(Option<Digest>, bool)> {
        let d = Digest::load(&self.store.digest(id))?;
        let stale = match &d {
            Some(d) => d.is_stale(
                file_bytes(&self.store.journal(id)),
                dir_bytes(&self.store.log_dir(id)),
            ),
            None => false,
        };
        Ok((d, stale))
    }

    pub fn print_json<T: Serialize>(&self, v: &T) -> Result<()> {
        let s = serde_json::to_string_pretty(v).context("serialize")?;
        println!("{s}");
        Ok(())
    }
}
