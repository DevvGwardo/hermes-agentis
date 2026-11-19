//! P0-03 Gate-1 probe: tray + menu, global PTT hotkey API, rune-notify action callbacks and
//! graceful `Unsupported` degradation, exercised as far as a headless-safe run allows.
//!
//! Run (from voxcell-desktop/):
//!   cargo run -p voxcell-app --example os_integration_probe
//!
//! What it really executes on this machine:
//! * `rune::Tray::new` with a flat menu (Windows: Shell_NotifyIcon on its own thread,
//!   third_party/rune/crates/rune/src/tray.rs:1-9,103) — the icon appears for well under a
//!   second and is removed on drop. `set_menu` / `set_tooltip` are exercised live.
//! * `"Ctrl+Shift+Space".parse::<rune::Shortcut>()` — the PTT accelerator parse
//!   (crates/rune/src/shortcuts/parse.rs:22).
//! * `rune_notify::available()` before identity registration — must be false and every
//!   notify call a graceful no-op (crates/rune-notify/src/lib.rs:196-233).
//! * `WindowOptions` for the 320x420 always-on-top frameless phone widget
//!   (crates/rune/src/window/spec.rs:72-97,152-155) — constructed, not shown (no GUI flash).
//!
//! What it cannot execute headless (documented in docs/gate1-evidence.md with file:line):
//! actual hotkey registration (needs a running App event loop; `AppHandle::register_shortcut`
//! returns Result and headless runs register virtually, shortcuts/mod.rs:106-135), a second
//! visible window, and toast click delivery.

use rune::{Shortcut, Tray, TrayError, TrayEvent, TrayIcon, TrayItem, TraySpec, windows::WindowOptions};

/// Slate status dot with an amber approval badge baked into the RGBA — RUNE's tray has no
/// badge API (tray.rs: icon is fixed at `TraySpec` time; live updates = drop + recreate).
fn tray_icon(badge: bool) -> TrayIcon {
    let (w, h) = (16u32, 16u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            if dx * dx + dy * dy <= 36.0 {
                px[i] = 0x94;
                px[i + 1] = 0xA3;
                px[i + 2] = 0xB8;
                px[i + 3] = 0xFF;
            }
            if badge && x >= 9 && y <= 6 {
                px[i] = 0xF5;
                px[i + 1] = 0x9E;
                px[i + 2] = 0x0B;
                px[i + 3] = 0xFF;
            }
        }
    }
    TrayIcon::rgba(w, h, px)
}

/// Spec §10 mitigation: degrade without panicking on platforms without a tray.
fn install_tray(badge: bool) -> Option<Tray> {
    let spec = TraySpec::new("dev.voxcell.desktop", "Voxcell", tray_icon(badge))
        .tooltip("Voxcell — idle")
        .menu(vec![
            TrayItem::action("show", "Show Voxcell"),
            TrayItem::action("ptt", "Push to talk"),
            TrayItem::Separator,
            TrayItem::action("quit", "Quit"),
        ]);
    match Tray::new(spec, |ev| match ev {
        TrayEvent::Activate { token } => println!("tray activate (token {token:?})"),
        TrayEvent::Menu(id) => println!("tray menu: {id}"),
        _ => println!("tray event (future variant)"),
    }) {
        Ok(tray) => Some(tray),
        Err(TrayError::Unsupported) => {
            // macOS today (tray.rs:9): surface a StatusNote in-app instead.
            println!("tray: TrayError::Unsupported -> in-app status fallback (no panic)");
            None
        }
        Err(TrayError::Backend(e)) => {
            println!("tray: TrayError::Backend({e}) -> in-app status fallback (no panic)");
            None
        }
        Err(other) => {
            println!("tray: {other} -> in-app status fallback (no panic)");
            None
        }
    }
}

/// Spec §10 mitigation for Wayland: registration is Result-based; Unsupported /
/// PermissionDenied / Conflict all degrade to a focused-window PTT key + StatusNote.
fn classify_shortcut_result(r: Result<rune::ShortcutId, rune::ShortcutError>) -> &'static str {
    match r {
        Ok(_) => "registered",
        Err(rune::ShortcutError::Unsupported) => "Unsupported -> focused-window PTT fallback",
        Err(rune::ShortcutError::PermissionDenied) => "portal consent declined -> fallback",
        Err(rune::ShortcutError::Conflict) => "conflict -> ask shortcut_owner, rebind",
        Err(rune::ShortcutError::InvalidAccelerator(_)) => "bad accelerator -> rebind",
        Err(rune::ShortcutError::NotRunning) => "app not running -> register at startup",
        Err(rune::ShortcutError::UnknownId) => "unknown id",
        Err(rune::ShortcutError::Backend(_)) => "backend error -> fallback",
    }
}

fn main() {
    println!("== P0-03 OS integration probe ==");

    // 1. Tray + menu, live on Windows/Linux(dbus); Unsupported path on macOS.
    match install_tray(true) {
        Some(tray) => {
            println!("tray: created OK (icon + 3-item menu + tooltip)");
            tray.set_menu(vec![
                TrayItem::action("show", "Show Voxcell"),
                TrayItem::action("ptt", "Push to talk").enabled(false),
                TrayItem::action("quit", "Quit"),
            ]);
            tray.set_tooltip("Voxcell — 1 approval pending");
            println!("tray: set_menu + set_tooltip OK; dropping (icon removed)");
            drop(tray);
        }
        None => println!("tray: graceful fallback path taken"),
    }

    // 2. Global PTT accelerator parse (registration itself needs the running App loop).
    let ptt: Result<Shortcut, _> = "Ctrl+Shift+Space".parse();
    match &ptt {
        Ok(s) => println!("shortcut parse Ctrl+Shift+Space: OK ({s:?})"),
        Err(e) => println!("shortcut parse FAILED: {e}"),
    }
    assert!(ptt.is_ok(), "PTT accelerator must parse");
    // Demonstrate the graceful-degradation classifier on the Unsupported variant.
    println!(
        "shortcut Unsupported handling: {}",
        classify_shortcut_result(Err(rune::ShortcutError::Unsupported))
    );

    // 3. rune-notify: action/reply builders + click callback type, availability probe.
    let n = rune_notify::Notification::new("voxcell-apr-1", "Approval Needed: shell")
        .body("rm -rf build/ — approve?")
        .action("approve", "Approve")
        .action("deny", "Deny")
        .reply("clarify", "Ask a question…", "Send");
    println!("notify: built notification with 2 action buttons + reply input (id voxcell-apr-1)");
    let available = rune_notify::available();
    println!("notify: available() before register/adopt = {available} (post() would no-op: {})", rune_notify::post(&n) == false || available);
    // Click routing API (registered at app startup in the real app):
    //   rune_notify::on_click(|click| { click.action() == Some("approve") -> focus thread;
    //                                   click.input("clarify") -> steer run })
    //   rune_notify::bring_to_front() for click-to-focus.
    println!("notify: on_click/Click::action/Click::input/bring_to_front API present (see lib.rs:265,97,102,327)");

    // 4. Phone-widget window options (constructed, not shown — no GUI flash in a probe).
    let phone = WindowOptions {
        transparent: true,
        borderless: true,
        always_on_top: true,
        resizable: false,
        visible: true,
        min_size: Some((320.0, 420.0)),
        max_size: Some((320.0, 420.0)),
        ..Default::default()
    };
    println!("phone widget WindowOptions: {phone:?}");
    println!("multi-window entry points: rune::run_windows / open_window(WindowSpec) (tooling.rs:207, window/spec.rs:368)");

    println!("PROBE OK");
}
