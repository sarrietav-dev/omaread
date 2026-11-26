//! The chrome's GTK stylesheet, generated from a [`Theme`].
//!
//! Sizes are Style.qml's type scale and spacing ladder, colours its state
//! rules (a tint of the foreground at the theme's alphas, hairline borders,
//! no rounding), so overlays read like the rest of the Omarchy shell.
//! libadwaita's named colours are redefined too, so any stock widget we use
//! wears the same palette.

use std::fmt::Write as _;

use crate::theme::{Theme, GLASS_ALPHA};

/// The chrome's stylesheet. When Omarchy's Glassy Windows is on, every surface
/// the CSS paints carries [`GLASS_ALPHA`] instead of the palette's solid
/// background, so the compositor's blur shows through and stacked panels stay
/// readable. The book page is cleared separately (see `book_palette`).
pub fn chrome_css(t: &Theme, glassy: bool) -> String {
    let fg = t.foreground;
    let bg = t.background;
    let bg_css = if glassy { bg.alpha(GLASS_ALPHA) } else { bg.to_string() };
    // Full-page surfaces stay clear in glassy mode so the window paints the
    // single frosted layer; panels drawn over them stack their own alpha.
    let page_bg = if glassy { "transparent".to_string() } else { bg_css.clone() };
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
        ("window_bg_color", bg_css.clone()),
        ("window_fg_color", fg.to_string()),
        ("view_bg_color", bg_css.clone()),
        ("view_fg_color", fg.to_string()),
        ("headerbar_bg_color", bg_css.clone()),
        ("headerbar_fg_color", fg.to_string()),
        ("popover_bg_color", bg_css.clone()),
        ("popover_fg_color", fg.to_string()),
        ("dialog_bg_color", bg_css.clone()),
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
  background-color: {bg_css};
  color: {fg};
  font-family: monospace;
  font-size: {body}px;
}}
window.omaread * {{ border-radius: 0; }}

.reader-status {{
  font-size: {body_small}px;
  color: {muted};
  padding: {sm}px {row_x}px;
  background-color: {bg_css};
  border-top: {hairline}px solid {rule};
}}
.reader-status .section {{ color: {fg}; }}
.reader-status .numeric {{ font-feature-settings: "tnum"; }}

.card {{
  background-color: {bg_css};
  color: {fg};
  border: {hairline}px solid {accent};
  padding: {popup}px;
}}
/* Images can be transparent: show them on the theme's solid background. */
.card.image-view {{ background-color: {bg}; }}
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
  background-color: {bg_css};
  color: {fg};
  border: {hairline}px solid {accent};
  padding: {md}px {row_x}px;
  margin: {panel}px;
  font-size: {body_small}px;
}}
.toast.error {{ border-color: {urgent}; }}

/* Panels over the book */
.side-panel {{
  min-width: {panel_w}px;
  margin: {lg}px;
  margin-bottom: {panel_bottom}px;
}}
.narrow .side-panel, .narrow .annotations-panel {{ min-width: 0; }}
list.settings-list {{ background: none; font-size: {body_small}px; }}
list.settings-list > row {{
  padding: {xs}px {sm}px;
  border: {nbw}px solid transparent;
  border-left: {thick}px solid transparent;
}}
list.settings-list > row:hover {{ background-color: {hover_fill}; }}
list.settings-list > row:selected {{
  background-color: {selected_fill};
  border-left-color: {accent};
}}
list.settings-list > row:selected .value {{ color: {accent_text}; }}
list.settings-list .name {{ color: {fg}; }}
list.settings-list .value {{ color: {muted}; font-feature-settings: "tnum"; }}
list.settings-list .value.on, list.settings-list .value.action {{ color: {accent_text}; }}
list.settings-list > row.settings-heading {{ padding: {md}px {sm}px 0; }}
list.settings-list .settings-heading {{
  font-size: {caption}px;
  color: {muted};
  letter-spacing: 1px;
  text-transform: uppercase;
}}
.scope-chips .chip {{
  font-size: {caption}px;
  color: {muted};
  padding: 1px {sm}px;
  border: {hairline}px solid {rule};
}}
.scope-chips .chip.active {{ color: {accent_text}; border-color: {accent}; }}
progressbar.range > trough {{
  min-height: {progress}px;
  background-color: {bg_alpha};
  border-radius: 0;
}}
progressbar.range > trough > progress {{
  min-height: {progress}px;
  background-color: {accent};
  border-radius: 0;
}}
progressbar.range > text {{ color: transparent; }}
.settings-note {{ font-size: {caption}px; color: {muted}; }}
.scope-label {{ color: {muted}; }}

