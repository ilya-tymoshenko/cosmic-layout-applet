use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use tracing::{debug, warn};
use zbus::blocking::{Connection, Proxy};

const A11Y_SERVICE: &str = "org.freedesktop.a11y.Manager";
const A11Y_PATH: &str = "/org/freedesktop/a11y/Manager";
const A11Y_IFACE: &str = "org.freedesktop.a11y.KeyboardMonitor";
const A11Y_ALLOWED_NAME_PREFIX: &str = "org.gnome.Orca.KeyboardMonitor";

const XKB_KEY_SHIFT_L: u32 = 0xFFE1;
const XKB_KEY_SHIFT_R: u32 = 0xFFE2;
const XKB_KEY_ALT_L: u32 = 0xFFE9;
const XKB_KEY_ALT_R: u32 = 0xFFEA;
const XKB_KEY_ISO_NEXT_GROUP: u32 = 0xFE08;
const XKB_KEY_ISO_PREV_GROUP: u32 = 0xFE0A;

#[derive(Debug, Clone, Copy)]
enum LayoutEvent {
    ToggleNext,
    TogglePrev,
    ObservedChar(u32),
}

#[derive(Default)]
struct AltShiftTracker {
    alt_down: bool,
    shift_down: bool,
    combo_armed: bool,
    blocked_by_other_key: bool,
}

impl AltShiftTracker {
    fn on_key_event(&mut self, released: bool, keysym: u32) -> Option<LayoutEvent> {
        if !released {
            if keysym == XKB_KEY_ISO_NEXT_GROUP {
                return Some(LayoutEvent::ToggleNext);
            }
            if keysym == XKB_KEY_ISO_PREV_GROUP {
                return Some(LayoutEvent::TogglePrev);
            }
        }

        let is_alt = matches!(keysym, XKB_KEY_ALT_L | XKB_KEY_ALT_R);
        let is_shift = matches!(keysym, XKB_KEY_SHIFT_L | XKB_KEY_SHIFT_R);

        if is_alt || is_shift {
            if !released {
                if is_alt {
                    self.alt_down = true;
                }
                if is_shift {
                    self.shift_down = true;
                }

                if self.alt_down && self.shift_down && !self.blocked_by_other_key {
                    self.combo_armed = true;
                }
                return None;
            }

            let should_toggle = self.combo_armed && self.alt_down && self.shift_down;

            if is_alt {
                self.alt_down = false;
            }
            if is_shift {
                self.shift_down = false;
            }

            if !self.alt_down && !self.shift_down {
                self.combo_armed = false;
                self.blocked_by_other_key = false;
            } else if should_toggle {
                self.combo_armed = false;
            }

            return if should_toggle {
                Some(LayoutEvent::ToggleNext)
            } else {
                None
            };
        }

        if !released && (self.alt_down || self.shift_down) {
            self.blocked_by_other_key = true;
            self.combo_armed = false;
        }

        None
    }
}

pub struct X11XkbSource {
    events_rx: Receiver<LayoutEvent>,
    config_path: PathBuf,
    config_mtime: Option<SystemTime>,
    layouts: Vec<String>,
    active_idx: usize,
}

impl X11XkbSource {
    pub fn connect() -> Result<Self, String> {
        let config_path = default_xkb_config_path()?;
        let layouts = read_layouts_from_config(&config_path).unwrap_or_else(|err| {
            warn!("failed to read xkb_config, using fallback list: {err}");
            vec!["us".to_string(), "ru".to_string()]
        });

        let config_mtime = file_mtime(&config_path);
        let (events_tx, events_rx) = mpsc::channel();
        spawn_a11y_keyboard_monitor(events_tx);

        Ok(Self {
            events_rx,
            config_path,
            config_mtime,
            layouts,
            active_idx: 0,
        })
    }

    pub fn current_layout_label(&mut self) -> Result<String, String> {
        self.reload_layouts_if_changed();
        self.consume_events();

        let current = self
            .layouts
            .get(self.active_idx)
            .or_else(|| self.layouts.first())
            .cloned()
            .unwrap_or_else(|| "--".to_string());

        Ok(layout_to_label(&current))
    }

    fn consume_events(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                LayoutEvent::ToggleNext => self.toggle_next(),
                LayoutEvent::TogglePrev => self.toggle_prev(),
                LayoutEvent::ObservedChar(ch) => self.sync_from_character(ch),
            }
        }
    }

    fn toggle_next(&mut self) {
        if self.layouts.is_empty() {
            self.active_idx = 0;
            return;
        }
        self.active_idx = (self.active_idx + 1) % self.layouts.len();
    }

    fn toggle_prev(&mut self) {
        if self.layouts.is_empty() {
            self.active_idx = 0;
            return;
        }
        self.active_idx = if self.active_idx == 0 {
            self.layouts.len() - 1
        } else {
            self.active_idx - 1
        };
    }

    fn sync_from_character(&mut self, ch: u32) {
        if self.layouts.is_empty() {
            return;
        }

        let Some(target_code) = infer_layout_code_from_unicode(ch) else {
            return;
        };

        if let Some(idx) = find_layout_index(&self.layouts, target_code) {
            self.active_idx = idx;
        }
    }

    fn reload_layouts_if_changed(&mut self) {
        let current_mtime = file_mtime(&self.config_path);
        if current_mtime.is_none() || current_mtime == self.config_mtime {
            return;
        }

        let previous_layout = self.layouts.get(self.active_idx).cloned();
        match read_layouts_from_config(&self.config_path) {
            Ok(new_layouts) => {
                if !new_layouts.is_empty() {
                    self.layouts = new_layouts;
                    self.active_idx = previous_layout
                        .as_deref()
                        .and_then(|name| {
                            let normalized = normalized_layout_code(name);
                            find_layout_index(&self.layouts, normalized.as_str())
                        })
                        .unwrap_or(0);
                }
            }
            Err(err) => debug!("failed to reload xkb_config: {err}"),
        }

        self.config_mtime = current_mtime;
    }
}

