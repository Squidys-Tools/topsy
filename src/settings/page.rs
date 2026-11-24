//! The settings page, the tray's third: its sections down the left, the chosen one's rows on the
//! right. It reads what it shows when it is turned to, and changes things through
//! `Island::update_settings`.

use gpui::{
    ClickEvent, Context, Div, FontWeight, MouseButton, Window, div, prelude::*, px, rgb, rgba,
};

use super::about::about;
use super::controls::{
    DIM, FILL, FILL_HOVER, button, dot, grid, keep_press, pressable, row, switch,
};
use crate::settings::Setting;
use crate::{Island, clipboard, droplets, update};
use model::droplets::Lineup;

/// The page's least width; its height is the tray's.
pub const WIDTH: f32 = 620.0;

const NAV_WIDTH: f32 = 92.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Notch,
    Widgets,
    Clipboard,
    General,
    About,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Notch,
        Section::Widgets,
        Section::Clipboard,
        Section::General,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Notch => "Notch",
            Section::Widgets => "Widgets",
            Section::Clipboard => "Clipboard",
            Section::General => "General",
            Section::About => "About",
        }
    }
}

/// What a change asks of the island, once it is saved.
pub enum Effect {
    Setting(Setting, bool),
    Lineup(Lineup),
    Redraw,
}

pub struct State {
    section: Section,
    on: [bool; Setting::ALL.len()],
    keep_days: u32,
    lineup: Lineup,
}

impl Default for State {
    fn default() -> Self {
        Self {
            section: Section::Notch,
            on: [false; Setting::ALL.len()],
            keep_days: clipboard::DEFAULT_DAYS,
            lineup: Lineup::new([]),
        }
    }
}

impl State {
    /// Read what the page shows from where it is kept: on turning to it, and after a change made
    /// elsewhere (the menu).
    pub fn reload(&mut self) {
        self.on = Setting::ALL.map(Setting::is_on);
        self.keep_days = clipboard::keep_days();
        self.lineup = droplets::saved();
    }

    pub fn show(&mut self, section: Section) -> Option<Effect> {
        self.section = section;
        Some(Effect::Redraw)
    }

    fn is_on(&self, setting: Setting) -> bool {
        (Setting::ALL.iter().position(|s| *s == setting)).is_some_and(|at| self.on[at])
    }

    fn toggle(&mut self, setting: Setting) -> Option<Effect> {
        let on = setting.toggle()?;
        if let Some(at) = Setting::ALL.iter().position(|s| *s == setting) {
            self.on[at] = on;
        }
        Some(Effect::Setting(setting, on))
    }

    fn keep(&mut self, days: u32) -> Option<Effect> {
        match clipboard::set_keep_days(days) {
            Ok(()) => self.keep_days = days,
            Err(err) => eprintln!("topsy: could not change how long history is kept: {err}"),
        }
        Some(Effect::Redraw)
    }

    fn change_lineup(&mut self, change: impl FnOnce(&mut Lineup)) -> Option<Effect> {
        let mut lineup = self.lineup.clone();
        change(&mut lineup);
        if let Err(err) = droplets::save(&lineup) {
            eprintln!("topsy: could not change the shelf widgets: {err}");
            return None;
        }
        self.lineup = lineup.clone();
        Some(Effect::Lineup(lineup))
    }

    /// The page, `height` tall.
    pub fn view(&self, height: f32, cx: &mut Context<Island>) -> Div {
        let content = match self.section {
            Section::Notch | Section::General => self.switch_grid(cx),
            Section::Widgets => self.widgets(cx),
            Section::Clipboard => self.clipboard(cx),
            Section::About => about(),
        };
        div()
            .w_full()
            .h(px(height))
            .flex()
            .gap(px(16.0))
            .py(px(12.0))
            .pl(px(14.0))
            .pr(px(22.0))
            .text_size(px(12.0))
            .on_mouse_down(MouseButton::Left, cx.listener(keep_press))
            .child(self.nav(cx))
            .child(div().flex_1().min_w_0().child(content))
    }

