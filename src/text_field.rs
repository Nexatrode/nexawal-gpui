//! Native GPUI text editing for NexaWal's form fields.
//!
//! GPUI deliberately keeps text editing low-level.  This element supplies the
//! same important pieces used by the GPUI input example: a platform input
//! handler, UTF-16/UTF-8 conversion for IME APIs, a shaped cursor, selections,
//! mouse hit testing, and grapheme-aware movement.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point, SharedString,
    Style, TextRun, UTF16Selection, UnderlineStyle, Window, fill, point, px, relative, rgba,
};

pub(crate) struct FieldElement {
    input: Entity<crate::Home>,
    field: crate::Field,
    challenge_slot: Option<usize>,
    placeholder: SharedString,
    multiline: bool,
}

impl FieldElement {
    pub(crate) fn new(
        input: Entity<crate::Home>,
        field: crate::Field,
        challenge_slot: Option<usize>,
        placeholder: impl Into<SharedString>,
        multiline: bool,
    ) -> Self {
        Self {
            input,
            field,
            challenge_slot,
            placeholder: placeholder.into(),
            multiline,
        }
    }
}

/// Byte-indexed shaped paragraphs, shared by painting, mouse selection and IME geometry.
pub(crate) struct FieldLayout {
    lines: Vec<gpui::WrappedLine>,
    line_height: Pixels,
    scroll: Point<Pixels>,
}
impl FieldLayout {
    fn size(&self) -> gpui::Size<Pixels> {
        gpui::size(self.lines.iter().map(|l| l.width()).fold(px(0.), Pixels::max),
            self.lines.iter().map(|l| l.size(self.line_height).height).sum())
    }
    pub(crate) fn position(&self, index: usize) -> Point<Pixels> {
        let mut offset = 0;
        let mut y = px(0.);
        for line in &self.lines {
            if index <= offset + line.len() {
                return line.position_for_index(index.saturating_sub(offset), self.line_height)
                    .unwrap_or_default() + point(px(0.), y) - self.scroll;
            }
            offset += line.len() + 1; // logical newline; visual wraps consume no bytes
            y += line.size(self.line_height).height;
        }
        point(px(0.), y) - self.scroll
    }
    pub(crate) fn closest_index(&self, position: Point<Pixels>) -> usize {
        let mut offset = 0;
        let mut y = px(0.);
        let position = position + self.scroll;
        for (i, line) in self.lines.iter().enumerate() {
            let height = line.size(self.line_height).height;
            if position.y < y + height || i + 1 == self.lines.len() {
                return offset + line.closest_index_for_position(position - point(px(0.), y), self.line_height)
                    .unwrap_or_else(|index| index);
            }
            offset += line.len() + 1;
            y += height;
        }
        0
    }
}