listview.toc-list {{ background: none; font-size: {body_small}px; }}
listview.toc-list > row {{
  padding: {xs}px {sm}px;
  color: {fg};
  border: {nbw}px solid transparent;
}}
listview.toc-list > row:hover {{ background-color: {hover_fill}; }}
listview.toc-list > row:selected {{
  background-color: {selected_fill};
  color: {accent_text};
}}
listview.toc-list expander {{ min-width: {body_small}px; color: {muted}; }}

button.swatch {{
  min-width: 22px;
  min-height: 22px;
  padding: 0;
  margin: 0;
  border: 1px solid {rule};
  border-radius: 0;
  background-image: none;
  box-shadow: none;
  outline: none;
}}
button.swatch.red {{ background-color: {hl_red}; }}
button.swatch.yellow {{ background-color: {hl_yellow}; }}
button.swatch.green {{ background-color: {hl_green}; }}
button.swatch.blue {{ background-color: {hl_blue}; }}
button.swatch.magenta {{ background-color: {hl_magenta}; }}
button.swatch:hover {{ border-color: {fg}; }}
button.swatch.active {{ border: 2px solid {accent_text}; box-shadow: 0 0 0 1px {bg_css}; }}
entry.selection-note {{
  background-color: {normal_fill};
  color: {fg};
  border: {hairline}px solid {rule};
  padding: {xs}px {md}px;
  min-height: 0;
  box-shadow: none;
  outline: none;
  font-size: {body_small}px;
}}
entry.selection-note:focus {{ border-color: {accent}; }}
.selection-bar button.action, .selection-bar button.primary {{
  padding: {xs}px {md}px;
  min-height: 0;
  background-color: {normal_fill};
  background-image: none;
  border: {hairline}px solid {rule};
  box-shadow: none;
  outline: none;
  font-size: {body_small}px;
  color: {fg};
}}
.selection-bar button.action:hover, .selection-bar button.primary:hover {{
  background-color: {hover_fill};
  border-color: {hover_border};
}}
.selection-bar button.primary {{ color: {accent_text}; font-weight: bold; }}
.card .excerpt {{ font-family: serif; font-size: {body}px; font-style: italic; color: {fg}; }}
textview.note-text, textview.note-text text {{
  background-color: {normal_fill};
  color: {fg};
  font-size: {body_small}px;
  padding: {sm}px;
}}
textview.note-text {{ border: {hairline}px solid {rule}; }}
list.annotation-list {{ background: none; }}
list.annotation-list > row {{
  padding: {md}px {sm}px;
  border: {nbw}px solid transparent;
  border-bottom: {hairline}px solid {rule};
}}
list.annotation-list > row:selected {{ background-color: {selected_fill}; }}
list.annotation-list .excerpt {{ font-size: {body_small}px; padding-left: {md}px; border-left: {thick}px solid {hl_yellow}; }}
list.annotation-list .note {{ font-size: {body_small}px; color: {muted}; padding-left: {md}px; }}
list.annotation-list .swatch-red {{ border-left-color: {hl_red}; }}
list.annotation-list .swatch-green {{ border-left-color: {hl_green}; }}
list.annotation-list .swatch-blue {{ border-left-color: {hl_blue}; }}
list.annotation-list .swatch-magenta {{ border-left-color: {hl_magenta}; }}
list.annotation-list .swatch-underline, list.annotation-list .swatch-squiggly,
list.annotation-list .swatch-strikethrough {{ border-left-color: {muted}; }}
.annotations-panel {{ min-width: {panel_wide}px; }}
label.tts-bar {{
  background-color: {bg_css};
  color: {fg};
  border: {hairline}px solid {accent};
  padding: {xs}px {row_x}px;
  margin-bottom: {panel_bottom}px;
  font-size: {caption}px;
}}
.speech-player {{
  background-color: {bg_css};
  border-top: {hairline}px solid {rule};
  padding: {md}px {row_x}px;
}}
.speech-panel {{ min-width: 0; }}
.speech-title {{ color: {fg}; font-size: {body_small}px; }}
.speech-detail {{ color: {muted}; font-size: {caption}px; }}
.speech-speed {{
  min-width: 42px;
  color: {accent_text};
  font-feature-settings: "tnum";
  font-size: {caption}px;
  padding: 0 {xs}px;
}}
.speech-player menubutton.speech-speed {{
  min-height: 30px;
  min-width: 56px;
  padding: 0 {sm}px;
  background-color: {normal_fill};
  border: {hairline}px solid {accent};
  color: {accent_text};
}}
/* The menu button draws the frame; its internal button must not draw another. */
.speech-player menubutton.speech-speed > button {{
  min-height: 0;
  min-width: 0;
  padding: 0;
  background: none;
  background-image: none;
  border: none;
  box-shadow: none;
}}
.speech-player menubutton.speech-speed:hover {{ background-color: {hover_fill}; }}
popover.speech-presets {{
  background: none;
  background-color: transparent;
  border: none;
  box-shadow: none;
}}
popover.speech-presets > contents {{
  background-color: {bg_css};
  border: {hairline}px solid {accent};
  box-shadow: none;
  padding: {xs}px;
}}
.speech-presets button {{
  min-height: 0;
  padding: {xs}px {md}px;
  background: none;
  background-image: none;
  border: none;
  box-shadow: none;
  color: {fg};
  font-size: {body_small}px;
}}
.speech-presets button:hover {{ background-color: {hover_fill}; color: {accent_text}; }}
.speech-player button {{
  min-height: 30px;
  min-width: 30px;
  padding: 0 {sm}px;
  background-image: none;
  background-color: {normal_fill};
  border: {hairline}px solid {rule};
  box-shadow: none;
  color: {fg};
}}
.speech-player button image {{ -gtk-icon-size: 16px; }}
.speech-player button:hover {{ background-color: {hover_fill}; border-color: {hover_border}; }}
.speech-player button:focus-visible {{ outline: {hairline}px solid {accent}; outline-offset: 2px; }}
.speech-player button.speech-primary {{
  color: {accent_text};
  border-color: {accent};
  background-color: {selected_fill};
}}
.voice-panel {{ min-width: {panel_w}px; }}
.voice-status {{ font-size: {body_small}px; color: {fg}; }}
.voice-status.playing {{ color: {accent_text}; }}
.voice-status.paused {{ color: {muted}; }}
.voice-section {{ font-size: {caption}px; color: {muted}; letter-spacing: 1px; text-transform: uppercase; }}
.voice-speed {{ font-size: {body_small}px; }}
.voice-speed .value {{ color: {muted}; font-feature-settings: "tnum"; }}
.voice-panel button {{
  min-height: 0;
  min-width: 0;
  padding: 0 {sm}px;
  background-color: {normal_fill};
  background-image: none;
  border: {hairline}px solid {rule};
  box-shadow: none;
  outline: none;
  color: {fg};
}}
.voice-panel button:hover {{ background-color: {hover_fill}; border-color: {hover_border}; }}
list.voice-list {{ background: none; font-size: {body_small}px; }}
list.voice-list > row {{
  padding: {xs}px {sm}px;
  border: {nbw}px solid transparent;
  border-left: {thick}px solid transparent;
}}
list.voice-list > row:hover {{ background-color: {hover_fill}; }}
list.voice-list > row:selected {{ background-color: {selected_fill}; }}
list.voice-list > row.active {{ border-left-color: {accent}; }}
list.voice-list > row:selected .voice-name {{ color: {accent_text}; }}
list.voice-list .voice-name {{ color: {fg}; }}
list.voice-list .voice-lang {{ color: {muted}; font-size: {caption}px; }}
list.voice-list .voice-marker {{ color: {accent_text}; }}
.lookup-tabs {{ font-size: {caption}px; color: {muted}; padding-bottom: {sm}px; border-bottom: {hairline}px solid {rule}; }}
.lookup-tabs .tab.active {{ color: {accent_text}; }}
.lookup-body {{ font-size: {body_small}px; color: {fg}; }}
.lookup-body.error {{ color: {muted}; }}

