use eframe::egui::IconData;

pub fn window() -> IconData {
    IconData {
        rgba: include_bytes!("../assets/icon-128.rgba").to_vec(),
        width: 128,
        height: 128,
    }
}

pub fn tray() -> IconData {
    IconData {
        rgba: include_bytes!("../assets/icon-32.rgba").to_vec(),
        width: 32,
        height: 32,
    }
}
