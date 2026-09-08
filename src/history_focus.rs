//! Each simultaneously visible history field owns its native input focus.
use crate::Field;
use gpui::{App, FocusHandle};

pub(super) struct HistoryFocus {
    search: FocusHandle,
    from: FocusHandle,
    through: FocusHandle,
}

impl HistoryFocus {
    pub fn new(cx: &App) -> Self {
        Self {
            search: cx.focus_handle(),
            from: cx.focus_handle(),
            through: cx.focus_handle(),
        }
    }

    pub fn for_field(&self, field: Field) -> &FocusHandle {
        match field {
            Field::TransferSearch => &self.search,
            Field::HistoryFrom => &self.from,
            Field::HistoryTo => &self.through,
            _ => unreachable!("not a history input"),
        }
    }
}

#[cfg(all(test, feature = "ui-tests"))]
mod tests {
    use super::*;
    use gpui::{Context, IntoElement, ParentElement, Render, Styled, TestAppContext, Window, div};

    struct Fields(HistoryFocus);
    impl Render for Fields {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use gpui::InteractiveElement;
            div().flex().children([
                div()
                    .track_focus(self.0.for_field(Field::TransferSearch))
                    .child("Search"),
                div()
                    .track_focus(self.0.for_field(Field::HistoryFrom))
                    .child("From"),
                div()
                    .track_focus(self.0.for_field(Field::HistoryTo))
                    .child("Through"),
            ])
        }
    }

    #[gpui::test]
    fn history_inputs_never_share_native_focus(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Fields(HistoryFocus::new(cx)));
        let fields = [Field::TransferSearch, Field::HistoryFrom, Field::HistoryTo];
        let handles = view.update(cx, |view, _| {
            fields.map(|field| view.0.for_field(field).clone())
        });
        for active in 0..handles.len() {
            cx.update(|window, cx| {
                handles[active].focus(window, cx);
                for (index, handle) in handles.iter().enumerate() {
                    assert_eq!(handle.is_focused(window), index == active);
                }
            });
        }
    }
}