entry.panel-entry {{
  padding: {xs}px {md}px;
  min-height: 0;
  background-color: {normal_fill};
  color: {fg};
  border: {hairline}px solid {accent};
  box-shadow: none;
  outline: none;
  font-size: {body_small}px;
}}
.palette {{ margin-bottom: {panel}px; }}
list.palette-list {{ background: none; font-size: {body_small}px; }}
list.palette-list > row {{ padding: {xs}px {sm}px; border: {nbw}px solid transparent; }}
list.palette-list > row:selected {{ background-color: {selected_fill}; color: {accent_text}; }}
list.palette-list .key, .search-panel .key {{ color: {muted}; font-size: {caption}px; }}
list.search-list {{ background: none; font-size: {body_small}px; }}
list.search-list > row {{ padding: {xs}px {sm}px; border: {nbw}px solid transparent; }}
list.search-list > row.header {{ padding-top: {md}px; }}
list.search-list > row:selected {{ background-color: {selected_fill}; }}
list.search-list > row:selected b {{ color: {accent_text}; }}

/* The `?` help */
.help .heading {{
  font-size: {body_small}px;
  font-weight: bold;
  color: {accent_text};
  margin-top: {md}px;
}}
.help .key {{
  min-width: {help_key}px;
  color: {accent_text};
  font-feature-settings: "tnum";
}}
.help .what {{ color: {fg}; }}

