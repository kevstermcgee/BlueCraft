//! Game-owned pause menu. Native pause, focus and neutral-device gates stay in GameShell.
use macroquad::prelude::*;
use vesper3d::viewer::{
    game_client::{AudioMenu, GameShell},
    game_input::ClientInput,
    game_text::{draw_text, measure_text},
};

const INK: Color = Color::from_rgba(233, 238, 226, 255);
const MUTED: Color = Color::from_rgba(165, 183, 174, 255);
const ACCENT: Color = Color::from_rgba(188, 216, 161, 255);

#[derive(Clone, Copy)]
pub struct Song<'a> {
    pub title: &'a str,
    pub duration_seconds: f32,
}

#[derive(Clone, Copy)]
pub struct MenuState<'a> {
    pub current_song: Option<usize>,
    /// A short, player-facing playback/loading/error description.
    pub playback_status: &'a str,
    pub can_download: bool,
    /// Export progress or the complete saved path; the library wraps it across lines.
    pub notice: Option<&'a str>,
}

pub struct MenuModel<'a> {
    pub title: &'a str,
    pub controls: &'a [&'a str],
    pub audio: AudioMenu,
    pub songs: &'a [Song<'a>],
    pub state: MenuState<'a>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuAction {
    pub resume: bool,
    pub quit: bool,
    pub toggle_music: bool,
    pub toggle_sfx: bool,
    pub download_song: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Main,
    Soundtrack,
    Controls,
    Settings,
}

#[derive(Default)]
pub struct MusicMenu {
    page: Page,
    selection: usize,
    was_paused: bool,
}

