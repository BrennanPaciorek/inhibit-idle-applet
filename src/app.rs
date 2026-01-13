// SPDX-License-Identifier: MPL-2.0
use crate::config::Config;
use crate::fl;
use crate::screensaver::ScreenSaverProxy;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::futures::executor::block_on;
use cosmic::iced::{Limits, Subscription, window::Id};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::prelude::*;
use cosmic::widget;
use futures_util::SinkExt;
use tokio::sync::Mutex;
use zbus::{Connection, Result};

// TODO fix the borrow checking me.

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// UI tracker for the inhibit idle toggle
    inhibit_idle: bool,
    /// Stores dbus connection for the duration of the application run
    dbus_connection: Mutex<Option<Connection>>,
    /// Tracks the actual state of whether we're inhibiting idle or not
    inhibit_idle_cookie: Option<u32>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    SubscriptionChannel,
    UpdateConfig(Config),
    SetInhibitIdle(bool),
    ToggleInhibitIdle(bool),
}

async fn create_dbus_connection() -> Result<Connection> {
    Ok(Connection::session().await?)
}

impl AppModel {
    const APP_NAME: &'static str = "com.github.brennanpaciorek.inhibit-idle-applet";

    async fn toggle_idle(&mut self, toggled: bool) -> Result<()> {
        if toggled {
            self.inhibit().await?;
        } else {
            self.uninhibit().await?;
        }
        Ok(())
    }

    async fn inhibit(&mut self) -> Result<()> {
        // Check config for a cookie, uninhibit
        if self.inhibit_idle_cookie.is_some() {
            self.uninhibit().await?;
        }

        let connection_option = self.dbus_connection.lock().await;
        let connection = connection_option
            .as_ref()
            .expect("Expected connection, found no connection");
        let proxy = ScreenSaverProxy::new(&connection).await?;

        proxy
            .inhibit(AppModel::APP_NAME, "User has enabled the idle inhibitor")
            .await?;
        Ok(())
    }

    async fn uninhibit(&mut self) -> Result<()> {
        let connection_option = self.dbus_connection.lock().await;
        let connection = connection_option
            .as_ref()
            .expect("Expected connection, found no connection");
        let proxy = ScreenSaverProxy::new(&connection).await?;

        if let Some(cookie) = self.inhibit_idle_cookie {
            proxy.uninhibit(cookie).await?;
        }

        Ok(())
    }
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = AppModel::APP_NAME;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let connection = Mutex::new(Some(
            block_on(async { create_dbus_connection().await })
                .expect("Failed to establish dbus connection"),
        ));
        // Construct the app model with the runtime's core.
        let app = AppModel {
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
            dbus_connection: connection,
            ..Default::default()
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// The applet's button in the panel will be drawn using the main view method.
    /// This view should emit messages to toggle the applet's popup window, which will
    /// be drawn using the `view_window` method.
    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button("display-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    /// The applet's popup window will be drawn using this view method. If there are
    /// multiple poups, you may match the id parameter to determine which popup to
    /// create a view for.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content_list = widget::list_column()
            .padding(5)
            .spacing(0)
            .add(widget::settings::item(
                fl!("inhibit-idle"),
                widget::toggler(self.inhibit_idle).on_toggle(Message::ToggleInhibitIdle),
            ));

        self.core.applet.popup_container(content_list).into()
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-lived async tasks running in the background which
    /// emit messages to the application through a channel. They may be conditionally
    /// activated by selectively appending to the subscription batch, and will
    /// continue to execute for the duration that they remain in the batch.
    fn subscription(&self) -> Subscription<Self::Message> {
        struct InhibitIdleSubscription;

        Subscription::batch(vec![
            // Create a subscription which emits updates through a channel.
            Subscription::run_with_id(
                std::any::TypeId::of::<InhibitIdleSubscription>(),
                cosmic::iced::stream::channel(4, move |mut channel| async move {
                    _ = channel.send(Message::SubscriptionChannel).await;
                }),
            ),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    // for why in update.errors {
                    //     tracing::error!(?why, "app config error");
                    // }

                    Message::UpdateConfig(update.config)
                }),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::SubscriptionChannel => {
                // For example purposes only.
            }
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::ToggleInhibitIdle(toggled) => {
                // Update the UI value, roll the change back on failure
                self.inhibit_idle = toggled;
                return cosmic::task::future(async move {
                    let message = match self.toggle_idle(toggled).await {
                        // We technically do not need to do anything in this case, but we should
                        // can re-set the value to the right value just in case.
                        Ok(_) => Message::SetInhibitIdle(toggled),
                        Err(e) => {
                            // Get our logging info
                            let action = if toggled { "inhibit" } else { "uninhibit" }.to_string();
                            let description = match e.description() {
                                Some(d) => d,
                                None => "error description unavailable",
                            }
                            .to_string();
                            // Log the error
                            log::error!(
                                "Failed to {} idle due to dbus error: {}",
                                action,
                                description
                            );
                            // Correct the UI-facing value
                            // Concern: if an action fails due to something like the bus session
                            //   disconnecting, this may put the app in a bad state where our
                            //   model of the inhibit idle state does not reflect the state in
                            //   the idle manager.
                            //   The best option may be to log, crash, then send a notification,
                            //   then expect COSMIC to restart.
                            Message::SetInhibitIdle(!toggled)
                        }
                    };
                    return message;
                });
            }
            Message::SetInhibitIdle(idle) => self.inhibit_idle = idle,
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);
                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