/* Library */
.library, .library-main, .library-side {{ background-color: {page_bg}; color: {fg}; }}
.library-side {{
  min-width: {rail}px;
  border-right: {hairline}px solid {rule};
}}
.library-rail {{ background: none; padding: {sm}px 0; }}
.library-rail row.shelf-row {{
  padding: {xs}px {row_x}px;
  font-size: {body_small}px;
  color: {muted};
  border: {nbw}px solid transparent;
}}
.library-rail row.shelf-row:hover {{ background-color: {hover_fill}; }}
.library-rail row.shelf-row:selected {{
  background-color: {selected_fill};
  color: {accent_text};
}}
.library-rail row.shelf-row.special {{ color: {fg}; }}
.library-rail row.shelf-row.special:selected {{ color: {accent_text}; }}
.library-rail .count {{ font-size: {caption}px; color: {muted}; font-feature-settings: "tnum"; }}
.library-rail row.shelf-row.root {{ color: {fg}; }}
.library-rail row.shelf-row.root:selected {{ color: {accent_text}; }}
.library-rail button.fold,
.library-rail button.remove {{
  background: none;
  border: none;
  box-shadow: none;
  padding: 0;
  min-width: {body_small}px;
  min-height: 0;
  color: {muted};
  font-size: {body_small}px;
}}
.library-rail button.fold:disabled,
.library-rail button.remove:disabled {{ opacity: 0; }}
.library-rail button.fold:hover,
.library-rail button.remove:hover {{ color: {accent_text}; }}
.library-rail button.remove {{ opacity: 0; }}
.library-rail row.shelf-row:hover button.remove:not(:disabled),
.library-rail row.shelf-row:selected button.remove:not(:disabled) {{ opacity: 1; }}
button.library-add {{
  margin: {sm}px {row_x}px;
  padding: {xs}px {md}px;
  background: none;
  border: {hairline}px solid {rule};
  box-shadow: none;
  color: {muted};
  font-size: {body_small}px;
  min-height: 0;
}}
button.library-add:hover {{ color: {accent_text}; border-color: {accent}; background-color: {hover_fill}; }}

.library-header {{
  padding: {md}px {row_x}px;
  border-bottom: {hairline}px solid {rule};
  font-size: {body_small}px;
}}
.library-title {{ color: {fg}; font-size: {title}px; font-weight: bold; }}
.library-hint {{ color: {muted}; font-size: {caption}px; }}
.library-header button {{
  padding: {xs}px {md}px;
  min-height: 28px;
  background-image: none;
  background-color: {normal_fill};
  border: {hairline}px solid {rule};
  box-shadow: none;
  color: {fg};
}}
.library-header button:hover {{ background-color: {hover_fill}; border-color: {hover_border}; }}
.library-header button:focus-visible {{ outline: {hairline}px solid {accent}; outline-offset: 2px; }}
.library-filter {{
  margin: {md}px {row_x}px 0;
  padding: {xs}px {md}px;
  background-color: {normal_fill};
  border: {hairline}px solid {accent};
  color: {fg};
  font-size: {body_small}px;
  min-height: 0;
  box-shadow: none;
  outline: none;
}}
.library-empty {{ color: {muted}; font-size: {body_small}px; }}
.continue-row {{
  padding: {md}px {row_x}px;
  border-bottom: {hairline}px solid {rule};
}}
.continue-row .heading {{ color: {muted}; font-size: {caption}px; }}
button.continue-item {{
  padding: {xs}px;
  background-image: none;
  background-color: transparent;
  border: {hbw}px solid transparent;
  box-shadow: none;
  color: {fg};
}}
button.continue-item:hover {{ background-color: {hover_fill}; border-color: {hover_border}; }}
button.continue-item:focus-visible {{ outline: {hairline}px solid {accent}; outline-offset: 2px; }}
.continue-item .cover {{ background-color: {surface}; }}
.continue-item .caption {{ color: {muted}; font-size: {caption}px; }}
.continue-item .title {{ font-size: {caption}px; }}