impl MusicMenu {
    /// Call after drawing the world. Apply `resume` to `shell.paused` in the host;
    /// the next shell frame performs the normal cursor/neutral transition.
    pub fn draw(&mut self, input: &ClientInput, shell: &GameShell, model: MenuModel<'_>) -> MenuAction {
        if !shell.paused {
            self.was_paused = false;
            return MenuAction::default();
        }
        if !self.was_paused {
            self.open(Page::Main);
            self.was_paused = true;
        }
        let allowed = shell.device_input_allowed();
        let rows = library_rows(screen_height());
        let mut confirm = false;
        if allowed {
            let step = input.menu_step();
            let next = step.down || input.pressed(KeyCode::Down) || input.pressed(KeyCode::Tab);
            let previous = step.up || input.pressed(KeyCode::Up);
            self.navigate(next, previous, self.row_count(model.songs.len()));
            if self.page == Page::Soundtrack {
                if step.right || input.pressed(KeyCode::Right) || input.pressed(KeyCode::PageDown) {
                    self.change_library_page(1, rows, model.songs.len());
                } else if step.left || input.pressed(KeyCode::Left) || input.pressed(KeyCode::PageUp) {
                    self.change_library_page(-1, rows, model.songs.len());
                }
            }
            if input.pressed(KeyCode::Backspace) {
                self.open(Page::Main);
            } else {
                confirm = input.menu_select() || input.pressed(KeyCode::Enter) || input.pressed(KeyCode::Space);
            }
        }
        self.selection = self.selection.min(self.row_count(model.songs.len()).saturating_sub(1));
        set_default_camera();
        draw_rectangle(0., 0., screen_width(), screen_height(), Color::from_rgba(5, 15, 18, 190));
        let layout = Layout::new(screen_width(), screen_height(), self.page, rows, model.controls.len());
        draw_rectangle(
            layout.panel.x,
            layout.panel.y,
            layout.panel.w,
            layout.panel.h,
            Color::from_rgba(24, 43, 43, 252),
        );
        draw_rectangle(layout.panel.x, layout.panel.y, 3. * layout.unit, layout.panel.h, ACCENT);
        let heading = match self.page {
            Page::Main => model.title,
            Page::Soundtrack => "BlueCraft soundtrack",
            Page::Controls => "Controls",
            Page::Settings => "Settings",
        };
        text_fit(
            heading,
            layout.left(),
            layout.top() + 29. * layout.unit,
            30. * layout.unit,
            layout.inner_width(),
            INK,
        );
        if !layout.compact {
            text_fit(
                "LOCAL WORLD PAUSED   Esc / Start / B: resume",
                layout.left(),
                layout.top() + 49. * layout.unit,
                12. * layout.unit,
                layout.inner_width(),
                MUTED,
            );
        }
        let mut clicked = None;
        match self.page {
            Page::Main => {
                self.draw_now_playing(&layout, &model);
                let labels = ["Resume", "Soundtrack", "Controls", "Settings", "Quit game"];
                let start = layout.content_y();
                let available = layout.panel.y + layout.panel.h - layout.pad - 30. * layout.unit - start;
                let gap = 6. * layout.unit;
                let row_h = ((available - gap * 4.) / 5.).min(44. * layout.unit);
                for (i, label) in labels.iter().enumerate() {
                    let row = Rect::new(layout.left(), start + i as f32 * (row_h + gap), layout.inner_width(), row_h);
                    let label = if i == 1 {
                        format!("Soundtrack  /  {} songs", model.songs.len())
                    } else {
                        (*label).to_owned()
                    };
                    if button(row, &label, self.selection == i, allowed, layout.unit) {
                        clicked = Some(i);
                    }
                }
                footer(&layout, "Esc / Start / B: resume     Arrows / stick: choose");
            }
            Page::Soundtrack => {
                self.draw_now_playing(&layout, &model);
                let range = visible_songs(self.selection, model.songs.len(), rows);
                let row_h = layout.song_row_height();
                for (offset, index) in range.clone().enumerate() {
                    let row = Rect::new(
                        layout.left(),
                        layout.content_y() + offset as f32 * (row_h + 4. * layout.unit),
                        layout.inner_width(),
                        row_h,
                    );
                    if song_button(
                        row,
                        index,
                        model.songs[index],
                        self.selection == index,
                        allowed && model.state.can_download,
                        layout.unit,
                    ) {
                        clicked = Some(index);
                    }
                }
                if model.songs.is_empty() {
                    text_fit(
                        "Soundtrack unavailable",
                        layout.left(),
                        layout.content_y() + 25. * layout.unit,
                        18. * layout.unit,
                        layout.inner_width(),
                        MUTED,
                    );
                }
                let nav_y = layout.panel.y + layout.panel.h - layout.pad - 34. * layout.unit;
                let nav_h = 32. * layout.unit;
                let third = (layout.inner_width() - 12. * layout.unit) / 3.;
                let prev = Rect::new(layout.left(), nav_y, third, nav_h);
                let next = Rect::new(prev.x + third + 6. * layout.unit, nav_y, third, nav_h);
                let back = Rect::new(next.x + third + 6. * layout.unit, nav_y, third, nav_h);
                let pages = model.songs.len().max(1).div_ceil(rows);
                if button(prev, "< Previous", false, allowed && pages > 1, layout.unit) {
                    self.change_library_page(-1, rows, model.songs.len());
                }
                if button(next, "Next >", false, allowed && pages > 1, layout.unit) {
                    self.change_library_page(1, rows, model.songs.len());
                }
                if button(back, "Back", self.selection == model.songs.len(), allowed, layout.unit) {
                    clicked = Some(model.songs.len());
                }
                let page = if range.is_empty() { 1 } else { range.start / rows + 1 };
                let info = model.state.notice.map(str::to_owned).unwrap_or_else(|| {
                    format!("Page {page} / {pages}. Select a song to download its original OGG. Left / right: pages. Backspace: back.")
                });
                let info_y = layout.content_y() + rows as f32 * (row_h + 4. * layout.unit) + 8. * layout.unit;
                wrapped_text(
                    &info,
                    Rect::new(layout.left(), info_y, layout.inner_width(), (nav_y - info_y - 4. * layout.unit).max(0.)),
                    (13. * layout.unit).max(10.),
                    MUTED,
                );
            }
            Page::Controls => {
                let start = layout.top() + if layout.compact { 54. } else { 90. } * layout.unit;
                for (i, line) in model.controls.iter().enumerate() {
                    text_fit(
                        line,
                        layout.left(),
                        start + i as f32 * 27. * layout.unit,
                        18. * layout.unit,
                        layout.inner_width(),
                        INK,
                    );
                }
                let back = layout.back_button();
                if button(back, "Back", true, allowed, layout.unit) {
                    clicked = Some(0);
                }
            }
            Page::Settings => {
                let labels = [
                    format!("Music: {}", if model.audio.music_on { "On" } else { "Off" }),
                    format!("Sound effects: {}", if model.audio.sfx_on { "On" } else { "Off" }),
                    "Back".to_owned(),
                ];
                let start = layout.top() + if layout.compact { 43. } else { 88. } * layout.unit;
                for (i, label) in labels.iter().enumerate() {
                    let row = Rect::new(
                        layout.left(),
                        start + i as f32 * 54. * layout.unit,
                        layout.inner_width(),
                        42. * layout.unit,
                    );
                    if button(row, label, self.selection == i, allowed, layout.unit) {
                        clicked = Some(i);
                    }
                }
                footer(&layout, "Backspace: back     Esc / Start / B: resume");
            }
        }
        if let Some(index) = clicked {
            self.selection = index;
            self.activate(model.songs.len(), model.state.can_download)
        } else if confirm {
            self.activate(model.songs.len(), model.state.can_download)
        } else {
            MenuAction::default()
        }
    }