fn spawn_a11y_keyboard_monitor(tx: Sender<LayoutEvent>) {
    thread::spawn(move || {
        loop {
            if let Err(err) = run_a11y_keyboard_monitor(&tx) {
                debug!("a11y keyboard monitor disconnected: {err}");
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn run_a11y_keyboard_monitor(tx: &Sender<LayoutEvent>) -> Result<(), String> {
    let conn =
        Connection::session().map_err(|err| format!("dbus session connect failed: {err}"))?;

    request_keyboard_monitor_name(&conn)?;

    let proxy = Proxy::new(&conn, A11Y_SERVICE, A11Y_PATH, A11Y_IFACE)
        .map_err(|err| format!("failed to build a11y proxy: {err}"))?;

    proxy
        .call::<_, _, ()>("WatchKeyboard", &())
        .map_err(|err| format!("WatchKeyboard call failed: {err}"))?;

    let mut tracker = AltShiftTracker::default();
    let mut stream = proxy
        .receive_signal("KeyEvent")
        .map_err(|err| format!("failed to subscribe KeyEvent: {err}"))?;

    while let Some(msg) = stream.next() {
        let (released, _state, keysym, unichar, _keycode): (bool, u32, u32, u32, u16) = msg
            .body()
            .deserialize()
            .map_err(|err| format!("signal decode failed: {err}"))?;

        if let Some(evt) = tracker.on_key_event(released, keysym) {
            let _ = tx.send(evt);
        }

        if !released && unichar != 0 {
            let _ = tx.send(LayoutEvent::ObservedChar(unichar));
        }
    }

    Err("key_event stream closed".to_string())
}

fn request_keyboard_monitor_name(conn: &Connection) -> Result<(), String> {
    if conn.request_name(A11Y_ALLOWED_NAME_PREFIX).is_ok() {
        return Ok(());
    }

    let fallback_name = format!(
        "{A11Y_ALLOWED_NAME_PREFIX}.CosmicLayoutApplet{}",
        std::process::id()
    );
    conn.request_name(fallback_name.as_str())
        .map_err(|err| format!("failed to own keyboard monitor name: {err}"))?;
    Ok(())
}

fn default_xkb_config_path() -> Result<PathBuf, String> {
    let base = if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = env::var("HOME").map_err(|err| format!("HOME is not set: {err}"))?;
        PathBuf::from(home).join(".config")
    };

    Ok(base
        .join("cosmic")
        .join("com.system76.CosmicComp")
        .join("v1")
        .join("xkb_config"))
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn read_layouts_from_config(path: &PathBuf) -> Result<Vec<String>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;

    let Some(layout_field) = extract_quoted_field(&contents, "layout") else {
        return Err(format!("layout field not found in {}", path.display()));
    };

    let layouts = layout_field
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    if layouts.is_empty() {
        return Err(format!("layout list is empty in {}", path.display()));
    }

    Ok(layouts)
}

fn extract_quoted_field(contents: &str, key: &str) -> Option<String> {
    for line in contents.lines() {
        let line = line.trim();
        if !line.starts_with(key) {
            continue;
        }

        let start = line.find('"')?;
        let tail = &line[start + 1..];
        let end = tail.find('"')?;
        return Some(tail[..end].to_string());
    }

    None
}

fn infer_layout_code_from_unicode(ch: u32) -> Option<&'static str> {
    if is_cyrillic(ch) {
        return Some("ru");
    }

    if is_latin_letter(ch) {
        return Some("us");
    }

    None
}

fn is_cyrillic(ch: u32) -> bool {
    (0x0400..=0x052F).contains(&ch)
        || (0x2DE0..=0x2DFF).contains(&ch)
        || (0xA640..=0xA69F).contains(&ch)
}

fn is_latin_letter(ch: u32) -> bool {
    (0x0041..=0x005A).contains(&ch) || (0x0061..=0x007A).contains(&ch)
}

fn find_layout_index(layouts: &[String], code: &str) -> Option<usize> {
    layouts
        .iter()
        .position(|layout| normalized_layout_code(layout) == code)
}

fn normalized_layout_code(layout: &str) -> String {
    layout
        .split(['(', ':'])
        .next()
        .unwrap_or(layout)
        .trim()
        .to_lowercase()
}

fn layout_to_label(layout: &str) -> String {
    match normalized_layout_code(layout).as_str() {
        "us" => "US".to_string(),
        "ru" => "RU".to_string(),
        other if other.is_empty() => "--".to_string(),
        other => other.chars().take(3).collect::<String>().to_uppercase(),
    }
}