    fn nav(&self, cx: &mut Context<Island>) -> Div {
        let ready = update::ready().is_some();
        let items = Section::ALL.map(|section| {
            let current = section == self.section;
            pressable(
                div().id(("settings-section", section as usize)),
                cx,
                move |state| state.show(section),
            )
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(9.0))
            .py(px(4.0))
            .line_height(px(16.0))
            .rounded(px(9.0))
            .font_weight(FontWeight::SEMIBOLD)
            .map(|item| match current {
                true => item.bg(rgba(0xffffff24)),
                false => item
                    .text_color(rgba(0xffffff8c))
                    .hover(|item| item.text_color(rgb(0xffffff))),
            })
            .child(section.label())
            .children((section == Section::About && ready).then(dot))
        });
        div()
            .w(px(NAV_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(items)
    }

    /// The section's switches, two to a line, and General's Quit after them.
    fn switch_grid(&self, cx: &mut Context<Island>) -> Div {
        let section = self.section;
        let mut rows: Vec<Div> = (Setting::ALL.into_iter())
            .filter(|setting| setting.section() == section)
            .map(|setting| self.switch_row(setting, cx))
            .collect();
        if section == Section::General {
            let quit = button("settings-quit", "Quit", false).on_click(|_, _, cx| {
                cx.stop_propagation();
                cx.quit();
            });
            rows.push(row("Quit topsy", "Until you start it again", quit));
        }
        grid(rows)
    }

    fn switch_row(&self, setting: Setting, cx: &mut Context<Island>) -> Div {
        let on = self.is_on(setting);
        let toggle = pressable(div().id(("setting", setting as usize)), cx, move |state| {
            state.toggle(setting)
        });
        div().child(toggle.child(row(&setting.label(), setting.description(), switch(on))))
    }

    fn widgets(&self, cx: &mut Context<Island>) -> Div {
        let rows = self.lineup.rows(&droplets::ids());
        let (on, off): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(_, on)| *on);
        let on = on
            .into_iter()
            .enumerate()
            .map(|(at, (id, _))| {
                let pill = Pill { id, name: name(id) };
                pill.view(true)
                    .id(("widget-on", at))
                    .cursor_grab()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.update_settings(cx, |state, _| {
                            state.change_lineup(|lineup| lineup.set(id, false))
                        });
                    }))
                    .on_drag(pill, |pill, _, _, cx| cx.new(|_| *pill))
                    .drag_over::<Pill>(|pill, _, _, _| pill.bg(rgba(FILL_HOVER)))
                    .on_drop(cx.listener(move |this, dragged: &Pill, _, cx| {
                        let moved = dragged.id;
                        this.update_settings(cx, |state, _| {
                            state.change_lineup(|lineup| lineup.move_to(moved, at))
                        });
                    }))
            })
            .collect::<Vec<_>>();
        let off: Vec<_> = (off.into_iter().enumerate())
            .map(|(at, (id, _))| {
                pressable(div().id(("widget-off", at)), cx, move |state| {
                    state.change_lineup(|lineup| lineup.set(id, true))
                })
                .child(Pill { id, name: name(id) }.view(false))
            })
            .collect();
        let hint = |text: &'static str| {
            div()
                .text_size(px(10.5))
                .text_color(rgba(DIM))
                .mt(px(2.0))
                .mb(px(6.0))
                .child(text)
        };
        let pills = || div().flex().flex_wrap().gap(px(6.0));
        div()
            .flex()
            .flex_col()
            .child(hint(
                "Across the top of Home, left to right. Drag to reorder; click to take one out.",
            ))
            .child(pills().children(on))
            .when(!off.is_empty(), |page| {
                page.child(div().mt(px(8.0)).child(hint("Not shown")))
                    .child(pills().children(off))
            })
    }

    fn clipboard(&self, cx: &mut Context<Island>) -> Div {
        let segments = clipboard::DAY_CHOICES.map(|days| {
            let current = days == self.keep_days;
            pressable(div().id(("keep", days as usize)), cx, move |state| {
                state.keep(days)
            })
            .px(px(8.0))
            .py(px(3.0))
            .rounded_full()
            .text_size(px(11.0))
            .map(|segment| match current {
                true => segment
                    .bg(rgb(0xffffff))
                    .text_color(rgb(0x000000))
                    .font_weight(FontWeight::SEMIBOLD),
                false => segment.bg(rgba(0xffffff14)),
            })
            .child(days_label(days))
        });
        let keep = div().flex().flex_none().gap(px(3.0)).children(segments);
        let clear = button("settings-clear", "Clear", false).on_click(|_, _, cx| {
            cx.stop_propagation();
            clipboard::send(clipboard::Command::Clear);
        });
        div()
            .flex()
            .flex_col()
            .child(self.switch_row(Setting::Clipboard, cx))
            .child(row(
                "Keep history for",
                "Pinned entries are always kept",
                keep,
            ))
            .child(row(
                "Clear history",
                "Remove everything you haven't pinned",
                clear,
            ))
    }
}

impl Setting {
    /// The settings page's section that holds it.
    fn section(self) -> Section {
        match self {
            Setting::Black | Setting::LiveGlass | Setting::VolumeHud | Setting::LiveActivities => {
                Section::Notch
            }
            Setting::Clipboard => Section::Clipboard,
            Setting::Autostart | Setting::SendTo | Setting::Updates => Section::General,
        }
    }

    /// The line under its label on the settings page.
    fn description(self) -> &'static str {
        match self {
            Setting::Black => "Plain black instead of glass",
            Setting::LiveGlass => "Glass follows the screen behind it",
            Setting::Autostart => "Start topsy when you sign in",
            Setting::SendTo => "In Explorer's right-click menu",
            Setting::Clipboard => "Keep what you copy, to paste it again later",
            Setting::VolumeHud => "The volume keys' level, in the notch",
            Setting::LiveActivities => "Downloads and Bluetooth audio",
            Setting::Updates => "Newer nightlies, installed on restart",
        }
    }
}

/// A droplet in the Widgets section; dragged, it is its own preview.
#[derive(Clone, Copy)]
struct Pill {
    id: &'static str,
    name: &'static str,
}

impl Pill {
    fn view(self, on: bool) -> Div {
        let pill = div()
            .h(px(28.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded_full()
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .whitespace_nowrap();
        match on {
            true => pill.bg(rgba(FILL)).child(self.name),
            false => pill
                .border_1()
                .border_dashed()
                .border_color(rgba(0xffffff38))
                .text_color(rgba(DIM))
                .hover(|pill| pill.text_color(rgb(0xffffff)))
                .child(format!("+ {}", self.name)),
        }
    }
}

impl Render for Pill {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.view(true)
            .bg(rgba(FILL_HOVER))
            .text_color(rgb(0xffffff))
    }
}

fn name(id: &'static str) -> &'static str {
    droplets::entry(id).map_or(id, |entry| entry.name)
}

fn days_label(days: u32) -> String {
    match days {
        0 => "Forever".into(),
        1 => "1 day".into(),
        n => format!("{n} days"),
    }
}
