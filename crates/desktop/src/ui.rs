//! Minimal GPUI shell (GPUI 0.2, desktop-only).
//!
//! Mirrors the canonical `gpui-0.2` example shape (`examples/hello_world.rs`):
//! `Application::run` -> `cx.open_window` -> root view with `Render` + `div()`.
//!
//! Phoenix-style: native GPU window, no Electron/WebView. Shows the active
//! theme name + first N semantic colors, plus the plugin list from
//! `Registry::load_from_dist`. Headless `cargo test` (default features) never
//! builds this module; see `main.rs` (`mod ui` is `#[cfg(feature = "gpui")]`).
//!
//! Desktop-only builds:
//! ```text
//! cargo run -p valenced-desktop --features gpui    # Linux glibc (x11/wayland)
//! ```
use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};

/// Placeholder view. Real Phoenix parity (server list, channels, DMs)
/// is out of scope for the Valenced shell — this proves windowing +
/// theme rendering from the shared Rust core.
pub struct ThemeView {
    theme_name: String,
    color_lines: Vec<String>,
    plugin_lines: Vec<String>,
}

impl ThemeView {
    pub fn new(theme_name: String, color_lines: Vec<String>, plugin_lines: Vec<String>) -> Self {
        Self {
            theme_name,
            color_lines,
            plugin_lines,
        }
    }
}

impl Render for ThemeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
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

/// Open the shell window. `theme_name`/`color_lines` come from
/// `valenced_core::theme::Theme`; `plugin_lines` from `Registry`.
pub fn run(theme_name: String, color_lines: Vec<String>, plugin_lines: Vec<String>) {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| ThemeView::new(theme_name, color_lines, plugin_lines)),
        )
        .expect("open window");
        cx.activate(true);
    });
}
