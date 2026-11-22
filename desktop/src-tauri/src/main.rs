// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // GTK3 (our windowing toolkit) misplaces client-side decoration input regions
    // under Wayland fractional scaling — the native minimize/close buttons render
    // but clicks land on the wrong logical pixel. The X11 backend under XWayland
    // doesn't have the bug, and GNOME's xwayland-native-scaling keeps it crisp.
    // Only pinned when an X server is reachable and the user hasn't chosen a backend.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
    desktop_lib::run()
}