pub(crate) struct PrepaintState {
    layout: Option<FieldLayout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for FieldElement {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}

impl Element for FieldElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;
    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }
    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !self.multiline {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        let home = self.input.read(cx);
        let content = home.field_text_at(self.field, self.challenge_slot);
        let text: SharedString = if content.is_empty() { self.placeholder.clone() } else { content.into() };
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let run = text_style.to_run(text.len());
        let height = window.line_height();
        (window.request_measured_layout(style, move |known, available, window, _cx| {
            let width = known.width.or(match available.width { gpui::AvailableSpace::Definite(w) => Some(w), _ => None });
            let lines = window.text_system().shape_text(text.clone(), font_size, &[run.clone()], width, None).unwrap_or_default();
            let actual = lines.iter().map(|l| l.size(height).height).sum::<Pixels>();
            gpui::size(width.unwrap_or(px(240.)), actual.max(height).min(height * 8))
        }), ())
    }
    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>, _request: &mut (), window: &mut Window, cx: &mut App) -> PrepaintState {
        let home = self.input.read(cx);
        let content = home.field_text_at(self.field, self.challenge_slot);
        let editing = home.edit_field == Some(self.field) && self.challenge_slot.is_none_or(|s| home.challenge_slot == s);
        let text: SharedString = if content.is_empty() { self.placeholder.clone() } else { content.clone().into() };
        let style = window.text_style();
        let color = if content.is_empty() { crate::theme_muted() } else { crate::theme_text() };
        let run = TextRun { len: text.len(), font: style.font(), color: gpui::rgb(color).into(),
            background_color: None, underline: None, strikethrough: None };
        let runs = if editing {
            if let Some(marked) = &home.edit_marked_range {
                vec![TextRun { len: marked.start, ..run.clone() },
                    TextRun { len: marked.end.saturating_sub(marked.start), underline: Some(UnderlineStyle {
                        color: Some(gpui::rgb(color).into()), thickness: px(1.), wavy: false }), ..run.clone() },
                    TextRun { len: text.len().saturating_sub(marked.end), ..run }]
                    .into_iter().filter(|r| r.len > 0).collect::<Vec<_>>()
            } else { vec![run] }
        } else { vec![run] };
        let lines = window.text_system().shape_text(text, style.font_size.to_pixels(window.rem_size()), &runs,
            self.multiline.then_some(bounds.size.width.max(px(1.))), None).unwrap_or_default();
        let mut layout = FieldLayout { lines: lines.into_vec(), line_height: window.line_height(), scroll: point(px(0.), px(0.)) };
        let index = if home.edit_selection_reversed { home.edit_selection.start } else { home.edit_selection.end };
        let caret = layout.position(index.min(content.len()));
        if editing {
            layout.scroll = caret_scroll(caret, layout.size(), bounds.size, layout.line_height, self.multiline);
        }
        let mut selection = Vec::new();
        let caret = layout.position(index.min(content.len()));
        let cursor = if editing && home.edit_selection.is_empty() {
            Some(fill(Bounds::new(bounds.origin + caret, gpui::size(px(2.), layout.line_height)), gpui::rgb(crate::theme_accent())))
        } else { None };
        if editing && !home.edit_selection.is_empty() {
            let start = layout.position(home.edit_selection.start.min(content.len()));
            let end = layout.position(home.edit_selection.end.min(content.len()));
            let mut y = start.y;
            while y <= end.y {
                let left = if y == start.y { start.x } else { px(0.) };
                let right = if y == end.y { end.x } else { bounds.size.width };
                selection.push(fill(Bounds::new(bounds.origin + point(left, y), gpui::size((right-left).max(px(0.)), layout.line_height)), rgba(0x335b9bff)));
                y += layout.line_height;
            }
        }
        PrepaintState { layout: Some(layout), cursor, selection }
    }
    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>, _request: &mut (), prepaint: &mut PrepaintState, window: &mut Window, cx: &mut App) {
        let focus = self.input.read(cx).focus_handle_for(self.field).clone();
        if focus.is_focused(window) { window.handle_input(&focus, ElementInputHandler::new(bounds, self.input.clone()), cx); }
        let layout = prepaint.layout.take().expect("field layout");
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for selection in prepaint.selection.drain(..) { window.paint_quad(selection); }
            let mut origin = bounds.origin - layout.scroll;
            for line in &layout.lines {
                line.paint(origin, layout.line_height, gpui::TextAlign::Left, Some(bounds), window, cx).ok();
                origin.y += line.size(layout.line_height).height;
            }
            if focus.is_focused(window) && let Some(cursor) = prepaint.cursor.take() { window.paint_quad(cursor); }
        });
        self.input.update(cx, |home, _| {
            if home.edit_field == Some(self.field) && focus.is_focused(window) {
                home.edit_last_layout = Some(layout); home.edit_last_bounds = Some(bounds);
            }
        });
    }
}

