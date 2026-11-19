use crate::state::InputMode;
use anyhow::{Context, Result};
use eframe::egui::{Context as EguiContext, ViewportId};
use std::sync::mpsc::{self, Receiver};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Menu choices surfaced to the app each frame.
pub enum TrayAction {
    Settings,
    Mode(InputMode),
    ToggleAutoPaste,
    CopyLastResult,
}

pub struct TrayController {
    _tray_icon: TrayIcon,
    settings_id: MenuId,
    code_id: MenuId,
    direct_id: MenuId,
    interview_id: MenuId,
    paste_id: MenuId,
    events: Receiver<MenuEvent>,
    code_item: CheckMenuItem,
    direct_item: CheckMenuItem,
    interview_item: CheckMenuItem,
    paste_item: CheckMenuItem,
    copy_item: MenuItem,
}

impl TrayController {
    pub fn install(
        mode: InputMode,
        auto_paste: bool,
        tooltip: &str,
        ctx: &EguiContext,
        request_close: fn(&EguiContext),
    ) -> Result<Self> {
        let settings = MenuItem::with_id("lite.settings", "设置…", true, None);
        let code = CheckMenuItem::with_id(
            "lite.code",
            "代码模式（AI 整理）",
            true,
            mode == InputMode::CodeEdit,
            None,
        );
        let direct = CheckMenuItem::with_id(
            "lite.direct",
            "普通口述",
            true,
            mode == InputMode::DirectPrompt,
            None,
        );
        let interview = CheckMenuItem::with_id(
            "lite.interview",
            "需求访谈（逐步提问）",
            true,
            mode == InputMode::Interview,
            None,
        );
        let paste = CheckMenuItem::with_id("lite.paste", "自动粘贴", true, auto_paste, None);
        let copy = MenuItem::with_id("lite.copy", "复制上次结果", false, None);
        let quit = MenuItem::with_id("lite.quit", "退出", true, None);
        let separator = PredefinedMenuItem::separator();
        let menu = Menu::new();
        menu.append_items(&[
            &settings,
            &separator,
            &direct,
            &code,
            &interview,
            &paste,
            &copy,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .context("构建托盘菜单失败。")?;

        let icon = load_tray_icon().context("创建托盘图标失败。")?;
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip(tooltip)
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .context("初始化系统托盘失败。")?;

        let (send, events) = mpsc::channel();
        let quit_id = quit.id().clone();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            crate::config::lifecycle(&format!("menu {}", event.id.0));
            if event.id == quit_id {
                request_close(&ctx);
            } else if send.send(event).is_ok() {
                ctx.request_repaint_of(ViewportId::ROOT);
            }
        }));
        crate::config::lifecycle("tray handler installed");

        Ok(Self {
            _tray_icon: tray_icon,
            settings_id: settings.id().clone(),
            code_id: code.id().clone(),
            direct_id: direct.id().clone(),
            interview_id: interview.id().clone(),
            paste_id: paste.id().clone(),
            events,
            code_item: code,
            direct_item: direct,
            interview_item: interview,
            paste_item: paste,
            copy_item: copy,
        })
    }

    pub fn set_mode(&self, mode: InputMode) {
        self.code_item.set_checked(mode == InputMode::CodeEdit);
        self.direct_item
            .set_checked(mode == InputMode::DirectPrompt);
        self.interview_item
            .set_checked(mode == InputMode::Interview);
    }

    pub fn set_auto_paste(&self, on: bool) {
        self.paste_item.set_checked(on);
    }

    pub fn enable_copy(&self) {
        self.copy_item.set_enabled(true);
        self.copy_item.set_text("复制上次结果");
    }

    pub fn show_copy_status(&self, copied: bool) {
        self.copy_item.set_text(if copied {
            "复制上次结果（已复制）"
        } else {
            "复制上次结果（剪贴板不可用，点此重试）"
        });
    }

    pub fn poll(&self) -> Option<TrayAction> {
        while let Ok(event) = self.events.try_recv() {
            if event.id == self.settings_id {
                return Some(TrayAction::Settings);
            }
            if event.id == self.code_id {
                return Some(TrayAction::Mode(InputMode::CodeEdit));
            }
            if event.id == self.direct_id {
                return Some(TrayAction::Mode(InputMode::DirectPrompt));
            }
            if event.id == self.interview_id {
                return Some(TrayAction::Mode(InputMode::Interview));
            }
            if event.id == self.paste_id {
                return Some(TrayAction::ToggleAutoPaste);
            }
            if event.id == self.copy_item.id() {
                return Some(TrayAction::CopyLastResult);
            }
        }
        None
    }
}

fn load_tray_icon() -> Result<Icon> {
    let icon_data = crate::icons::tray();
    Icon::from_rgba(icon_data.rgba.clone(), icon_data.width, icon_data.height)
        .context("生成托盘 RGBA 图标失败。")
}
