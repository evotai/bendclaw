/// Atomic edits to the favorites set. Repository implementations apply these
/// under one lock, never from a caller-owned stale snapshot.
pub enum FavoritesEdit {
    Replace(Vec<String>),
    Toggle(String),
    Remove(Vec<String>),
}

impl FavoritesEdit {
    pub fn apply(self, ids: &mut Vec<String>) -> bool {
        let before = ids.clone();
        match self {
            Self::Replace(next) => *ids = next,
            Self::Toggle(id) => {
                if let Some(index) = ids.iter().position(|existing| existing == &id) {
                    ids.remove(index);
                } else {
                    ids.push(id);
                }
            }
            Self::Remove(removed) => ids.retain(|id| !removed.contains(id)),
        }
        *ids != before
    }
}

pub struct FavoritesUpdate {
    pub ids: Vec<String>,
    pub removed: usize,
}
