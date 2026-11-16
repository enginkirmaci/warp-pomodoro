//! The About section: the installed family in the list, the selected member in
//! the detail. The screen itself is `warpui_kit_core::about`; this holds the
//! one piece of state it needs and the effects behind its verbs, so every app's
//! About behaves the same and only the rail plumbing differs.

use gpui::prelude::*;
use gpui::{App, ClipboardItem, Context, Window};
use warpui_kit_core::about;
use warpui_kit_core::shell;

use crate::theme;
use crate::ui::window::{MainWindow, launch_sibling};

impl MainWindow {
    /// The rail's About section: the installed members in the list, the
    /// selected one's identity, Build and Legal cards in the detail. It opens
    /// on Pomodoro, the app you are in.
    pub(crate) fn render_about(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use warpui_kit_core::about::AppId;

        // the list takes a quarter of the window, between the compact floor
        // (the detail's cards need their 320) and the family's full list width
        let width = (window.viewport_size().width.as_f32() * 0.26).clamp(
            theme::scaled(theme::ABOUT_LIST_MIN_W),
            theme::scaled(theme::ABOUT_LIST_MAX_W),
        );
        let selected = self.about_selected;
        let current = AppId::Pomodoro;
        if self.about_members.is_empty() {
            let build = warpui_kit_core::build_info!();
            self.about_members = about::detect_installed(current, &build);
        }

        let entity = cx.entity();
        let pick = entity.clone();
        let on_pick = move |id: AppId, _: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            pick.update(cx, |this, cx| {
                this.about_selected = id;
                cx.notify();
            });
        };
        let act = entity.clone();
        let on_action =
            move |action: about::Action, _: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                act.update(cx, |this, cx| this.apply_about_action(action, cx));
            };
        let open = entity;
        let on_link =
            move |link: about::Link, _: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                open.update(cx, |this, cx| this.open_about_link(link, cx));
            };

        shell::panes()
            .min_w_0()
            .debug_selector(|| "about-page".into())
            .child(about::list(
                width,
                selected,
                current,
                &self.about_members,
                on_pick,
            ))
            .child(about::detail(
                selected,
                current,
                &self.about_members,
                "Open Station",
                on_action,
                on_link,
            ))
            .into_any_element()
    }

    /// What the About detail's buttons do. A sibling app can be launched but
    /// not steered, so SnapIt's row here means "open Station", where its
    /// controls live.
    pub(crate) fn apply_about_action(&mut self, action: about::Action, cx: &mut Context<Self>) {
        match action {
            about::Action::ReleaseNotes => {
                let url = format!("{}/releases", about::repository());
                cx.open_url(&url);
            }
            about::Action::OpenApp(id) => launch_sibling(about::app(id).binary),
            about::Action::SnapIt => launch_sibling("warp-station"),
            about::Action::CopyVersion => {
                let Some(member) = about::selected_member(
                    &self.about_members,
                    self.about_selected,
                    about::AppId::Pomodoro,
                ) else {
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_string(member.version_line()));
            }
        }
    }

    /// Every link, the license included, opens in the browser: the GPL text
    /// lives in this repository, and the About screen can point at it without
    /// pulling the markdown renderer in for one static document.
    pub(crate) fn open_about_link(&mut self, link: about::Link, cx: &mut Context<Self>) {
        let url = link.url(about::app(self.about_selected));
        cx.open_url(&url);
    }
}