    fn draw_now_playing(&self, layout: &Layout, model: &MenuModel<'_>) {
        let y = layout.now_y();
        let h = if layout.compact { 40. } else { 60. } * layout.unit;
        draw_rectangle(layout.left(), y, layout.inner_width(), h, Color::from_rgba(16, 31, 32, 255));
        let title = model
            .state
            .current_song
            .and_then(|i| model.songs.get(i))
            .map(|song| song.title)
            .unwrap_or("Preparing soundtrack...");
        let x = layout.left() + 12. * layout.unit;
        let width = layout.inner_width() - 24. * layout.unit;
        if !layout.compact {
            text_fit("NOW PLAYING", x, y + 15. * layout.unit, 11. * layout.unit, width, ACCENT);
        }
        text_fit(title, x, y + if layout.compact { 17. } else { 36. } * layout.unit, 20. * layout.unit, width, INK);
        text_fit(
            model.state.playback_status,
            x,
            y + if layout.compact { 33. } else { 52. } * layout.unit,
            12. * layout.unit,
            width,
            MUTED,
        );
    }

    fn open(&mut self, page: Page) {
        self.page = page;
        self.selection = 0;
    }

    fn row_count(&self, songs: usize) -> usize {
        match self.page {
            Page::Main => 5,
            Page::Soundtrack => songs + 1,
            Page::Controls => 1,
            Page::Settings => 3,
        }
    }

    fn navigate(&mut self, next: bool, previous: bool, count: usize) {
        if count == 0 || next == previous {
            return;
        }
        self.selection = if next { (self.selection + 1) % count } else { (self.selection + count - 1) % count };
    }

    fn change_library_page(&mut self, delta: i32, rows: usize, songs: usize) {
        if songs == 0 {
            return;
        }
        let pages = songs.div_ceil(rows);
        let page = self.selection.min(songs - 1) / rows;
        let target = (page as i32 + delta).rem_euclid(pages as i32) as usize;
        self.selection = (target * rows + self.selection.min(songs - 1) % rows).min(songs - 1);
    }

    fn activate(&mut self, songs: usize, can_download: bool) -> MenuAction {
        let mut action = MenuAction::default();
        match self.page {
            Page::Main => match self.selection {
                0 => action.resume = true,
                1 => self.open(Page::Soundtrack),
                2 => self.open(Page::Controls),
                3 => self.open(Page::Settings),
                4 => action.quit = true,
                _ => {}
            },
            Page::Soundtrack if self.selection < songs => {
                if can_download {
                    action.download_song = Some(self.selection);
                }
            }
            Page::Soundtrack | Page::Controls => self.open(Page::Main),
            Page::Settings => match self.selection {
                0 => action.toggle_music = true,
                1 => action.toggle_sfx = true,
                _ => self.open(Page::Main),
            },
        }
        action
    }
}

fn library_rows(height: f32) -> usize {
    if height >= 660. {
        10
    } else if height >= 460. {
        5
    } else if height >= 340. {
        3
    } else {
        2
    }
}

fn visible_songs(selection: usize, songs: usize, rows: usize) -> std::ops::Range<usize> {
    if songs == 0 {
        return 0..0;
    }
    let start = selection.min(songs - 1) / rows * rows;
    start..(start + rows).min(songs)
}

struct Layout {
    panel: Rect,
    unit: f32,
    pad: f32,
    compact: bool,
}

impl Layout {
    fn new(width: f32, height: f32, page: Page, rows: usize, controls: usize) -> Self {
        let unit = (width / 850.).min(height / 620.).clamp(0.6, 1.);
        let compact = height < 360.;
        let wanted_height = match page {
            Page::Main => 490. * unit,
            Page::Soundtrack => (if compact { 272. } else { 286. } + rows as f32 * 42.) * unit,
            Page::Controls => ((if compact { 106. } else { 144. }) + controls as f32 * 27.) * unit,
            Page::Settings => (if compact { 236. } else { 320. }) * unit,
        };
        let panel_width = (660. * unit).min(width - 24.).max(1.);
        let panel_height = wanted_height.min(height - 24.).max(1.);
        Self {
            panel: Rect::new((width - panel_width) * 0.5, (height - panel_height) * 0.5, panel_width, panel_height),
            unit,
            pad: 20. * unit,
            compact,
        }
    }

