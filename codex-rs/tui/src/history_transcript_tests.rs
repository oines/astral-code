use std::sync::Arc;

use pretty_assertions::assert_eq;
use ratatui::text::Line;

use super::HistoryEntryId;
use super::HistoryTranscript;
use crate::history_cell::HistoryCell;
use crate::history_cell::PlainHistoryCell;

fn cell(text: &str) -> Arc<dyn HistoryCell> {
    Arc::new(PlainHistoryCell::new(vec![Line::from(text.to_string())]))
}

fn id_at(transcript: &HistoryTranscript, index: usize) -> HistoryEntryId {
    transcript
        .entries()
        .nth(index)
        .map(|(id, _)| id)
        .expect("entry id")
}

#[test]
fn consolidation_retains_first_identity_and_surrounding_ids() {
    let mut transcript: HistoryTranscript = ["before", "part 1", "part 2", "after"]
        .into_iter()
        .map(cell)
        .collect();
    let before = id_at(&transcript, 0);
    let first_part = id_at(&transcript, 1);
    let after = id_at(&transcript, 3);

    let consolidated = transcript.consolidate(1..3, cell("final"));

    assert_eq!(consolidated, first_part);
    assert_eq!(
        transcript.entries().map(|(id, _)| id).collect::<Vec<_>>(),
        vec![before, first_part, after]
    );
}

#[test]
fn structural_mutations_keep_cells_and_ids_aligned() {
    let mut transcript: HistoryTranscript = ["one", "two", "three"].into_iter().map(cell).collect();

    transcript.remove(1);
    transcript.truncate(1);
    let retained = id_at(&transcript, 0);
    let appended = transcript.push(cell("four"));

    assert_eq!(transcript.len(), 2);
    assert_eq!(
        transcript.entries().map(|(id, _)| id).collect::<Vec<_>>(),
        vec![retained, appended]
    );
    assert!(appended.value() > retained.value());
}

#[test]
fn clear_does_not_reuse_presentation_identity() {
    let mut transcript: HistoryTranscript = ["old"].into_iter().map(cell).collect();
    let old = id_at(&transcript, 0);

    transcript.clear();
    let new = transcript.push(cell("new"));

    assert!(new.value() > old.value());
}
