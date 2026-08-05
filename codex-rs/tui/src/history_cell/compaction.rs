//! Typed history cells for context-compaction outcomes.

use super::*;

#[derive(Debug)]
pub(crate) struct CompactionCompletedCell {
    elapsed_ms: Option<u64>,
}

pub(crate) fn new_compaction_completed(elapsed_ms: Option<u64>) -> CompactionCompletedCell {
    CompactionCompletedCell { elapsed_ms }
}

impl HistoryCell for CompactionCompletedCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let line = Line::from(self.message()).dim();
        adaptive_wrap_lines([line], RtOptions::new(width.max(1) as usize))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        vec![Line::from(self.message())]
    }
}

impl CompactionCompletedCell {
    fn message(&self) -> String {
        match self.elapsed_ms {
            Some(elapsed_ms) => {
                format!(
                    "Compaction completed in {}.",
                    format_duration_ms(elapsed_ms)
                )
            }
            None => "Compaction completed.".to_string(),
        }
    }
}
