//! Native GPUI text editing for NexaWal's form fields.
//!
//! GPUI deliberately keeps text editing low-level.  This element supplies the
//! same important pieces used by the GPUI input example: a platform input
//! handler, UTF-16/UTF-8 conversion for IME APIs, a shaped cursor, selections,
//! mouse hit testing, and grapheme-aware movement.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point, ShapedLine, SharedString,
    Style, TextRun, UTF16Selection, UnderlineStyle, Window, fill, point, px, relative, rgba,
};

pub(crate) struct FieldElement {
    input: Entity<crate::Home>,
    field: crate::Field,
    challenge_slot: Option<usize>,
    placeholder: SharedString,
}

impl FieldElement {
    pub(crate) fn new(
        input: Entity<crate::Home>,
        field: crate::Field,
        challenge_slot: Option<usize>,
        placeholder: impl Into<SharedString>,
    ) -> Self {
        Self {
            input,
            field,
            challenge_slot,
            placeholder: placeholder.into(),
        }
    }
}

pub(crate) struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for FieldElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FieldElement {
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let home = self.input.read(cx);
        let content = home.field_text_at(self.field, self.challenge_slot);
        let editing = home.edit_field == Some(self.field)
            && self
                .challenge_slot
                .is_none_or(|slot| home.challenge_slot == slot);
        let selected_range = if editing {
            home.edit_selection.clone()
        } else {
            0..0
        };
        let cursor_offset = if home.edit_selection_reversed {
            home.edit_selection.start
        } else {
            home.edit_selection.end
        };
        let display_text: SharedString = if content.is_empty() {
            self.placeholder.clone()
        } else {
            content.clone().into()
        };
        let text_color = if content.is_empty() {
            gpui::rgb(crate::theme_muted())
        } else {
            gpui::rgb(crate::theme_text())
        };
        let style = window.text_style();
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if editing {
            if let Some(marked_range) = home.edit_marked_range.as_ref() {
                vec![
                    TextRun {
                        len: marked_range.start,
                        ..run.clone()
                    },
                    TextRun {
                        len: marked_range.end.saturating_sub(marked_range.start),
                        underline: Some(UnderlineStyle {
                            color: Some(text_color.into()),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..run.clone()
                    },
                    TextRun {
                        len: display_text.len().saturating_sub(marked_range.end),
                        ..run
                    },
                ]
                .into_iter()
                .filter(|run| run.len > 0)
                .collect()
            } else {
                vec![run]
            }
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text.clone(), font_size, &runs, None);
        let cursor_offset = cursor_offset.min(content.len());
        let cursor_pos = line.x_for_index(cursor_offset);
        let (selection, cursor) = if editing && !selected_range.is_empty() {
            let start = selected_range.start.min(content.len());
            let end = selected_range.end.min(content.len()).max(start);
            (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(start), bounds.top()),
                        point(bounds.left() + line.x_for_index(end), bounds.bottom()),
                    ),
                    rgba(0x335b9bff),
                )),
                None,
            )
        } else if editing {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        gpui::size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    gpui::rgb(crate::theme_accent()),
                )),
            )
        } else {
            (None, None)
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle_for(self.field).clone();
        if focus_handle.is_focused(window) {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("text field line");
        line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .ok();
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |home, _cx| {
            if home.edit_field == Some(self.field) && focus_handle.is_focused(window) {
                home.edit_last_layout = Some(line);
                home.edit_last_bounds = Some(bounds);
            }
        });
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
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
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
        let x = point.x - bounds.left();
        Some(offset_to_utf16(&text, line.closest_index_for_x(x)))
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
                crate::Screen::Send | crate::Screen::Settings | crate::Screen::Receive
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