fn caret_scroll(caret: Point<Pixels>, text: gpui::Size<Pixels>, viewport: gpui::Size<Pixels>, height: Pixels, multiline: bool) -> Point<Pixels> {
    if multiline {
        point(px(0.), (caret.y + height - viewport.height).max(px(0.)).min((text.height-viewport.height).max(px(0.))))
    } else {
        point((caret.x + px(3.) - viewport.width).max(px(0.)).min((text.width + px(3.)-viewport.width).max(px(0.))), px(0.))
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[cfg(feature = "ui-tests")]
    #[gpui::test]
    fn wrapped_multibyte_text_round_trips_mouse_and_ime_positions(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        crate::paths::with_test_data_dir(directory.path().to_owned(), || {
            let (_, cx) = cx.add_window_view(|_, cx| crate::Home::new(cx));
            cx.update(|window, _| {
                let text: SharedString = "αβγδεζηθικλμνξοπρστυφχψω\nsecond paragraph".into();
                let style = window.text_style();
                let lines = window.text_system().shape_text(text.clone(), style.font_size.to_pixels(window.rem_size()),
                    &[style.to_run(text.len())], Some(px(65.)), None).unwrap();
                let layout = FieldLayout { lines: lines.into_vec(), line_height: px(20.), scroll: point(px(0.), px(0.)) };
                assert!(layout.size().height > px(40.), "long paragraphs must wrap");
                for index in [2, 10, 24, 50, text.len()] {
                    let position = layout.position(index);
                    assert_eq!(layout.closest_index(position + point(px(0.1), px(1.))), index);
                }
            });
        });
    }

    #[test]
    fn long_single_line_scrolls_to_caret_and_returns_to_start() {
        let text = gpui::size(px(900.), px(20.));
        let viewport = gpui::size(px(200.), px(20.));
        assert_eq!(caret_scroll(point(px(900.), px(0.)), text, viewport, px(20.), false), point(px(703.), px(0.)));
        assert_eq!(caret_scroll(point(px(0.), px(0.)), text, viewport, px(20.), false), point(px(0.), px(0.)));
    }

    #[test]
    fn multiline_scrolls_vertically_only_and_keeps_last_row_visible() {
        let text = gpui::size(px(200.), px(400.));
        let viewport = gpui::size(px(200.), px(160.));
        assert_eq!(caret_scroll(point(px(190.), px(380.)), text, viewport, px(20.), true), point(px(0.), px(240.)));
        assert_eq!(caret_scroll(point(px(0.), px(0.)), text, viewport, px(20.), true), point(px(0.), px(0.)));
    }
}

impl EntityInputHandler for crate::Home {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.field_text(self.active);
        let range = range_from_utf16(&text, &range_utf16);
        actual_range.replace(range_to_utf16(&text, &range));
        Some(text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let text = self.field_text(self.active);
        Some(UTF16Selection {
            range: range_to_utf16(&text, &self.edit_selection),
            reversed: self.edit_selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let text = self.field_text(self.active);
        self.edit_marked_range
            .as_ref()
            .map(|range| range_to_utf16(&text, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.edit_marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = self.active;
        let current = self.field_text(field);
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&current, range))
            .or_else(|| self.edit_marked_range.clone())
            .unwrap_or_else(|| self.edit_selection.clone());
        let start = range.start.min(current.len());
        let end = range.end.min(current.len()).max(start);
        let replacement = crate::Home::sanitize_edit_text(field, new_text);
        let mut updated = String::with_capacity(current.len() + replacement.len());
        updated.push_str(&current[..start]);
        updated.push_str(&replacement);
        updated.push_str(&current[end..]);
        self.set_field_text_raw(field, updated);
        let cursor = start + replacement.len();
        self.edit_selection = cursor..cursor;
        self.edit_selection_reversed = false;
        self.edit_marked_range = None;
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
        let field = self.active;
        let current = self.field_text(field);
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&current, range))
            .or_else(|| self.edit_marked_range.clone())
            .unwrap_or_else(|| self.edit_selection.clone());
        let start = range.start.min(current.len());
        let end = range.end.min(current.len()).max(start);
        let mut updated = String::with_capacity(current.len() + new_text.len());
        updated.push_str(&current[..start]);
        updated.push_str(new_text);
        updated.push_str(&current[end..]);
        self.set_field_text_raw(field, updated);
        self.edit_marked_range = if new_text.is_empty() {
            None
        } else {
            Some(start..start + new_text.len())
        };
        self.edit_selection = new_selected_range_utf16
            .as_ref()
            .map(|range| range_from_utf16(new_text, range))
            .map(|range| start + range.start..start + range.end)
            .unwrap_or_else(|| start + new_text.len()..start + new_text.len());
        self.edit_selection_reversed = false;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let text = self.field_text(self.active);
        let range = range_from_utf16(&text, &range_utf16);
        let line = self.edit_last_layout.as_ref()?;
        let start = line.position(range.start);
        let end = line.position(range.end);
        Some(Bounds::from_corners(
            bounds.origin + point(if start.y == end.y { start.x } else { px(0.) }, start.y),
            bounds.origin + point(if start.y == end.y { end.x } else { bounds.size.width }, end.y + line.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.edit_last_bounds?;
        let line = self.edit_last_layout.as_ref()?;
        let text = self.field_text(self.active);
        Some(offset_to_utf16(&text, line.closest_index(point - bounds.origin)))
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let text = self.field_text(self.active);
        self.edit_selection = range_from_utf16(&text, &range_utf16);
        self.edit_selection_reversed = false;
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.field_text(self.active).encode_utf16().count())
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        if matches!(self.screen, crate::Screen::Terms | crate::Screen::Legal) {
            return false;
        }
        if self.opened
            && !matches!(
                self.screen,
                crate::Screen::Send | crate::Screen::Settings | crate::Screen::Receive | crate::Screen::Transactions
            )
            && !(self.screen == crate::Screen::Wallet
                && self.active == crate::Field::TransferSearch)
        {
            return false;
        }
        true
    }
}

fn offset_to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

fn offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
}

fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_from_utf16(text, range.start)..offset_from_utf16(text, range.end)
}
