//! Glues the scanner, device tracker, pairing lookup, ear detection, media
//! control and notifications together.

use std::time::{Duration, Instant};

use kord_core::DeviceStatus;
use kord_core::battery::LowBatteryMonitor;
use kord_core::ear::{EarAction, EarDetector};
use kord_core::tracker::{Observation, Tracker};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::config::Config;
use crate::link::{self, Link};
use crate::scanner::Sighting;
use crate::{audio, media, notify, scanner};

/// Everything the UI shows about the tracked headphones.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub device: DeviceStatus,
    pub link: Link,
}

impl Snapshot {
    /// The name the user gave the headphones, or the model name.
    pub fn display_name(&self) -> &str {
        match &self.link {
            Link::Paired(paired) if !paired.name.is_empty() => &paired.name,
            _ => &self.device.model_name,
        }
    }
}

/// Requests from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Connect,
    Disconnect,
    /// Disconnect and connect again, e.g. after changing the audio codec.
    Reconnect,
}

/// How long both earbuds must stay out before sound moves to the speakers.
const OUTPUT_SWITCH_DELAY: Duration = Duration::from_millis(1500);

/// How often the pairing's connection state is re-read.
const LINK_POLL: Duration = Duration::from_secs(2);
/// Faster polling right after a connect/disconnect request, so the UI follows quickly.
const LINK_POLL_FAST: Duration = Duration::from_millis(700);
const FAST_POLL_FOR: Duration = Duration::from_secs(12);
/// How often the list of paired models is refreshed.
const PAIRED_POLL: Duration = Duration::from_secs(30);
/// Don't auto-connect again within this long.
const AUTO_CONNECT_COOLDOWN: Duration = Duration::from_secs(30);

