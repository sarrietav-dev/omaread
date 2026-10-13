//! The chrome's GTK stylesheet, generated from a [`Theme`].
//!
//! Sizes are Style.qml's type scale and spacing ladder, colours its state
//! rules (a tint of the foreground at the theme's alphas, hairline borders,
//! no rounding), so overlays read like the rest of the Omarchy shell.
//! libadwaita's named colours are redefined too, so any stock widget we use
//! wears the same palette.

use std::fmt::Write as _;

use crate::theme::Theme;

pub fn chrome_css(t: &Theme) -> String {
    let fg = t.foreground;
    let bg = t.background;
    let c = &t.controls;

    let caption = t.font_px(0.833);
    let body_small = t.font_px(0.917);
    let body = t.font_px(1.0);
    let title = t.font_px(1.167);
    let heading = t.font_px(1.333);
    let display = t.font_px(2.0);

    let hairline = t.space(1.0).max(1);
    let sm = t.space(4.0);
    let md = t.space(6.0);
    let lg = t.space(8.0);
    let row_x = t.space(12.0);
    let popup = t.space(14.0);
    let panel = t.space(18.0);

    let mut css = String::new();
    let named = [
        ("window_bg_color", bg.to_string()),
        ("window_fg_color", fg.to_string()),
        ("view_bg_color", bg.to_string()),
        ("view_fg_color", fg.to_string()),
        ("headerbar_bg_color", bg.to_string()),
        ("headerbar_fg_color", fg.to_string()),
        ("popover_bg_color", bg.to_string()),
        ("popover_fg_color", fg.to_string()),
        ("dialog_bg_color", bg.to_string()),
        ("dialog_fg_color", fg.to_string()),
        ("card_bg_color", fg.alpha(c.normal_fill_alpha)),
        ("card_fg_color", fg.to_string()),
        ("accent_color", t.accent.lifted(bg, fg, 3.0).to_string()),
        ("accent_bg_color", t.accent.to_string()),
        ("accent_fg_color", t.accent.readable_ink().to_string()),
        ("borders", fg.alpha(c.normal_border_alpha)),
    ];
    for (name, value) in named {
        let _ = writeln!(css, "@define-color {name} {value};");
    }

    let _ = write!(
        css,
        r#"
window.omaread {{
  background-color: {bg};
  color: {fg};
  font-family: monospace;
  font-size: {body}px;
}}
window.omaread * {{ border-radius: 0; }}

.reader-status {{
  font-size: {body_small}px;
  color: {muted};
  padding: {sm}px {row_x}px;
  background-color: {bg};
  border-top: {hairline}px solid {rule};
}}
.reader-status .section {{ color: {fg}; }}
.reader-status .numeric {{ font-feature-settings: "tnum"; }}

.card {{
  background-color: {bg};
  color: {fg};
  border: {hairline}px solid {accent};
  padding: {popup}px;
}}
.card .title {{ font-size: {title}px; font-weight: bold; }}
.card .caption {{ font-size: {caption}px; color: {muted}; }}

.row {{
  padding: {md}px {row_x}px;
  background-color: {normal_fill};
  border: {nbw}px solid transparent;
}}
.row:hover, .row.cursor {{
  background-color: {hover_fill};
  border: {hbw}px solid {hover_border};
}}
.row.selected {{ background-color: {selected_fill}; color: {accent_text}; }}
.row:active {{ background-color: {pressed_fill}; }}

.empty {{ padding: {panel}px; }}
.empty .display {{ font-size: {display}px; font-weight: bold; }}
.empty .heading {{ font-size: {heading}px; }}
.empty .caption {{ font-size: {body_small}px; color: {muted}; }}
.empty .key {{
  font-size: {body_small}px;
  padding: 0 {sm}px;
  border: {hairline}px solid {rule};
  color: {accent_text};
}}
.empty .gap {{ margin-top: {lg}px; }}

.toast {{
  background-color: {bg};
  color: {fg};
  border: {hairline}px solid {accent};
  padding: {md}px {row_x}px;
  margin: {panel}px;
  font-size: {body_small}px;
}}
.toast.error {{ border-color: {urgent}; }}
"#,
        muted = t.muted,
        accent = t.accent,
        accent_text = t.accent.lifted(bg, fg, 4.5),
        urgent = t.urgent,
        rule = fg.alpha(c.normal_border_alpha * 0.5),
        normal_fill = fg.alpha(c.normal_fill_alpha),
        hover_fill = fg.alpha(c.hover_fill_alpha),
        selected_fill = fg.alpha(c.selected_fill_alpha),
        pressed_fill = fg.alpha(c.pressed_fill_alpha),
        hover_border = fg.alpha(c.hover_border_alpha),
        nbw = c.normal_border_width,
        hbw = c.hover_border_width,
    );
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redefines_the_platform_colours_from_the_theme() {
        let t = Theme::from_omarchy("x", "background = \"#282828\"\nforeground = \"#d4be98\"\naccent = \"#7daea3\"", "");
        let css = chrome_css(&t);
        assert!(css.contains("@define-color window_bg_color #282828;"));
        assert!(css.contains("@define-color accent_bg_color #7daea3;"));
        assert!(css.contains("font-size: 12px"));
        assert!(css.contains("border-radius: 0"));
    }

    #[test]
    fn sizes_follow_the_shell_base_size() {
        let t = Theme::from_omarchy("x", "background = \"#000000\"", "[font]\nbase-size = 16\n");
        assert!(chrome_css(&t).contains("font-size: 16px"));
    }
}