gridview.library-grid {{ background: none; padding: {lg}px {sm}px; }}
gridview.library-grid > child {{
  padding: {lg}px;
  margin: {xs}px;
  border: {hbw}px solid transparent;
}}
gridview.library-grid > child:hover {{ background-color: {hover_fill}; }}
gridview.library-grid > child:selected {{
  background-color: {selected_fill};
  border-color: {accent};
}}
.book .cover {{ background-color: {surface}; }}
.book .cover-placeholder {{
  font-size: {body_small}px;
  color: {muted};
  padding: {lg}px;
}}
.book .book-title {{ font-size: {body_small}px; color: {fg}; font-weight: bold; }}
.book .book-author {{ font-size: {caption}px; color: {muted}; }}
.book .book-reading {{ font-size: {caption}px; color: {accent_text}; font-feature-settings: "tnum"; }}
.book.unreadable .book-author {{ color: {urgent}; }}
.book.unreadable .cover {{ opacity: 0.5; }}
progressbar.book-progress > trough {{
  min-height: {progress}px;
  background-color: {bg_alpha};
  border-radius: 0;
}}
progressbar.book-progress > trough > progress {{
  min-height: {progress}px;
  background-color: {accent};
  border-radius: 0;
}}
"#,
        muted = t.muted,
        accent = t.accent,
        surface = t.surface,
        bg_css = bg_css,
        page_bg = page_bg,
        bg_alpha = bg.alpha(0.6),
        xs = t.space(3.0),
        rail = t.space(200.0),
        panel_w = t.space(340.0),
        panel_wide = t.space(420.0),
        thick = t.space(3.0).max(2),
        hl_red = t.highlights[0],
        hl_yellow = t.highlights[1],
        hl_green = t.highlights[2],
        hl_blue = t.highlights[3],
        hl_magenta = t.highlights[4],
        panel_bottom = t.space(40.0),
        progress = t.space(3.0).max(2),
        help_key = t.space(132.0),
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
        let css = chrome_css(&t, false);
        assert!(css.contains("@define-color window_bg_color #282828;"));
        assert!(css.contains("@define-color accent_bg_color #7daea3;"));
        assert!(css.contains("font-size: 12px"));
        assert!(css.contains("border-radius: 0"));
    }

    #[test]
    fn sizes_follow_the_shell_base_size() {
        let t = Theme::from_omarchy("x", "background = \"#000000\"", "[font]\nbase-size = 16\n");
        assert!(chrome_css(&t, false).contains("font-size: 16px"));
    }

    #[test]
    fn glassy_surfaces_are_see_through() {
        let t = Theme::from_omarchy("x", "background = \"#282828\"\nforeground = \"#d4be98\"", "");
        let glass = chrome_css(&t, true);
        assert!(glass.contains("@define-color window_bg_color rgba(40, 40, 40, 0.650);"));
        assert!(glass.contains("window.omaread {\n  background-color: rgba(40, 40, 40, 0.650);"));
        // Panels stack their alpha over the window's, staying readable.
        assert!(glass.contains(".card {\n  background-color: rgba(40, 40, 40, 0.650);"));
        // The library leaves the single frosted layer to the window.
        assert!(glass.contains(".library, .library-main, .library-side { background-color: transparent;"));
        // Only the image viewer stays solid: images can be transparent.
        let solid: Vec<&str> = glass.lines().filter(|l| l.contains("#282828")).collect();
        assert_eq!(solid, [".card.image-view { background-color: #282828; }"]);
        // Opaque mode keeps solid fills.
        assert!(chrome_css(&t, false).contains("background-color: #282828"));
    }
}
