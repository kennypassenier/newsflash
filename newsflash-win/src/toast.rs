//! The Windows half of `newsflash::run::Desktop`: WinRT toasts under
//! our AppUserModelID. `courier_core::wintoast` decides what a toast
//! looks like; this file only loads the XML and talks to the notifier.
//!
//! Unlike notify-send, `ToastNotifier::Show` never blocks on the user:
//! it returns once Windows has the toast, and clicks arrive later
//! through COM (`activator`). So "shown" (ack) and "answered" (publish)
//! are decoupled here by construction — AR24 without any watcher.

use crate::{AUMID, images, registry};
use courier_core::envelope::Envelope;
use courier_core::hub::HubMessage;
use courier_core::toast::{Language, Lifetimes, PopupDurations};
use courier_core::wintoast::{BuildInput, CriticalScenario, WinToast, build_toast, logo_asset};
use newsflash::config::Config;
use newsflash::logx;
use newsflash::run::{AfterAck, Desktop};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::{DateTime, IReference, PropertyValue, TypedEventHandler};
use windows::UI::Notifications::{
    NotificationData, NotificationSetting, NotificationUpdateResult, ToastDismissalReason,
    ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
    ToastNotifier,
};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::core::{HSTRING, Ref};

/// All tagged toasts share one group, so tags only need to be unique
/// within newsflash.
const GROUP: &str = "newsflash";
/// Toast objects kept alive so their Dismissed/Failed handlers fire.
const KEEP_ALIVE: usize = 128;

pub fn notifier() -> Result<ToastNotifier, String> {
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
        .map_err(|e| format!("cannot create a toast notifier: {}", e.message()))
}

pub struct WinDesktop {
    language: Language,
    critical_scenario: CriticalScenario,
    chime: Option<PathBuf>,
    assets: PathBuf,
    images: PathBuf,
    notifier: Option<ToastNotifier>,
    hold: String,
    live: VecDeque<ToastNotification>,
    lifetimes: Lifetimes,
    popup: PopupDurations,
    link_base_url: Option<String>,
}

impl WinDesktop {
    pub fn new(
        config: &Config,
        critical_scenario: CriticalScenario,
        assets: PathBuf,
        images: PathBuf,
    ) -> Self {
        WinDesktop {
            language: config.language,
            critical_scenario,
            chime: config.sound_file.clone(),
            assets,
            images,
            notifier: None,
            hold: String::new(),
            live: VecDeque::new(),
            lifetimes: config.lifetimes,
            popup: config.popup,
            link_base_url: config.link_base_url.clone(),
        }
    }
}

impl Desktop for WinDesktop {
    /// AR22 on Windows: there is always a notification platform, but
    /// Kenny can switch newsflash (or all notifications) off in
    /// Settings. While that is so, hold — the messages wait at the hub
    /// under the TTL instead of being acked into a toast nobody sees.
    /// Do Not Disturb is NOT a hold: toasts then go quietly to
    /// Notification Center, which is exactly right.
    fn ready(&mut self) -> bool {
        if !registry::is_registered() {
            self.hold = "newsflash is not registered with Windows (run `newsflash install`) — \
                         holding, not consuming"
                .into();
            return false;
        }
        let notifier = match notifier() {
            Ok(n) => n,
            Err(e) => {
                self.hold = format!("{e} — holding, not consuming");
                return false;
            }
        };
        let setting = notifier.Setting();
        let reason = match setting {
            Ok(NotificationSetting::Enabled) | Err(_) => None,
            Ok(NotificationSetting::DisabledForApplication) => Some(
                "notifications for newsflash are turned off (Settings → System → \
                 Notifications → newsflash)",
            ),
            Ok(NotificationSetting::DisabledForUser) => {
                Some("all notifications are turned off for this Windows user")
            }
            Ok(NotificationSetting::DisabledByGroupPolicy) => {
                Some("notifications are disabled by group policy")
            }
            Ok(_) => Some("Windows reports notifications as disabled"),
        };
        if let Some(reason) = reason {
            self.hold = format!("{reason} — holding, not consuming");
            return false;
        }
        self.notifier = Some(notifier);
        true
    }

