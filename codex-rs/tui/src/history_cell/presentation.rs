use astral_tui::DisplayMode;

/// Presentation policy for one authoritative [`HistoryCell`](super::HistoryCell).
///
/// The cell owns which display modes have semantic content. Hosts only retain
/// the selected mode by stable history id; they do not infer foldability from
/// rendered text or concrete cell types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HistoryCellPresentation {
    default_mode: DisplayMode,
    fold_cycle: FoldCycle,
    groupable: bool,
}

impl HistoryCellPresentation {
    pub(crate) const fn fixed(default_mode: DisplayMode) -> Self {
        Self {
            default_mode,
            fold_cycle: FoldCycle::Fixed,
            groupable: false,
        }
    }

    pub(crate) const fn two_state(default_mode: DisplayMode) -> Self {
        Self {
            default_mode,
            fold_cycle: FoldCycle::CollapsedExpanded,
            groupable: false,
        }
    }

    pub(crate) const fn with_groupable(mut self) -> Self {
        self.groupable = true;
        self
    }

    pub(crate) const fn is_foldable(self) -> bool {
        !matches!(self.fold_cycle, FoldCycle::Fixed)
    }

    pub(crate) const fn is_groupable(self) -> bool {
        self.groupable
    }

    pub(crate) fn normalize(self, mode: Option<DisplayMode>) -> DisplayMode {
        match (self.fold_cycle, mode) {
            (FoldCycle::Fixed, _) => self.default_mode,
            (
                FoldCycle::CollapsedExpanded,
                Some(mode @ (DisplayMode::Collapsed | DisplayMode::Expanded)),
            ) => mode,
            (FoldCycle::CollapsedExpanded, None | Some(DisplayMode::Truncated)) => {
                self.default_mode
            }
        }
    }

    pub(crate) fn toggle(self, current: DisplayMode) -> Option<DisplayMode> {
        match self.fold_cycle {
            FoldCycle::Fixed => None,
            FoldCycle::CollapsedExpanded => Some(match current {
                DisplayMode::Collapsed => DisplayMode::Expanded,
                DisplayMode::Truncated | DisplayMode::Expanded => DisplayMode::Collapsed,
            }),
        }
    }

    pub(crate) fn collapse(self) -> Option<DisplayMode> {
        self.is_foldable().then_some(DisplayMode::Collapsed)
    }

    pub(crate) fn expand(self) -> Option<DisplayMode> {
        self.is_foldable().then_some(DisplayMode::Expanded)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FoldCycle {
    Fixed,
    CollapsedExpanded,
}
