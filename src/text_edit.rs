//! Logical UTF-8 editing, independent of layout, windows and `Signal`.
//!
//! Positions are byte offsets at extended grapheme boundaries (UAX #29).
//! Forward/backward mean string order, not visual bidi cursor order. Word steps
//! skip adjacent Unicode whitespace, then consume one UAX #29 word-bound span,
//! including punctuation/emoji spans; Chinese uses Unicode boundaries, not a
//! dictionary. Targets are rounded outward to grapheme boundaries.
//!
//! Each effective user edit is an undo step unless explicitly enclosed in
//! [`TextEditModel::edit_group`]. There is no clock-based or implicit coalescing.

use std::{collections::VecDeque, fmt, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

/// A validated, directed selection in UTF-8 byte offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    anchor: usize,
    focus: usize,
}

impl Selection {
    /// The fixed endpoint when extending a selection.
    pub fn anchor(self) -> usize {
        self.anchor
    }

    /// The moving endpoint when extending a selection.
    pub fn focus(self) -> usize {
        self.focus
    }

    /// The ordered byte range, regardless of selection direction.
    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }

    /// Whether the selection has no selected text.
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    /// Whether focus precedes anchor in logical string order.
    pub fn is_backward(self) -> bool {
        self.focus < self.anchor
    }
}

/// An application-supplied byte offset was outside the text or inside a cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTextPosition {
    /// The rejected byte offset; the model is unchanged.
    pub offset: usize,
}

impl fmt::Display for InvalidTextPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "byte offset {} is not a grapheme boundary",
            self.offset
        )
    }
}

impl std::error::Error for InvalidTextPosition {}

/// Logical string order; visual bidi movement belongs to a future layout adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

/// The unit used for logical movement and deletion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditUnit {
    Grapheme,
    Word,
}

/// Combined undo/redo limits. Zero disables history.
///
/// `max_bytes` counts exact UTF-8 payload lengths of both snapshots per entry.
/// Entry metadata is bounded by `max_entries`. Current text and temporary edit
/// snapshots (including one outer group baseline) are not history-budgeted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryLimits {
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self {
            max_entries: 128,
            max_bytes: 1024 * 1024,
        }
    }
}

/// Retained history usage, excluding current text and temporary edit snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryStats {
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub text_bytes: usize,
}

#[derive(Clone, Debug)]
struct Snapshot {
    text: Box<str>,
    selection: Selection,
}

#[derive(Debug)]
struct Entry {
    before: Snapshot,
    after: Snapshot,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.before.text.len().saturating_add(self.after.text.len())
    }
}

/// A small logical edit model; no rich text, rendering, native IME or GUI control.
///
/// Line breaks are preserved as text. User edits, undo, redo and differing
/// programmatic replacements advance a text-only revision (saturating at
/// `u64::MAX`). Selection-only changes do not. All supplied offsets are checked;
/// selection offsets remain valid even when insertion joins adjacent clusters.
///
/// ```
/// use chronobreak::{Direction, EditUnit, TextEditModel};
///
/// let mut model = TextEditModel::new("你好 👩🏽‍💻");
/// model.move_cursor(Direction::Backward, EditUnit::Grapheme, true);
/// assert_eq!(model.selected_text(), "👩🏽‍💻");
/// model.edit_group(|model| {
///     model.replace_selection("🇨🇳");
///     model.replace_selection("!");
/// });
/// assert_eq!(model.text(), "你好 🇨🇳!");
/// assert!(model.undo());
/// assert_eq!(model.selected_text(), "👩🏽‍💻");
/// ```
#[derive(Debug)]
pub struct TextEditModel {
    text: String,
    selection: Selection,
    revision: u64,
    limits: HistoryLimits,
    undo: VecDeque<Entry>,
    redo: VecDeque<Entry>,
    group_depth: usize,
    group_before: Option<Snapshot>,
}

impl TextEditModel {
    /// Start with a collapsed selection at the end, revision zero, empty history.
    pub fn new(text: impl Into<String>) -> Self {
        Self::with_history_limits(text, HistoryLimits::default())
    }

    /// Create a model with deterministic history budgets.
    pub fn with_history_limits(text: impl Into<String>, limits: HistoryLimits) -> Self {
        let text = text.into();
        let end = text.len();
        Self {
            text,
            selection: Selection {
                anchor: end,
                focus: end,
            },
            revision: 0,
            limits,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            group_depth: 0,
            group_before: None,
        }
    }

