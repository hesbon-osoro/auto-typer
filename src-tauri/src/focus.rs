//! Which window has keyboard focus, and bringing one back to the front.
//! Windows: HWND. Linux: X11 window id (Wayland isn't supported yet). macOS: the frontmost
//! app's pid, since apps (not windows) take focus there.

use serde::{Deserialize, Serialize};

/// A window keys can be typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub id: u64,
    pub pid: u32,
}

impl Target {
    /// One of Auto Typer's own windows.
    pub fn is_own(&self) -> bool {
        self.pid == std::process::id()
    }
}

pub use imp::{activate, foreground};

#[cfg(windows)]
mod imp {
    use super::Target;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SW_RESTORE,
        SetForegroundWindow, ShowWindow,
    };

    pub fn foreground() -> Option<Target> {
        // SAFETY: plain Win32 calls with no pointers kept beyond the call.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            Some(Target { id: hwnd as u64, pid })
        }
    }

    pub fn activate(target: Target) -> bool {
        // SAFETY: IsWindow guards against a stale handle.
        unsafe {
            let hwnd = target.id as _;
            if IsWindow(hwnd) == 0 {
                return false;
            }
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd) != 0
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Target;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, Window,
    };
    use x11rb::rust_connection::RustConnection;

    fn connect() -> Option<(RustConnection, Window)> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;
        Some((conn, root))
    }

    fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
        Some(conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok()?.atom)
    }

    fn cardinal(conn: &RustConnection, window: Window, prop: u32, ty: AtomEnum) -> Option<u32> {
        let reply = conn.get_property(false, window, prop, ty, 0, 1).ok()?.reply().ok()?;
        reply.value32()?.next()
    }

    pub fn foreground() -> Option<Target> {
        let (conn, root) = connect()?;
        let window = cardinal(&conn, root, atom(&conn, "_NET_ACTIVE_WINDOW")?, AtomEnum::WINDOW)?;
        if window == 0 {
            return None;
        }
        let pid = cardinal(&conn, window, atom(&conn, "_NET_WM_PID")?, AtomEnum::CARDINAL).unwrap_or(0);
        Some(Target { id: window as u64, pid })
    }

    pub fn activate(target: Target) -> bool {
        let Some((conn, root)) = connect() else { return false };
        let Some(active) = atom(&conn, "_NET_ACTIVE_WINDOW") else { return false };
        // Source 2 = pager: window managers honour it without focus-stealing prevention.
        let event = ClientMessageEvent::new(32, target.id as Window, active, [2, 0, 0, 0, 0]);
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        conn.send_event(false, root, mask, event).is_ok() && conn.flush().is_ok()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Target;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub fn foreground() -> Option<Target> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = app.processIdentifier();
        (pid > 0).then_some(Target { id: pid as u64, pid: pid as u32 })
    }

    pub fn activate(target: Target) -> bool {
        NSRunningApplication::runningApplicationWithProcessIdentifier(target.pid as i32)
            .is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::empty()))
    }
}