    fn hold_reason(&self) -> String {
        self.hold.clone()
    }

    fn ready_line(&self) -> &'static str {
        "Windows notifications enabled for newsflash"
    }

    fn show(&mut self, message: &HubMessage, env: &Envelope) -> Result<AfterAck, String> {
        let notifier = self.notifier.clone().ok_or("no toast notifier")?;
        let hero = env.image.as_deref().and_then(|url| {
            images::fetch(url, &self.images, &message.id)
                .map_err(|e| {
                    logx::warn(&format!(
                        "{}: hero image skipped ({e}) — toast shown without it",
                        message.id
                    ))
                })
                .ok()
        });
        let hero = hero.map(|p| p.display().to_string());
        let logo = existing(&self.assets.join(logo_asset(env.priority.as_deref())));
        let built = build_toast(
            env,
            &BuildInput {
                hub_id: &message.id,
                published_at_ms: message.published_at_ms,
                language: self.language,
                critical_scenario: self.critical_scenario,
                logo_uri: logo.as_deref(),
                hero_uri: hero.as_deref(),
                silent: self.chime.is_some(),
                demo: false,
                lifetimes: self.lifetimes,
                popup: self.popup,
                link_base: self.link_base_url.as_deref(),
            },
        );
        if built.dropped_actions > 0 {
            logx::warn(&format!(
                "{}: {} action(s) beyond the Windows limit of 5 dropped (AR27)",
                message.id, built.dropped_actions
            ));
        }
        if built.dropped_inputs > 0 {
            logx::warn(&format!(
                "{}: {} input(s) dropped (over the limit of 5, or a selection without choices)",
                message.id, built.dropped_inputs
            ));
        }
        show_built(&notifier, &built, &message.id, &mut self.live)?;
        let chime = self.chime.clone();
        Ok(Box::new(move || {
            if let Some(chime) = &chime {
                play_chime(chime);
            }
        }))
    }
}

/// Path as a string if the file exists — a missing asset means "no
/// logo", never a failed toast.
pub fn existing(path: &Path) -> Option<String> {
    path.is_file().then(|| path.display().to_string())
}

fn notification_data(values: &[(String, String)]) -> windows::core::Result<NotificationData> {
    let data = NotificationData::new()?;
    let map = data.Values()?;
    for (k, v) in values {
        map.Insert(&HSTRING::from(k), &HSTRING::from(v))?;
    }
    // 0 = always apply (the loop delivers in order; see wintoast feat-win-8).
    data.SetSequenceNumber(0)?;
    Ok(data)
}