    fn left(&self) -> f32 {
        self.panel.x + self.pad
    }
    fn top(&self) -> f32 {
        self.panel.y + 6. * self.unit
    }
    fn inner_width(&self) -> f32 {
        self.panel.w - self.pad * 2.
    }
    fn now_y(&self) -> f32 {
        self.top() + if self.compact { 39. } else { 64. } * self.unit
    }
    fn content_y(&self) -> f32 {
        self.now_y() + if self.compact { 50. } else { 76. } * self.unit
    }
    fn song_row_height(&self) -> f32 {
        38. * self.unit
    }
    fn back_button(&self) -> Rect {
        Rect::new(
            self.left(),
            self.panel.y + self.panel.h - self.pad - 36. * self.unit,
            self.inner_width(),
            34. * self.unit,
        )
    }
}

fn button(rect: Rect, label: &str, selected: bool, enabled: bool, unit: f32) -> bool {
    let hover = enabled && rect.contains(vec2(mouse_position().0, mouse_position().1));
    draw_button_background(rect, selected, hover, enabled, unit);
    let color = if enabled { INK } else { MUTED };
    text_fit(label, rect.x + 12. * unit, rect.y + rect.h * 0.5 + 6. * unit, 19. * unit, rect.w - 24. * unit, color);
    hover && is_mouse_button_pressed(MouseButton::Left)
}

fn draw_button_background(rect: Rect, selected: bool, hover: bool, enabled: bool, unit: f32) {
    let fill = if hover {
        Color::from_rgba(66, 88, 76, 255)
    } else if selected {
        Color::from_rgba(53, 77, 67, 255)
    } else {
        Color::from_rgba(33, 55, 53, 255)
    };
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, fill);
    if selected {
        draw_rectangle_lines(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            (1.5 * unit).max(1.),
            if enabled { ACCENT } else { MUTED },
        );
    }
}

fn song_button(rect: Rect, index: usize, song: Song<'_>, selected: bool, enabled: bool, unit: f32) -> bool {
    let hover = enabled && rect.contains(vec2(mouse_position().0, mouse_position().1));
    draw_button_background(rect, selected, hover, enabled, unit);
    let baseline = rect.y + rect.h * 0.5 + 6. * unit;
    text_fit(&format!("{:02}", index + 1), rect.x + 9. * unit, baseline, 13. * unit, 23. * unit, MUTED);
    let action_width = 103. * unit;
    let duration_width = 47. * unit;
    let title_x = rect.x + 35. * unit;
    let duration_x = rect.x + rect.w - action_width - duration_width;
    text_fit(song.title, title_x, baseline, 18. * unit, (duration_x - title_x - 8. * unit).max(0.), INK);
    let duration = song.duration_seconds.max(0.).round() as u32;
    text_fit(
        &format!("{}:{:02}", duration / 60, duration % 60),
        duration_x,
        baseline,
        14. * unit,
        duration_width,
        MUTED,
    );
    text_fit(
        "Download",
        rect.x + rect.w - action_width,
        baseline,
        15. * unit,
        action_width - 8. * unit,
        if enabled { ACCENT } else { MUTED },
    );
    hover && is_mouse_button_pressed(MouseButton::Left)
}

fn footer(layout: &Layout, label: &str) {
    text_fit(
        label,
        layout.left(),
        layout.panel.y + layout.panel.h - 13. * layout.unit,
        12. * layout.unit,
        layout.inner_width(),
        MUTED,
    );
}

fn text_fit(text: &str, x: f32, y: f32, size: f32, width: f32, color: Color) {
    let size = size.max(10.);
    let fitted = fit_text(text, width, size);
    draw_text(&fitted, x, y, size, color);
}

fn fit_text(text: &str, width: f32, size: f32) -> String {
    if text_width(text, size) <= width {
        return text.to_owned();
    }
    let mut result = text.to_owned();
    while !result.is_empty() && text_width(&format!("{result}..."), size) > width {
        result.pop();
    }
    if result.is_empty() {
        String::new()
    } else {
        format!("{result}...")
    }
}

fn text_width(text: &str, size: f32) -> f32 {
    measure_text(text, None, 48, size / 48.).width
}

