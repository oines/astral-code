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

fn entry_snapshot(transcript: &HistoryTranscript) -> Vec<(HistoryEntryId, String)> {
    transcript
        .entries()
        .map(|(id, cell)| {
            let text = cell
                .display_lines(/*width*/ 80)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            (id, text)
        })
        .collect()
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
        entry_snapshot(&transcript),
        vec![
            (before, "before".to_string()),
            (first_part, "final".to_string()),
            (after, "after".to_string()),
        ]
    );
}

#[test]
fn structural_mutations_keep_cells_and_ids_aligned() {
    let mut transcript: HistoryTranscript = ["one", "two", "three"].into_iter().map(cell).collect();
    let original = entry_snapshot(&transcript);

    let removed = transcript.remove(1);
    assert_eq!(removed.display_lines(/*width*/ 80), vec![Line::from("two")]);
    assert_eq!(
        entry_snapshot(&transcript),
        vec![original[0].clone(), original[2].clone()]
    );

    transcript.truncate(1);
    assert_eq!(entry_snapshot(&transcript), vec![original[0].clone()]);

    let appended = transcript.push(cell("four"));

    assert_eq!(
        entry_snapshot(&transcript),
        vec![original[0].clone(), (appended, "four".to_string())]
    );
    assert!(!original.iter().any(|(id, _)| *id == appended));
}

#[test]
fn clear_does_not_reuse_presentation_identity() {
    let mut transcript: HistoryTranscript = ["old"].into_iter().map(cell).collect();
    let old = id_at(&transcript, 0);

    transcript.clear();
    let new = transcript.push(cell("new"));

    assert_ne!(new, old);
}
