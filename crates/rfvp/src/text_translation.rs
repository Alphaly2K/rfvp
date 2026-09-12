//! Engine-neutral text translation request and replacement bookkeeping.
//!
//! Hosts can install an exact replacement table and receive asynchronous
//! translation requests. This module deliberately contains no FFI, Flutter,
//! network or GPU types so the engine can keep the translation boundary
//! independent from the eventual host ABI.

#[cfg(feature = "no_std")]
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
#[cfg(not(feature = "no_std"))]
use std::collections::BTreeMap;

/// A text slot that should be translated by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTranslationRequest {
    pub serial: u64,
    pub slot: u32,
    pub generation: u64,
    pub source: String,
    pub ruby: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingTextTranslation {
    slot: u32,
    generation: u64,
    translated: Option<String>,
}

/// State owned by `TextManager`.
///
/// The manager drains requests and submits results through this controller.
/// Pending requests are bound to both a text slot and its generation so stale
/// network responses cannot overwrite newer text.
#[derive(Debug, Default)]
pub(crate) struct TextTranslationController {
    replacements: BTreeMap<String, String>,
    online_enabled: bool,
    next_serial: u64,
    outgoing: Vec<TextTranslationRequest>,
    pending: BTreeMap<u64, PendingTextTranslation>,
}

impl TextTranslationController {
    pub(crate) fn set_replacements<I>(&mut self, replacements: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.replacements = replacements.into_iter().collect();
    }

    pub(crate) fn clear_replacements(&mut self) {
        self.replacements.clear();
    }

    pub(crate) fn set_online_enabled(&mut self, enabled: bool) {
        self.online_enabled = enabled;
        if !enabled {
            self.pending.clear();
        }
    }

    pub(crate) fn online_enabled(&self) -> bool {
        self.online_enabled
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Starts a new source revision for a slot.
    ///
    /// Returns a synchronous replacement when the exact source is present in
    /// the replacement table. Otherwise an online request is queued and
    /// `None` is returned so the caller renders the original text first.
    pub(crate) fn begin_source(
        &mut self,
        slot: u32,
        generation: u64,
        source: &str,
        ruby: Option<String>,
    ) -> Option<String> {
        self.invalidate_slot(slot);

        if let Some(replacement) = self.replacements.get(source) {
            return Some(replacement.clone());
        }

        if !self.online_enabled || source.trim().is_empty() {
            return None;
        }

        let serial = {
            self.next_serial = self.next_serial.wrapping_add(1);
            if self.next_serial == 0 {
                self.next_serial = 1;
            }
            self.next_serial
        };
        let request = TextTranslationRequest {
            serial,
            slot,
            generation,
            source: source.to_string(),
            ruby,
        };
        self.pending.insert(
            serial,
            PendingTextTranslation {
                slot,
                generation,
                translated: None,
            },
        );
        self.outgoing.push(request);
        None
    }

    pub(crate) fn drain_requests(&mut self) -> Vec<TextTranslationRequest> {
        core::mem::take(&mut self.outgoing)
    }

    /// Stores an asynchronous result.
    ///
    /// Returning `false` means the request was already invalidated by a newer
    /// text revision, a slot clear, or a disabled online translation mode.
    pub(crate) fn submit(&mut self, serial: u64, translated: Option<String>) -> bool {
        let Some(pending) = self.pending.get_mut(&serial) else {
            return false;
        };
        let Some(translated) = translated.filter(|text| !text.is_empty()) else {
            self.pending.remove(&serial);
            return true;
        };
        pending.translated = Some(translated);
        true
    }

    pub(crate) fn take_ready(&mut self, slot: u32, generation: u64) -> Option<String> {
        let serial = self.pending.iter().find_map(|(&serial, pending)| {
            (pending.slot == slot
                && pending.generation == generation
                && pending.translated.is_some())
            .then_some(serial)
        })?;
        self.pending
            .remove(&serial)
            .and_then(|pending| pending.translated)
    }

    pub(crate) fn invalidate_slot(&mut self, slot: u32) {
        self.pending.retain(|_, pending| pending.slot != slot);
    }

    pub(crate) fn clear(&mut self) {
        self.replacements.clear();
        self.online_enabled = false;
        self.outgoing.clear();
        self.pending.clear();
        self.next_serial = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_replacement_wins_and_does_not_queue_network_work() {
        let mut controller = TextTranslationController::default();
        controller.set_replacements([("原文".to_string(), "译文".to_string())]);
        controller.set_online_enabled(true);

        let replacement = controller.begin_source(1, 4, "原文", None);

        assert_eq!(replacement.as_deref(), Some("译文"));
        assert!(controller.drain_requests().is_empty());
        assert_eq!(controller.pending_count(), 0);
    }

    #[test]
    fn result_is_bound_to_slot_generation() {
        let mut controller = TextTranslationController::default();
        controller.set_online_enabled(true);
        assert_eq!(controller.begin_source(2, 10, "A", None), None);
        let first = controller.drain_requests().pop().unwrap();

        assert_eq!(controller.begin_source(2, 11, "B", None), None);
        let second = controller.drain_requests().pop().unwrap();

        assert!(!controller.submit(first.serial, Some("A translated".to_string())));
        assert!(controller.submit(second.serial, Some("B translated".to_string())));
        assert_eq!(controller.take_ready(2, 10), None);
        assert_eq!(
            controller.take_ready(2, 11).as_deref(),
            Some("B translated")
        );
    }

    #[test]
    fn clearing_a_slot_invalidates_pending_results_and_preserves_other_slots() {
        let mut controller = TextTranslationController::default();
        controller.set_online_enabled(true);
        controller.begin_source(3, 1, "A", None);
        controller.begin_source(4, 2, "B", None);
        let requests = controller.drain_requests();

        controller.invalidate_slot(3);
        assert!(!controller.submit(requests[0].serial, Some("A".to_string())));
        assert!(controller.submit(requests[1].serial, Some("B".to_string())));
        assert_eq!(controller.take_ready(4, 2).as_deref(), Some("B"));
    }
}