    /// Borrow the current UTF-8 text without exposing mutable storage.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Read the current directed, grapheme-safe selection.
    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// The selected substring in logical order, including for backward selections.
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.range()]
    }

    /// Text-only revision; selection and history eviction do not advance it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Reject invalid endpoints atomically, including UTF-8 and cluster interiors.
    pub fn set_selection(
        &mut self,
        anchor: usize,
        focus: usize,
    ) -> Result<(), InvalidTextPosition> {
        for offset in [anchor, focus] {
            if !self.is_boundary(offset) {
                return Err(InvalidTextPosition { offset });
            }
        }
        self.selection = Selection { anchor, focus };
        Ok(())
    }

    /// Select the entire string with forward direction; no history/revision change.
    pub fn select_all(&mut self) {
        self.selection = Selection {
            anchor: 0,
            focus: self.text.len(),
        };
    }

    /// Swap anchor and focus without changing selected text or history.
    pub fn reverse_selection(&mut self) {
        std::mem::swap(&mut self.selection.anchor, &mut self.selection.focus);
    }

    /// Move focus, optionally keeping anchor fixed. Without extension a nonempty
    /// selection collapses to the directional edge, without an additional step.
    pub fn move_cursor(&mut self, direction: Direction, unit: EditUnit, extend: bool) {
        let focus = if !extend && !self.selection.is_collapsed() {
            match direction {
                Direction::Backward => self.selection.range().start,
                Direction::Forward => self.selection.range().end,
            }
        } else {
            self.destination(self.selection.focus, direction, unit)
        };
        self.selection.focus = focus;
        if !extend {
            self.selection.anchor = focus;
        }
    }

    /// Insert at the caret or replace the selected range as a user edit.
    ///
    /// The caret collapses at the splice end, rounded forward if newly joined
    /// clusters absorb adjacent text. Identical replacement may collapse selection
    /// but does not change revision/history or invalidate redo. Returns text change.
    pub fn replace_selection(&mut self, inserted: &str) -> bool {
        self.replace_range(self.selection.range(), inserted)
    }

    fn replace_range(&mut self, range: Range<usize>, inserted: &str) -> bool {
        if &self.text[range.clone()] == inserted {
            self.selection = Selection {
                anchor: range.end,
                focus: range.end,
            };
            return false;
        }
        let before = if self.group_depth == 0 || self.group_before.is_none() {
            Some(self.snapshot())
        } else {
            None
        };
        self.text.replace_range(range.clone(), inserted);
        let caret = self.ceil_boundary(range.start + inserted.len());
        self.selection = Selection {
            anchor: caret,
            focus: caret,
        };
        self.advance_revision();
        if self.group_depth == 0 {
            if let Some(before) = before {
                self.record(before);
            }
        } else if self.group_before.is_none() {
            self.group_before = before;
        }
        true
    }

    /// Delete selection, or one directional unit. Whitespace/word policy is the
    /// same as movement. Boundary no-ops leave history, selection and redo intact.
    pub fn delete(&mut self, direction: Direction, unit: EditUnit) -> bool {
        if !self.selection.is_collapsed() {
            return self.replace_selection("");
        }
        let focus = self.destination(self.selection.focus, direction, unit);
        if focus == self.selection.focus {
            return false;
        }
        self.replace_range(
            focus.min(self.selection.focus)..focus.max(self.selection.focus),
            "",
        )
    }

    /// Coalesce nested explicit groups into one undo step. The before-selection
    /// is captured at the first effective edit, after-selection at outer exit.
    /// Net-identical text creates no entry and preserves redo. Undo/redo return
    /// false inside a group. Panic retains edits and finalizes the group on unwind;
    /// this is coalescing, not rollback. There is no implicit typing coalescing.
    pub fn edit_group<Output>(&mut self, edit: impl FnOnce(&mut Self) -> Output) -> Output {
        self.group_depth += 1;
        let guard = EditGroup(self);
        edit(guard.0)
    }

    /// External authoritative replacement, not a user edit.
    ///
    /// Equal text is a complete no-op (including selection/history/revision), so
    /// an adapter can ignore its own echoed value. Differing text clears undo,
    /// redo and any pending group baseline, advances revision once, and preserves
    /// endpoint byte offsets clamped backward to valid boundaries in the new text.
    /// Inside a group this is a barrier; only subsequent user edits can be undone.
    /// A future Signal adapter must publish user text outside formatting closures,
    /// suppress equal echoes, and apply external changes through this method.
    pub fn replace_programmatic(&mut self, text: impl Into<String>) -> bool {
        let text = text.into();
        if self.text == text {
            return false;
        }
        self.text = text;
        self.selection.anchor = self.floor_boundary(self.selection.anchor.min(self.text.len()));
        self.selection.focus = self.floor_boundary(self.selection.focus.min(self.text.len()));
        self.undo.clear();
        self.redo.clear();
        self.group_before = None;
        self.advance_revision();
        true
    }

    /// Whether undo is available now; false while an explicit group is active.
    pub fn can_undo(&self) -> bool {
        self.group_depth == 0 && !self.undo.is_empty()
    }

    /// Whether redo is available now; false while an explicit group is active.
    pub fn can_redo(&self) -> bool {
        self.group_depth == 0 && !self.redo.is_empty()
    }

    /// Restore text and directed selection; revision advances rather than rewinds.
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        if let Some(entry) = self.undo.pop_back() {
            self.restore(&entry.before);
            self.redo.push_back(entry);
        }
        true
    }

    /// Reapply the last undone step, including its directed selection.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        if let Some(entry) = self.redo.pop_back() {
            self.restore(&entry.after);
            self.undo.push_back(entry);
        }
        true
    }

    /// Combined retained history usage; undo/redo transfer entries without copies.
    pub fn history_stats(&self) -> HistoryStats {
        HistoryStats {
            undo_entries: self.undo.len(),
            redo_entries: self.redo.len(),
            text_bytes: self
                .undo
                .iter()
                .chain(&self.redo)
                .fold(0usize, |bytes, entry| bytes.saturating_add(entry.bytes())),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone().into_boxed_str(),
            selection: self.selection,
        }
    }

    fn restore(&mut self, snapshot: &Snapshot) {
        self.text = snapshot.text.to_string();
        self.selection = snapshot.selection;
        self.advance_revision();
    }

    fn advance_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn record(&mut self, before: Snapshot) {
        if before.text.as_ref() == self.text {
            return;
        }
        self.redo.clear();
        self.undo.push_back(Entry {
            before,
            after: self.snapshot(),
        });
        loop {
            let usage = self.history_stats();
            if usage.undo_entries <= self.limits.max_entries
                && usage.text_bytes <= self.limits.max_bytes
            {
                break;
            }
            self.undo.pop_front();
        }
    }

    fn is_boundary(&self, offset: usize) -> bool {
        offset == self.text.len()
            || self
                .text
                .grapheme_indices(true)
                .any(|(start, _)| start == offset)
    }

    fn floor_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(start, _)| start)
            .chain(std::iter::once(self.text.len()))
            .take_while(|&start| start <= offset)
            .last()
            .unwrap_or(0)
    }

    fn ceil_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(start, _)| start)
            .find(|&start| start >= offset)
            .unwrap_or(self.text.len())
    }

    fn destination(&self, offset: usize, direction: Direction, unit: EditUnit) -> usize {
        match (direction, unit) {
            (Direction::Backward, EditUnit::Grapheme) => self
                .text
                .grapheme_indices(true)
                .map(|(start, _)| start)
                .take_while(|&start| start < offset)
                .last()
                .unwrap_or(0),
            (Direction::Forward, EditUnit::Grapheme) => self
                .text
                .grapheme_indices(true)
                .map(|(start, _)| start)
                .find(|&start| start > offset)
                .unwrap_or(self.text.len()),
            (Direction::Backward, EditUnit::Word) => {
                let target = self
                    .text
                    .split_word_bound_indices()
                    .rev()
                    .find(|(start, span)| *start < offset && !span.chars().all(char::is_whitespace))
                    .map_or(0, |(start, _)| start);
                self.floor_boundary(target)
            }
            (Direction::Forward, EditUnit::Word) => {
                let target = self
                    .text
                    .split_word_bound_indices()
                    .find(|(start, span)| {
                        start + span.len() > offset && !span.chars().all(char::is_whitespace)
                    })
                    .map_or(self.text.len(), |(start, span)| start + span.len());
                self.ceil_boundary(target)
            }
        }
    }
}

struct EditGroup<'a>(&'a mut TextEditModel);

impl Drop for EditGroup<'_> {
    fn drop(&mut self) {
        self.0.group_depth -= 1;
        if self.0.group_depth == 0
            && let Some(before) = self.0.group_before.take()
        {
            self.0.record(before);
        }
    }
}

#[cfg(test)]
#[path = "text_edit_tests.rs"]
mod tests;
