//! Transport flags are metadata, not MIDI note-lifetime events.

/// Stop and jumps during continuous playback must release the old phrase.
/// Starting playback alone must NOT kill notes: hosts may deliver the first
/// NoteOn before the block that first reports `playing = true`. A stopped
/// position is not a prediction of the first playing block's position either.
/// Explicit host reset, MIDI panic and GUI panic are handled independently.
pub(super) fn requires_reset(
    previous_playing: Option<bool>,
    expected_position: Option<i64>,
    playing: bool,
    position: Option<i64>,
) -> bool {
    previous_playing == Some(true)
        && (!playing || expected_position.zip(position).is_some_and(|(a, b)| a != b))
}

#[cfg(test)]
mod tests {
    use super::requires_reset;

    #[test]
    fn start_is_not_a_note_reset_even_with_changed_position() {
        assert!(!requires_reset(None, None, true, Some(0)));
        assert!(!requires_reset(Some(false), Some(0), true, Some(64)));
        assert!(!requires_reset(Some(false), Some(1000), true, Some(0)));
        assert!(!requires_reset(Some(false), None, true, None));
    }

    #[test]
    fn stop_seek_and_loop_still_reset() {
        assert!(requires_reset(Some(true), Some(64), false, Some(64)));
        assert!(requires_reset(Some(true), None, false, None));
        assert!(requires_reset(Some(true), Some(64), true, Some(1024)));
        assert!(requires_reset(Some(true), Some(1024), true, Some(0)));
        assert!(!requires_reset(Some(true), Some(64), true, Some(64)));
        assert!(!requires_reset(Some(true), None, true, None));
        assert!(!requires_reset(Some(false), Some(64), false, Some(0)));
    }
}
