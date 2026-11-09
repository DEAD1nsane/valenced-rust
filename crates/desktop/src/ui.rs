//! Minimal GPUI shell (behind `--features gpui`).
//!
//! Phoenix-style: native GPU window, no Electron/WebView. Shows the active
//! theme name + first N semantic colors, plus the plugin list from
//! `Registry::load_from_dist`. Headless `cargo test` never builds this.

#[cfg(feature = "gpui")]
use gpui::{actions, div, Context, FocusHandle, HttpClient, ParentElement, Styled, Window, WindowOptions, Styled as _, Menu, Model, Subscription, Application, Focusable, KeyBinding, Titlebar, InteractiveElement, StatefulInteractiveElement, scrollable, WindowHandler, WindowHandle, IntoElement, AnyElement, prelude::*};

/// Placeholder view. Real Phoenix parity (server list, channels, DMs)
/// is out of scope for the Valenced shell — this proves windowing +
/// theme rendering from the shared Rust core.
#[cfg(feature = "gpui")]
pub struct ThemeView {
    focus_handle: FocusHandle,
    theme_name: String,
    color_lines: Vec<String>,
    plugin_lines: Vec<String>,
}

#[cfg(feature = "gpui")]
impl ThemeView {
    pub fn new(cx: &mut Context<Self>, theme_name: String, color_lines: Vec<String>, plugin_lines: Vec<String>) -> Self {
        Self { focus_handle: cx.focus_handle(), theme_name, color_lines, plugin_lines }
    }
}

#[cfg(feature = "gpui")]
impl Render for ThemeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.color_lines.clone();
        let plugins = self.plugin_lines.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .p_4()
            .gap_2()
            .child(format!("Theme: {}", self.theme_name))
            .child(div().child("Colors:"))
            .children(colors.into_iter().map(|c| div().child(c)))
            .child(div().child("Plugins:"))
            .children(plugins.into_iter().map(|p| div().child(p)))
    }
}

#[cfg(feature = "gpui")]
impl Focusable for ThemeView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Open the shell window. `theme_name`/`color_lines` come from
/// `valenced_core::theme::Theme`; `plugin_lines` from `Registry`.
#[cfg(feature = "gpui")]
pub fn run(theme_name: String, color_lines: Vec<String>, plugin_lines: Vec<String>) {
    let app = Application::new();
    let _ = app.run(|cx| {
        let model = cx.new(|cx| ThemeView::new(cx, theme_name, color_lines, plugin_lines));
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|cx| ThemeView::new(cx, "Valenced".into(), vec![], vec![]))
        })
        .expect("open window");
    });
}