/// Runs until the process exits, calling `on_update` whenever what the UI
/// shows changes (`None` = no headphones around).
pub async fn run(
    mut config: watch::Receiver<Config>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    on_update: impl Fn(Option<Snapshot>) + Send + 'static,
) {
    let (tx, mut sightings) = mpsc::channel(64);
    tokio::spawn(scanner::run(tx));

    let settings = config.borrow_and_update().clone();
    let mut engine = Engine::new(settings, on_update);
    engine.refresh_paired_models().await;
    let mut tick = tokio::time::interval(Duration::from_millis(500));

    loop {
        let ear_deadline = engine.ear.deadline();
        tokio::select! {
            Some(sighting) = sightings.recv() => engine.on_sighting(sighting).await,
            () = sleep_until(ear_deadline) => engine.on_ear_deadline().await,
            _ = tick.tick() => engine.on_tick().await,
            Some(command) = commands.recv() => engine.on_command(command).await,
            Ok(()) = config.changed() => {
                let settings = config.borrow_and_update().clone();
                engine.on_config(settings);
            }
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

struct Engine<F> {
    settings: Config,
    tracker: Tracker,
    ear: EarDetector,
    battery: LowBatteryMonitor,
    /// Players we paused, to resume exactly those and nothing else.
    paused_players: Vec<String>,
    link: Link,
    next_link_check: Instant,
    fast_poll_until: Instant,
    /// Model ids of the headphones paired with this computer.
    paired_models: Vec<u16>,
    next_paired_check: Instant,
    /// Buds in an ear at the last advertisement, for auto-connect.
    previous_worn: Option<u8>,
    last_auto_connect: Option<Instant>,
    /// Since when both earbuds have been out, for moving sound to the speakers.
    both_out_since: Option<Instant>,
    /// Sound was moved to the speakers because both earbuds came out.
    output_moved: bool,
    /// The output that was the default before the headphones took over.
    previous_output: Option<String>,
    keep_alive: Option<audio::KeepAlive>,
    last_reported: Option<Snapshot>,
    on_update: F,
}

impl<F: Fn(Option<Snapshot>)> Engine<F> {
    fn new(settings: Config, on_update: F) -> Self {
        let now = Instant::now();
        Self {
            tracker: Tracker::new(settings.tracker_config()),
            ear: EarDetector::new(settings.ear_config()),
            battery: LowBatteryMonitor::new(settings.notifications.low_battery),
            settings,
            paused_players: Vec::new(),
            link: Link::Unknown,
            next_link_check: now,
            fast_poll_until: now,
            paired_models: Vec::new(),
            next_paired_check: now + PAIRED_POLL,
            previous_worn: None,
            last_auto_connect: None,
            both_out_since: None,
            output_moved: false,
            previous_output: None,
            keep_alive: None,
            last_reported: None,
            on_update,
        }
    }

    async fn on_sighting(&mut self, sighting: Sighting) {
        let now = Instant::now();
        match self.tracker.observe(sighting.status, sighting.rssi, now) {
            Observation::Ignored => return,
            Observation::Updated => {}
            Observation::Switched => {
                self.forget_session();
                if let Some(device) = self.tracker.current().cloned() {
                    info!("tracking {} (rssi {:?})", device.model_name, sighting.rssi);
                    let fresh = lookup(&device).await;
                    // A fresh lock is not a connection change: don't run connect actions.
                    self.link = fresh;
                    self.next_link_check = now + LINK_POLL;
                    self.sync_keep_alive();
                }
            }
        }
        let Some(device) = self.tracker.current().cloned() else { return };

        if self.ear_detection_applies(&device)
            && let Some(worn) = device.ears.worn_count()
            && let Some(action) = self.ear.update(worn, now)
        {
            self.apply_ear_action(action).await;
        }
        self.maybe_auto_connect(&device, now).await;
        self.track_both_out(&device, now);
        self.maybe_move_output(now).await;

        // Warn only while they're in use with this computer, not when they
        // just happen to be nearby connected to a phone.
        if self.link.may_play_here() {
            for (component, level) in self.battery.check(&device) {
                let name = Snapshot { device: device.clone(), link: self.link.clone() }.display_name().to_owned();
                notify::low_battery(&name, component, level);
            }
        }
        self.report();
    }

    async fn on_ear_deadline(&mut self) {
        let applies = self.tracker.current().is_some_and(|device| self.ear_detection_applies(device));
        if let Some(action) = self.ear.poll(Instant::now())
            && applies
        {
            self.apply_ear_action(action).await;
        }
    }

    async fn on_tick(&mut self) {
        let now = Instant::now();
        if now >= self.next_paired_check {
            self.refresh_paired_models().await;
        }
        if self.tracker.expire(now) {
            info!("headphones gone");
            self.forget_session();
            self.link = Link::Unknown;
            self.sync_keep_alive();
            self.report();
            return;
        }
        let Some(device) = self.tracker.current().cloned() else { return };
        self.maybe_move_output(now).await;
        if now < self.next_link_check {
            return;
        }
        let fresh = lookup(&device).await;
        self.set_link(fresh).await;
        let interval = if now < self.fast_poll_until { LINK_POLL_FAST } else { LINK_POLL };
        self.next_link_check = Instant::now() + interval;
        self.report();
    }

    async fn on_command(&mut self, command: Command) {
        let Link::Paired(paired) = self.link.clone() else {
            warn!("{command:?} ignored: headphones are not paired with this computer");
            return;
        };
        info!("{command:?} {}", paired.name);
        if command == Command::Reconnect {
            // The keep-alive stream would hold the old connection open.
            self.keep_alive = None;
        }
        run_blocking(move || match command {
            Command::Connect => link::connect(&paired),
            Command::Disconnect => link::disconnect(&paired),
            Command::Reconnect => {
                link::disconnect(&paired)?;
                std::thread::sleep(Duration::from_secs(2));
                link::connect(&paired)
            }
        })
        .await;
        self.poll_link_fast();
    }

    fn on_config(&mut self, settings: Config) {
        self.ear.set_config(settings.ear_config());
        self.battery.set_threshold(settings.notifications.low_battery);
        if !settings.ear_detection.enabled {
            self.ear.reset();
            self.paused_players.clear();
        }
        let refilter = settings.tracking != self.settings.tracking;
        self.settings = settings;
        if refilter {
            self.apply_tracker_config();
        }
        self.sync_keep_alive();
    }

    /// Runs the keep-alive stream exactly while it's enabled and the headphones
    /// are connected here.
    fn sync_keep_alive(&mut self) {
        let output = match &self.link {
            Link::Paired(paired) if paired.connected && self.settings.audio.keep_alive => paired.output_endpoint(),
            _ => None,
        };
        match output {
            Some(output) if self.keep_alive.is_none() => {
                info!("keeping the audio link awake");
                self.keep_alive = Some(audio::KeepAlive::start(output));
            }
            None if self.keep_alive.is_some() => self.keep_alive = None,
            _ => {}
        }
    }

    fn track_both_out(&mut self, device: &DeviceStatus, now: Instant) {
        match device.ears.worn_count() {
            Some(0) => {
                self.both_out_since.get_or_insert(now);
            }
            Some(_) => self.both_out_since = None,
            None => {}
        }
    }

    /// Sends sound to the speakers while both earbuds are out, and back to the
    /// headphones when one goes in again, if enabled.
    async fn maybe_move_output(&mut self, now: Instant) {
        let Link::Paired(paired) = &self.link else { return };
        if !self.settings.ear_detection.switch_output_when_removed || !paired.connected {
            return;
        }
        let Some(headphones) = paired.output_endpoint() else { return };
        let both_out = self.both_out_since.is_some_and(|since| now.duration_since(since) >= OUTPUT_SWITCH_DELAY);
        if both_out && !self.output_moved {
            let previous = self.previous_output.clone();
            let speakers = run_blocking(move || match previous {
                Some(previous) => Ok(Some(previous)),
                None => audio::other_output(std::slice::from_ref(&headphones)),
            })
            .await
            .flatten();
            if let Some(speakers) = speakers {
                info!("both earbuds out, moving sound to the speakers");
                run_blocking(move || audio::set_default(&speakers)).await;
                self.output_moved = true;
            }
        } else if self.output_moved && self.both_out_since.is_none() {
            info!("earbud back in, moving sound to the headphones");
            run_blocking(move || audio::set_default(&headphones)).await;
            self.output_moved = false;
        }
    }

    /// Only touch playback when these headphones can actually be playing this
    /// computer's audio (not just sitting next to it, connected to a phone).
    fn ear_detection_applies(&self, device: &DeviceStatus) -> bool {
        self.settings.ear_detection.enabled && device.connected != Some(false) && self.link.may_play_here()
    }

    async fn apply_ear_action(&mut self, action: EarAction) {
        match action {
            EarAction::Pause => {
                self.paused_players = run_blocking(media::pause_playing).await.unwrap_or_default();
                info!("earbud removed, paused {:?}", self.paused_players);
            }
            EarAction::Resume => {
                let players = std::mem::take(&mut self.paused_players);
                if self.settings.ear_detection.resume {
                    info!("earbuds back in, resuming {players:?}");
                    run_blocking(move || media::resume(&players)).await;
                }
            }
        }
    }

    /// Connects the headphones here when you put one in your ear, if enabled.
    async fn maybe_auto_connect(&mut self, device: &DeviceStatus, now: Instant) {
        let worn = device.ears.worn_count();
        let put_on = self.previous_worn == Some(0) && worn.is_some_and(|w| w > 0);
        self.previous_worn = worn;
        let Link::Paired(paired) = &self.link else { return };
        let cooled_down = self.last_auto_connect.is_none_or(|at| now.duration_since(at) > AUTO_CONNECT_COOLDOWN);
        if put_on && !paired.connected && self.settings.connection.auto_connect && cooled_down {
            info!("earbud put on, connecting {}", paired.name);
            self.last_auto_connect = Some(now);
            let paired = paired.clone();
            run_blocking(move || link::connect(&paired)).await;
            self.poll_link_fast();
        }
    }

    /// Updates the connection state, running the connect/disconnect actions on changes.
    async fn set_link(&mut self, fresh: Link) {
        if fresh == self.link {
            return;
        }
        let known_before = matches!(self.link, Link::Paired(_));
        let (was, is) = (self.link.is_connected(), fresh.is_connected());
        self.link = fresh;
        self.sync_keep_alive();
        if !known_before || was == is {
            return;
        }
        let connection = self.settings.connection.clone();
        let Link::Paired(paired) = self.link.clone() else { return };
        if connection.notify_connection {
            notify::connection(&paired.name, is);
        }
        if is {
            info!("connected to this computer");
            if connection.switch_output_on_connect
                && let Some(headphones) = paired.output_endpoint()
            {
                let previous = run_blocking(audio::default_output).await.flatten();
                if previous.as_ref() != Some(&headphones) {
                    self.previous_output = previous;
                }
                run_blocking(move || audio::set_default(&headphones)).await;
            }
            if connection.default_volume > 0 {
                let paired = paired.clone();
                run_blocking(move || link::set_volume(&paired, connection.default_volume)).await;
            }
            if connection.auto_play {
                run_blocking(media::play_current).await;
            }
        } else {
            info!("disconnected from this computer");
            // Nothing of ours to resume any more.
            self.ear.reset();
            self.paused_players.clear();
            self.output_moved = false;
            if connection.pause_on_disconnect {
                run_blocking(media::pause_playing).await;
            }
        }
    }

    async fn refresh_paired_models(&mut self) {
        self.next_paired_check = Instant::now() + PAIRED_POLL;
        let ids = run_blocking(link::paired_ids).await.unwrap_or_default();
        let models: Vec<u16> = ids
            .into_iter()
            .filter(|(vendor, _)| *vendor == kord_core::apple::COMPANY_ID)
            .map(|(_, product)| product.swap_bytes())
            .collect();
        if models != self.paired_models {
            self.paired_models = models;
            self.apply_tracker_config();
        }
    }

    fn apply_tracker_config(&mut self) {
        let mut config = self.settings.tracker_config();
        if self.settings.tracking.only_paired {
            config.allowed_models = self.paired_models.clone();
        }
        self.tracker.set_config(config);
    }

    fn poll_link_fast(&mut self) {
        let now = Instant::now();
        self.fast_poll_until = now + FAST_POLL_FOR;
        self.next_link_check = now + LINK_POLL_FAST;
    }

    /// Forgets per-device state when the tracked device changes or leaves.
    fn forget_session(&mut self) {
        self.ear.reset();
        self.paused_players.clear();
        self.previous_worn = None;
        self.both_out_since = None;
        self.output_moved = false;
        self.battery = LowBatteryMonitor::new(self.settings.notifications.low_battery);
    }

    fn report(&mut self) {
        let snapshot =
            self.tracker.current().map(|device| Snapshot { device: device.clone(), link: self.link.clone() });
        if self.last_reported != snapshot {
            self.last_reported = snapshot.clone();
            (self.on_update)(snapshot);
        }
    }
}

/// Finds the pairing for the tracked headphones on this computer.
async fn lookup(device: &DeviceStatus) -> Link {
    let (vendor_id, product_id) = device.pnp_id();
    run_blocking(move || link::find(vendor_id, product_id)).await.unwrap_or(Link::Unknown)
}

/// Runs a blocking OS call on the blocking pool, logging failures.
async fn run_blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Option<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(err)) => {
            warn!("{err:#}");
            None
        }
        Err(err) => {
            warn!("background task failed: {err}");
            None
        }
    }
}
