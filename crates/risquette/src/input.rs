//! A text input for GPUI: single-line or multi-line (with soft wrapping),
//! optional password masking, IME support, selection, clipboard and undo.
//!
//! Adapted from GPUI's `examples/input.rs` (Apache-2.0, Zed Industries).

use gpui::{
    actions, div, fill, point, prelude::*, px, size, App, Bounds, ClipboardItem, ContentMask,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
};
use risquette_theme::ActiveTheme;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    text_input,
    [
        Backspace,
        BackspaceWord,
        Delete,
        DeleteWord,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        Enter,
        Submit,
        Paste,
        Cut,
        Copy,
        Undo,
    ]
);

const CONTEXT: &str = "TextInput";
const MASK_CHAR: char = '•';

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("ctrl-backspace", BackspaceWord, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("ctrl-delete", DeleteWord, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("ctrl-home", DocStart, c),
        KeyBinding::new("ctrl-end", DocEnd, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("ctrl-enter", Submit, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-insert", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-z", Undo, c),
        // macOS equivalents
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-enter", Submit, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// The text changed.
    Changed,
    /// Enter (single-line) or Ctrl/Cmd+Enter (multi-line).
    Submit,
}

struct LaidOutLine {
    line: WrappedLine,
    /// Byte offset of this logical line in the display text.
    start: usize,
    /// Top of this line relative to the element.
    y: Pixels,
    /// Byte offsets (relative to `start`) where each visual row begins.
    row_starts: Vec<usize>,
}

struct Layout {
    lines: Vec<LaidOutLine>,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    multiline: bool,
    masked: bool,
    disabled: bool,
    /// Visible rows for multi-line inputs (grows beyond this with content).
    min_rows: usize,
    max_len: Option<usize>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    scroll_x: Pixels,
    layout: Option<Layout>,
    undo: Vec<(String, Range<usize>)>,
    /// The column a run of Up/Down presses is aiming for.
    goal_x: Option<Pixels>,
    /// Optional icon shown before the text (e.g. a magnifier for search boxes).
    icon: Option<crate::icons::Icon>,
}

impl EventEmitter<InputEvent> for TextInput {}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: SharedString::default(),
            multiline: false,
            masked: false,
            disabled: false,
            min_rows: 4,
            max_len: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
            scroll_x: px(0.),
            layout: None,
            undo: Vec::new(),
            goal_x: None,
            icon: None,
        }
    }

    pub fn icon(mut self, icon: crate::icons::Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn multiline(mut self, min_rows: usize) -> Self {
        self.multiline = true;
        self.min_rows = min_rows.max(1);
        self
    }

    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn max_len(mut self, n: usize) -> Self {
        self.max_len = Some(n);
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.marked_range = None;
        self.undo.clear();
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    /// Number of characters (for counters).
    pub fn char_count(&self) -> usize {
        self.content.chars().count()
    }

    /// Wrap the selection in `before`/`after` (e.g. `**bold**`), or insert
    /// `before placeholder after` with the placeholder selected.
    pub fn wrap_selection(&mut self, before: &str, after: &str, placeholder: &str, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.selected_range.clone();
        let inner = if range.is_empty() { placeholder.to_string() } else { self.content[range.clone()].to_string() };
        self.push_undo();
        let replacement = format!("{before}{inner}{after}");
        self.content.replace_range(range.clone(), &replacement);
        let start = range.start + before.len();
        self.selected_range = start..start + inner.len();
        self.selection_reversed = false;
        self.marked_range = None;
        window.focus(&self.focus_handle);
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    /// Prefix the line(s) touched by the selection (e.g. `> ` or `- `).
    pub fn prefix_lines(&mut self, prefix: &str, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.line_start(self.selected_range.start);
        let end = self.line_end(self.selected_range.end);
        let block = self.content[start..end].to_string();
        let prefixed: String =
            if block.is_empty() { prefix.to_string() } else { block.split('\n').map(|l| format!("{prefix}{l}")).collect::<Vec<_>>().join("\n") };
        self.push_undo();
        self.content.replace_range(start..end, &prefixed);
        let cursor = start + prefixed.len();
        self.selected_range = cursor..cursor;
        window.focus(&self.focus_handle);
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    // ----- offsets ---------------------------------------------------------

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
    }

    fn display_text(&self) -> String {
        if self.masked {
            std::iter::repeat_n(MASK_CHAR, self.content.chars().count()).collect()
        } else {
            self.content.clone()
        }
    }

    /// Content byte offset → display byte offset.
    fn to_display(&self, offset: usize) -> usize {
        if self.masked {
            masked_to_display(&self.content, offset)
        } else {
            offset
        }
    }

    /// Display byte offset → content byte offset.
    fn from_display(&self, offset: usize) -> usize {
        if self.masked {
            masked_from_display(&self.content, offset)
        } else {
            offset.min(self.content.len())
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).rev().find_map(|(i, _)| (i < offset).then_some(i)).unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content.grapheme_indices(true).find_map(|(i, _)| (i > offset).then_some(i)).unwrap_or(self.content.len())
    }

    fn previous_word(&self, offset: usize) -> usize {
        if self.masked {
            return 0;
        }
        prev_word(&self.content, offset)
    }

    fn next_word(&self, offset: usize) -> usize {
        if self.masked {
            return self.content.len();
        }
        next_word(&self.content, offset)
    }

    fn line_start(&self, offset: usize) -> usize {
        if !self.multiline {
            return 0;
        }
        self.content[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, offset: usize) -> usize {
        if !self.multiline {
            return self.content.len();
        }
        self.content[offset..].find('\n').map(|i| offset + i).unwrap_or(self.content.len())
    }

    // ----- geometry ----------------------------------------------------------

    /// Position of a content offset relative to the element's origin (unscrolled).
    fn point_for_offset(&self, offset: usize) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let d = self.to_display(offset);
        let line = layout
            .lines
            .iter()
            .rev()
            .find(|l| l.start <= d)
            .or(layout.lines.first())?;
        let local = d.saturating_sub(line.start).min(line.line.len());
        let p = line.line.position_for_index(local, layout.line_height).unwrap_or(point(px(0.), px(0.)));
        Some(point(p.x, p.y + line.y))
    }

    /// Content offset closest to a point relative to the element's origin (unscrolled).
    fn offset_for_point(&self, p: Point<Pixels>) -> usize {
        let Some(layout) = self.layout.as_ref() else { return 0 };
        if self.content.is_empty() || layout.lines.is_empty() {
            return 0;
        }
        let lh = layout.line_height;
        let line = layout
            .lines
            .iter()
            .find(|l| p.y < l.y + l.line.size(lh).height)
            .unwrap_or(layout.lines.last().unwrap());
        let local = point(p.x.max(px(0.)), (p.y - line.y).max(px(0.)));
        let ix = line.line.closest_index_for_position(local, lh).unwrap_or_else(|i| i);
        self.from_display(line.start + ix.min(line.line.len()))
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(layout) = self.layout.as_ref() else { return 0 };
        let p = position - layout.bounds.origin;
        self.offset_for_point(point(p.x + self.scroll_x, p.y))
    }

    fn vertical_target(&mut self, direction: f32) -> usize {
        let cursor = self.cursor_offset();
        let Some(pos) = self.point_for_offset(cursor) else { return cursor };
        let lh = self.layout.as_ref().map(|l| l.line_height).unwrap_or(px(20.));
        let x = *self.goal_x.get_or_insert(pos.x);
        let y = pos.y + lh * direction + lh / 2.;
        if y < px(0.) {
            return 0;
        }
        let total = self
            .layout
            .as_ref()
            .and_then(|l| l.lines.last().map(|last| last.y + last.line.size(lh).height))
            .unwrap_or(px(0.));
        if y > total {
            return self.content.len();
        }
        self.offset_for_point(point(x, y))
    }

    // ----- editing -----------------------------------------------------------

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn push_undo(&mut self) {
        if self.undo.last().map(|(t, _)| t != &self.content).unwrap_or(true) {
            self.undo.push((self.content.clone(), self.selected_range.clone()));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            cx.propagate();
            return;
        }
        let t = self.vertical_target(-1.);
        self.selected_range = t..t;
        cx.notify();
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            cx.propagate();
            return;
        }
        let t = self.vertical_target(1.);
        self.selected_range = t..t;
        cx.notify();
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.move_to(self.previous_word(self.cursor_offset()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.move_to(self.next_word(self.cursor_offset()), cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let t = if self.multiline { self.vertical_target(-1.) } else { 0 };
        self.select_to(t, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let t = if self.multiline { self.vertical_target(1.) } else { self.content.len() };
        self.select_to(t, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word(self.cursor_offset()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.move_to(self.line_start(self.cursor_offset()), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.move_to(self.line_end(self.cursor_offset()), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_start(self.cursor_offset()), cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_end(self.cursor_offset()), cx);
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn backspace_word(&mut self, _: &BackspaceWord, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete_word(&mut self, _: &DeleteWord, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            cx.emit(InputEvent::Submit);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submit);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n");
            let text = if self.multiline { text } else { text.replace('\n', " ") };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((text, sel)) = self.undo.pop() {
            self.content = text;
            self.selected_range = sel.start.min(self.content.len())..sel.end.min(self.content.len());
            self.marked_range = None;
            cx.emit(InputEvent::Changed);
            cx.notify();
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.goal_x = None;
        let ix = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 {
            self.selected_range = self.line_start(ix)..self.line_end(ix);
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let start = self.previous_word(ix.min(self.next_boundary(ix)));
            let end = self.next_word(start);
            self.selected_range = start..end.max(start);
            self.selection_reversed = false;
            cx.notify();
            return;
        }
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(ix, cx);
        } else {
            self.move_to(ix, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    // ----- utf16 helpers (IME) -------------------------------------------------

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn sanitize(&self, text: &str) -> String {
        let text = if self.multiline { text.replace('\r', "") } else { text.replace(['\r', '\n'], " ") };
        match self.max_len {
            Some(max) => {
                let room = max.saturating_sub(self.content.chars().count());
                text.chars().take(room + self.content[self.selected_range.clone()].chars().count()).collect()
            }
            None => text,
        }
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _ignore_disabled_input: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected_range), reversed: self.selection_reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = self.sanitize(new_text);
        self.push_undo();
        self.content.replace_range(range.clone(), &new_text);
        let end = range.start + new_text.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range.take();
        self.goal_x = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content.replace_range(range.clone(), new_text);
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let lh = self.layout.as_ref()?.line_height;
        let a = self.point_for_offset(range.start)?;
        let b = self.point_for_offset(range.end)?;
        let off = point(bounds.left() - self.scroll_x, bounds.top());
        Some(Bounds::from_corners(a + off, point(b.x, b.y + lh) + off))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let ix = self.index_for_mouse_position(p);
        Some(self.offset_to_utf16(ix))
    }
}

/// The element that lays out, paints and hit-tests the text.
struct TextElement {
    input: Entity<TextInput>,
    placeholder_color: Hsla,
    selection_color: Hsla,
    cursor_color: Hsla,
}

struct PrepaintState {
    lines: Vec<LaidOutLine>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn runs_for(text: &str, style: &gpui::TextStyle, color: Hsla, marked: Option<Range<usize>>) -> Vec<TextRun> {
    let run = TextRun { len: text.len(), font: style.font(), color, background_color: None, underline: None, strikethrough: None };
    match marked {
        Some(m) if m.end <= text.len() => [
            TextRun { len: m.start, ..run.clone() },
            TextRun {
                len: m.end - m.start,
                underline: Some(UnderlineStyle { color: Some(color), thickness: px(1.), wavy: false }),
                ..run.clone()
            },
            TextRun { len: text.len() - m.end, ..run },
        ]
        .into_iter()
        .filter(|r| r.len > 0)
        .collect(),
        _ => vec![run],
    }
}

fn shape(text: &str, runs: &[TextRun], font_size: Pixels, wrap: Option<Pixels>, line_height: Pixels, window: &mut Window) -> Vec<LaidOutLine> {
    let shaped = window
        .text_system()
        .shape_text(SharedString::from(text.to_string()), font_size, runs, wrap, None)
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut start = 0;
    let mut y = px(0.);
    for line in shaped {
        let mut row_starts = vec![0];
        for b in line.wrap_boundaries() {
            if let Some(g) = line.runs().get(b.run_ix).and_then(|r| r.glyphs.get(b.glyph_ix)) {
                row_starts.push(g.index);
            }
        }
        let h = line.size(line_height).height;
        let len = line.len();
        out.push(LaidOutLine { line, start, y, row_starts });
        start += len + 1; // + '\n'
        y += h;
    }
    out
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let input = self.input.read(cx);
        let line_height = window.line_height();
        // A growing flex item rather than `width: 100%`: percentage widths can
        // resolve to (almost) zero depending on the parent, hiding the text.
        let mut style = Style::default();
        style.flex_grow = 1.;
        style.flex_shrink = 1.;
        style.flex_basis = px(0.).into();
        style.min_size.width = px(0.).into();
        if !input.multiline {
            style.size.height = line_height.into();
            return (window.request_layout(style, [], cx), ());
        }
        let min_rows = input.min_rows;
        let entity = self.input.clone();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let layout = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let input = entity.read(cx);
            let text = input.display_text();
            let text = if text.is_empty() { input.placeholder.to_string() } else { text };
            let runs = runs_for(&text, &text_style, text_style.color, None);
            let lines = shape(&text, &runs, font_size, width, line_height, window);
            let content_h = lines.last().map(|l| l.y + l.line.size(line_height).height).unwrap_or(line_height);
            size(width.unwrap_or(px(200.)), content_h.max(line_height * min_rows as f32))
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let line_height = window.line_height();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let display = input.display_text();
        let is_placeholder = display.is_empty();
        let (text, color, marked) = if is_placeholder {
            (input.placeholder.to_string(), self.placeholder_color, None)
        } else {
            let marked = input.marked_range.as_ref().map(|m| input.to_display(m.start)..input.to_display(m.end));
            (display, style.color, marked)
        };
        let runs = runs_for(&text, &style, color, marked);
        let wrap = input.multiline.then_some(bounds.size.width);
        let lines = shape(&text, &runs, font_size, wrap, line_height, window);

        // Temporarily compute positions with this layout (the entity's layout is updated in paint).
        let pos_for = |offset: usize| -> Point<Pixels> {
            if is_placeholder {
                return point(px(0.), px(0.));
            }
            let d = input.to_display(offset);
            let Some(line) = lines.iter().rev().find(|l| l.start <= d).or(lines.first()) else { return point(px(0.), px(0.)) };
            let local = d.saturating_sub(line.start).min(line.line.len());
            let p = line.line.position_for_index(local, line_height).unwrap_or_default();
            point(p.x, p.y + line.y)
        };

        let cursor_pos = pos_for(input.cursor_offset());
        let mut scroll_x = input.scroll_x;
        if !input.multiline {
            let width = bounds.size.width - px(2.);
            if cursor_pos.x - scroll_x > width {
                scroll_x = cursor_pos.x - width;
            } else if cursor_pos.x < scroll_x {
                scroll_x = cursor_pos.x;
            }
            let text_w = lines.first().map(|l| l.line.width()).unwrap_or_default();
            if text_w < width {
                scroll_x = px(0.);
            }
            scroll_x = scroll_x.max(px(0.));
        }
        let origin = point(bounds.left() - scroll_x, bounds.top());

        let selected = input.selected_range.clone();
        let mut selections = Vec::new();
        let mut cursor = None;
        if selected.is_empty() || is_placeholder {
            cursor = Some(fill(
                Bounds::new(point(origin.x + cursor_pos.x, origin.y + cursor_pos.y), size(px(1.5), line_height)),
                self.cursor_color,
            ));
        } else {
            let (a, b) = (input.to_display(selected.start), input.to_display(selected.end));
            for line in &lines {
                let line_end = line.start + line.line.len();
                if b < line.start || a > line_end {
                    continue;
                }
                let mut rows = line.row_starts.clone();
                rows.push(line.line.len() + 1);
                for (row, w) in rows.windows(2).enumerate() {
                    let (rs, re) = (line.start + w[0], line.start + w[1]);
                    if b < rs || a >= re {
                        continue;
                    }
                    let y = line.y + line_height * row as f32;
                    let x1 = if a > rs {
                        line.line.position_for_index(a - line.start, line_height).map(|p| p.x).unwrap_or_default()
                    } else {
                        px(0.)
                    };
                    let x2 = if b < re && b <= line_end {
                        line.line.position_for_index(b - line.start, line_height).map(|p| p.x).unwrap_or_default()
                    } else if input.multiline {
                        bounds.size.width.min(line.line.width() + px(6.)).max(x1 + px(6.))
                    } else {
                        line.line.width()
                    };
                    selections.push(fill(
                        Bounds::from_corners(point(origin.x + x1, origin.y + y), point(origin.x + x2.max(x1), origin.y + y + line_height)),
                        self.selection_color,
                    ));
                }
            }
        }
        PrepaintState { lines, line_height, cursor, selections, scroll_x }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        let disabled = self.input.read(cx).disabled;
        if !disabled {
            window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        }
        let lh = prepaint.line_height;
        let origin = point(bounds.left() - prepaint.scroll_x, bounds.top());
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for sel in prepaint.selections.drain(..) {
                window.paint_quad(sel);
            }
            for line in &prepaint.lines {
                line.line.paint(point(origin.x, origin.y + line.y), lh, TextAlign::Left, None, window, cx).ok();
            }
            if focus_handle.is_focused(window) && !disabled {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
        });
        let lines = std::mem::take(&mut prepaint.lines);
        let scroll_x = prepaint.scroll_x;
        let placeholder = self.input.read(cx).content.is_empty();
        self.input.update(cx, |input, _| {
            input.scroll_x = scroll_x;
            // Hit-testing should use the real (possibly empty) content layout, not the placeholder.
            input.layout = Some(Layout { lines: if placeholder { Vec::new() } else { lines }, line_height: lh, bounds });
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.theme();
        let focused = self.focus_handle.is_focused(window);
        let border = if focused { t.colors.input_border_focus } else { gpui::transparent_black() };
        let hover_border = if focused { t.colors.input_border_focus } else { t.colors.input_border_hover };
        let line_height = if self.multiline { px(21.) } else { px(20.) };
        div()
            .id(("text-input", cx.entity_id()))
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(if self.disabled { CursorStyle::Arrow } else { CursorStyle::IBeam })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::backspace_word))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::undo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .px(px(10.))
            .py(px(if self.multiline { 8. } else { 6. }))
            .rounded(t.metrics.radius)
            .bg(t.colors.input)
            .border_1()
            .border_color(border)
            .hover(move |s| s.border_color(hover_border))
            .when(self.disabled, |d| d.opacity(0.6))
            .text_size(t.fs.normal)
            .line_height(line_height)
            .text_color(t.colors.text)
            .flex()
            .items_center()
            .gap(px(8.))
            .when_some(self.icon, |d, icon| d.child(crate::icons::icon(icon, t.colors.text_light)))
            .child(TextElement {
                input: cx.entity(),
                placeholder_color: t.colors.text_light.opacity(0.7),
                selection_color: t.colors.brand.opacity(0.3),
                cursor_color: t.colors.text,
            })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Content byte offset → display byte offset when every char is shown as one mask char.
fn masked_to_display(content: &str, offset: usize) -> usize {
    content[..offset.min(content.len())].chars().count() * MASK_CHAR.len_utf8()
}

/// Display byte offset → content byte offset for masked text.
fn masked_from_display(content: &str, offset: usize) -> usize {
    let n = offset / MASK_CHAR.len_utf8();
    content.char_indices().nth(n).map(|(i, _)| i).unwrap_or(content.len())
}

fn prev_word(content: &str, offset: usize) -> usize {
    content
        .split_word_bound_indices()
        .rev()
        .find(|(i, w)| *i < offset && !w.trim().is_empty())
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_word(content: &str, offset: usize) -> usize {
    content
        .split_word_bound_indices()
        .find(|(i, w)| i + w.len() > offset && !w.trim().is_empty())
        .map(|(i, w)| i + w.len())
        .unwrap_or(content.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_offsets_roundtrip() {
        let s = "héllo";
        // 'é' is 2 bytes; display uses 3-byte bullets.
        assert_eq!(masked_to_display(s, 3), 6);
        assert_eq!(masked_from_display(s, 6), 3);
        assert_eq!(masked_from_display(s, 15), s.len());
    }

    #[test]
    fn word_boundaries() {
        let s = "hello  big world";
        assert_eq!(next_word(s, 0), 5);
        assert_eq!(next_word(s, 5), 10);
        assert_eq!(prev_word(s, 16), 11);
        assert_eq!(prev_word(s, 11), 7);
        assert_eq!(prev_word(s, 0), 0);
    }
}
