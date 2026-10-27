//! The content owners a host publishes values for (LLP 1123 D9), and the
//! environment each node's rows resolve against.

use super::NodeArena;
use crate::style::Env;

impl NodeArena {
    /// Each owner's environment onto a new document environment, keeping
    /// the owner's values.
    pub(super) fn rebase_owners(&mut self, env: &Env) {
        for (_, owned) in &mut self.owners {
            *owned = Env {
                owner: owned.owner.take(),
                ..env.clone()
            };
        }
    }

    /// Whether a host published values for any content owner.
    pub(crate) fn has_owners(&self) -> bool {
        !self.owners.is_empty()
    }

    /// The environment `slot`'s rows resolve against: the document's,
    /// carrying the values of the nearest content owner at or above it
    /// (LLP 1123 D9), or none outside every owner. A top-layer box, a
    /// popover or a `dialog`, is the document's, so one written in an owner
    /// reads the document's values; its container units read the owner it
    /// is written in (D16, `owner::container_at`).
    pub fn owner_env(&self, slot: u32) -> &Env {
        let mut at = Some(slot).filter(|_| !self.owners.is_empty());
        while let Some(s) = at {
            if let Some((_, env)) = self.owners.iter().find(|(o, _)| *o == s) {
                return env;
            }
            // A `dialog` too: Exact shows one only modally, in the top layer
            if self.top_layer_slots.contains(s)
                || self.props(s).str(crate::PropId::SemanticTag) == Some("dialog")
            {
                break;
            }
            at = self.parent(s);
        }
        &self.env
    }

    /// The values a host published for the owner rooted at `slot`.
    pub fn owner_values(&self, slot: u32) -> Option<&crate::style::OwnerValues> {
        self.owners
            .iter()
            .find(|(s, _)| *s == slot)
            .and_then(|(_, env)| env.owner.as_deref())
    }

    /// Publish (or, `None`, take back) the values of the owner rooted at
    /// `slot`; whether they changed.
    pub(crate) fn set_owner(
        &mut self,
        slot: u32,
        values: Option<crate::style::OwnerValues>,
    ) -> bool {
        if self.owner_values(slot) == values.as_ref() {
            return false;
        }
        self.owners.retain(|(s, _)| *s != slot);
        if let Some(v) = values {
            let env = Env {
                owner: Some(std::sync::Arc::new(v)),
                ..self.env.clone()
            };
            self.owners.push((slot, env));
        }
        true
    }
}
