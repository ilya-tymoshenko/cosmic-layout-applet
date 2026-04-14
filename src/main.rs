mod x11_xkb;

use std::time::Duration;

use cosmic::app::Core;
use cosmic::iced::Subscription;
use cosmic::{app, applet, iced, prelude::*};
use tracing::{debug, warn};
use x11_xkb::X11XkbSource;

const APP_ID: &str = "com.ilya.CosmicAppletRealKeyboardLayout";

fn main() -> iced::Result {
    init_tracing();
    applet::run::<LayoutApplet>(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cosmic_layout_applet=debug".into()),
        )
        .try_init();
}

struct LayoutApplet {
    core: Core,
    source: Option<X11XkbSource>,
    label: String,
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Noop,
}

impl cosmic::Application for LayoutApplet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        let source = match X11XkbSource::connect() {
            Ok(source) => Some(source),
            Err(err) => {
                warn!("xkb source unavailable: {err}");
                None
            }
        };

        let mut app = Self {
            core,
            source,
            label: "--".to_string(),
        };
        app.refresh_label();

        (app, iced::Task::none())
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::Tick => self.refresh_label(),
            Message::Noop => {}
        }

        iced::Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let button = self
            .core
            .applet
            .text_button(self.core.applet.text(self.label.as_str()), Message::Noop);
        self.core.applet.autosize_window(button).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl LayoutApplet {
    fn refresh_label(&mut self) {
        let Some(source) = self.source.as_mut() else {
            self.label = "--".to_string();
            return;
        };

        match source.current_layout_label() {
            Ok(label) => {
                self.label = label;
            }
            Err(err) => {
                debug!("failed to read xkb state: {err}");
            }
        }
    }
}