/// Shows (or live-updates) one built toast. `label` is the hub id, for
/// the log lines of the dismissal/failure events.
pub fn show_built(
    notifier: &ToastNotifier,
    built: &WinToast,
    label: &str,
    live: &mut VecDeque<ToastNotification>,
) -> Result<(), String> {
    let winerr = |what: &str, e: windows::core::Error| format!("{what}: {}", e.message());

    // feat-win-8: a data-bound toast with the same tag still on screen or in
    // Notification Center is refreshed silently instead of re-popping.
    if let Some(tag) = &built.tag
        && !built.data.is_empty()
    {
        let data = notification_data(&built.data).map_err(|e| winerr("toast data", e))?;
        if let Ok(NotificationUpdateResult::Succeeded) =
            notifier.UpdateWithTagAndGroup(&data, &HSTRING::from(tag), &HSTRING::from(GROUP))
        {
            logx::info(&format!("{label}: updated the live toast {tag:?} in place"));
            return Ok(());
        }
    }

    let doc = XmlDocument::new().map_err(|e| winerr("XmlDocument", e))?;
    doc.LoadXml(&HSTRING::from(&built.xml))
        .map_err(|e| winerr("toast XML rejected", e))?;
    let toast = ToastNotification::CreateToastNotification(&doc)
        .map_err(|e| winerr("creating the toast", e))?;
    if let Some(tag) = &built.tag {
        toast
            .SetTag(&HSTRING::from(tag))
            .and_then(|_| toast.SetGroup(&HSTRING::from(GROUP)))
            .map_err(|e| winerr("setting the tag", e))?;
    }
    if !built.data.is_empty() {
        let data = notification_data(&built.data).map_err(|e| winerr("toast data", e))?;
        toast
            .SetData(&data)
            .map_err(|e| winerr("setting toast data", e))?;
    }
    if let Some(at_ms) = built.expires_at_ms {
        // Never in the past: a toast that already outlived its lifetime
        // while waiting still gets a minute rather than a refused Show.
        let at_ms = at_ms.max(newsflash::run::now_ms() + 60_000);
        let expiry: IReference<DateTime> = PropertyValue::CreateDateTime(unix_ms_to_winrt(at_ms))
            .and_then(|v| windows::core::Interface::cast(&v))
            .map_err(|e| winerr("expiration time", e))?;
        toast
            .SetExpirationTime(&expiry)
            .map_err(|e| winerr("setting the expiration time", e))?;
    }

    let id = label.to_string();
    let _ = toast.Dismissed(&TypedEventHandler::new(
        move |_: Ref<ToastNotification>, args: Ref<ToastDismissedEventArgs>| {
            let what = match args.ok().and_then(|a| a.Reason()) {
                Ok(ToastDismissalReason::UserCanceled) => "dismissed, no action chosen",
                Ok(ToastDismissalReason::TimedOut) => {
                    "popup timed out — the toast stays in Notification Center"
                }
                Ok(ToastDismissalReason::ApplicationHidden) => "replaced or hidden",
                _ => "gone (reason unknown)",
            };
            logx::info(&format!("{id}: {what}"));
            Ok(())
        },
    ));
    let id = label.to_string();
    let _ = toast.Failed(&TypedEventHandler::new(
        move |_: Ref<ToastNotification>, args: Ref<ToastFailedEventArgs>| {
            let code = args
                .ok()
                .and_then(|a| a.ErrorCode())
                .map(|c| format!("{c:?}"))
                .unwrap_or_default();
            logx::warn(&format!("{id}: Windows failed to display the toast {code}"));
            Ok(())
        },
    ));

    notifier
        .Show(&toast)
        .map_err(|e| winerr("showing the toast", e))?;
    live.push_back(toast);
    while live.len() > KEEP_ALIVE {
        live.pop_front();
    }
    Ok(())
}

/// WinRT `DateTime`: 100 ns ticks since 1601-01-01 UTC.
fn unix_ms_to_winrt(ms: u64) -> DateTime {
    const EPOCH_DIFF_MS: i64 = 11_644_473_600_000;
    DateTime {
        UniversalTime: (ms as i64 + EPOCH_DIFF_MS) * 10_000,
    }
}

/// A one-off notice from newsflash itself. Best effort: returns false
/// when it could not be shown (not registered, notifications off).
pub fn show_notice(title: &str, body: &str) -> bool {
    let Ok(n) = notifier() else { return false };
    let built = WinToast {
        xml: courier_core::wintoast::notice_xml(title, body),
        tag: None,
        data: Vec::new(),
        dropped_actions: 0,
        dropped_inputs: 0,
        expires_at_ms: None,
    };
    show_built(&n, &built, "notice", &mut VecDeque::new()).is_ok()
}

/// K6 on Windows: the toast itself is silent when a chime is set, and
/// the WAV plays asynchronously — never a failed message.
fn play_chime(file: &Path) {
    let ok = unsafe {
        PlaySoundW(
            &HSTRING::from(file.as_os_str()),
            None,
            SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
        )
    };
    if !ok.as_bool() {
        logx::warn(&format!(
            "chime {} failed to play — toast was shown anyway",
            file.display()
        ));
    }
}