fn wrapped_text(text: &str, rect: Rect, size: f32, color: Color) {
    let line_height = size * 1.22;
    let count = (rect.h / line_height).floor() as usize;
    let mut remaining = text.trim();
    for row in 0..count {
        if remaining.is_empty() {
            break;
        }
        if row + 1 == count {
            text_fit(remaining, rect.x, rect.y + size + row as f32 * line_height, size, rect.w, color);
            break;
        }
        let mut end = 0;
        for (i, character) in remaining.char_indices() {
            let next = i + character.len_utf8();
            if text_width(&remaining[..next], size) > rect.w {
                break;
            }
            end = next;
        }
        // Split long paths too; a saved location must not disappear off the panel.
        let candidate = &remaining[..end];
        if end < remaining.len() {
            end = candidate.rfind(' ').filter(|&i| i > candidate.len() / 2).unwrap_or(end);
        }
        if end == 0 {
            break;
        }
        draw_text(remaining[..end].trim(), rect.x, rect.y + size + row as f32 * line_height, size, color);
        remaining = remaining[end..].trim_start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_track_can_be_downloaded_with_keyboard_or_controller_navigation() {
        let mut menu = MusicMenu { selection: 1, ..Default::default() };
        menu.activate(10, true);
        assert_eq!(menu.page, Page::Soundtrack);
        for i in 0..10 {
            assert_eq!(menu.activate(10, true).download_song, Some(i));
            menu.navigate(true, false, 11);
        }
        assert_eq!(menu.selection, 10);
        assert_eq!(menu.activate(10, true), MenuAction::default());
        assert_eq!(menu.page, Page::Main);
    }

    #[test]
    fn disabled_export_never_dispatches_and_resume_controls_settings_quit_remain_available() {
        let mut menu = MusicMenu::default();
        assert!(menu.activate(10, true).resume);
        menu.selection = 1;
        menu.activate(10, true);
        assert_eq!(menu.activate(10, false).download_song, None);
        menu.open(Page::Main);
        menu.selection = 2;
        menu.activate(10, true);
        assert_eq!(menu.page, Page::Controls);
        menu.activate(10, true);
        menu.selection = 3;
        menu.activate(10, true);
        assert!(menu.activate(10, true).toggle_music);
        menu.navigate(true, false, 3);
        assert!(menu.activate(10, true).toggle_sfx);
        menu.navigate(true, false, 3);
        menu.activate(10, true);
        menu.selection = 4;
        assert!(menu.activate(10, true).quit);
    }

    #[test]
    fn smaller_windows_page_the_full_library_without_hiding_the_last_track() {
        for height in [240., 360., 480., 720.] {
            let rows = library_rows(height);
            for selection in 0..10 {
                let range = visible_songs(selection, 10, rows);
                assert!(range.contains(&selection));
                assert!(range.end - range.start <= rows);
            }
            assert!(visible_songs(10, 10, rows).contains(&9));
            let mut menu = MusicMenu { page: Page::Soundtrack, selection: 0, ..Default::default() };
            let pages = 10usize.div_ceil(rows);
            for page in 0..pages {
                assert_eq!(menu.selection / rows, page);
                menu.change_library_page(1, rows, 10);
            }
            assert_eq!(menu.selection, 0);
            menu.change_library_page(-1, rows, 10);
            assert_eq!(menu.selection / rows, pages - 1);
        }
        assert_eq!(visible_songs(0, 0, 5), 0..0);
    }

    #[test]
    fn panels_and_library_rows_fit_supported_small_and_standard_windows() {
        for (width, height) in [(320., 240.), (640., 480.), (960., 540.), (1280., 720.), (1920., 1080.)] {
            let rows = library_rows(height);
            for page in [Page::Main, Page::Soundtrack, Page::Controls, Page::Settings] {
                let layout = Layout::new(width, height, page, rows, 7);
                assert!(layout.panel.x >= 0. && layout.panel.y >= 0.);
                assert!(layout.panel.right() <= width && layout.panel.bottom() <= height);
                assert!(layout.inner_width() > 0.);
                if page == Page::Soundtrack {
                    let list_bottom = layout.content_y() + rows as f32 * (layout.song_row_height() + 4. * layout.unit);
                    let buttons_top = layout.panel.bottom() - layout.pad - 34. * layout.unit;
                    assert!(
                        list_bottom + 8. * layout.unit < buttons_top,
                        "{width}x{height}: soundtrack rows overlap page buttons"
                    );
                    let notice_height = buttons_top - list_bottom - 12. * layout.unit;
                    let line_height = (13. * layout.unit).max(10.) * 1.22;
                    assert!(
                        notice_height >= line_height * 2.,
                        "{width}x{height}: export notices need two readable lines"
                    );
                }
            }
        }
    }
}
