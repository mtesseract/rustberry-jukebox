use std::thread;

use crossbeam_channel::{self, Receiver, Select, Sender};
use failure::Fallible;
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;

use rustberry::config::Config;
use rustberry::effects::Effects;
use rustberry::effects::ProdInterpreter;
use rustberry::input_controller::{
    button,
    playback::{self},
    Input,
};
use rustberry::player::{self, Player};

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || main_with_log())
}

fn main_with_log() -> Fallible<()> {
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    // Create Effects Channel and Interpreter.
    let (tx, rx): (Sender<Effects>, Receiver<Effects>) = crossbeam_channel::bounded(2);
    let mut interpreter = ProdInterpreter::new(&config).unwrap();

    interpreter.wait_until_ready().map_err(|err| {
        error!("Failed to wait for interpreter readiness: {}", err);
        err
    })?;

    // Run Interpreter.
    thread::Builder::new()
        .name("interpreter".to_string())
        .spawn(move || interpreter.run(rx))
        .unwrap();

    // Create Player component.
    let player_handle = Player::new(tx.clone());

    // Prepare individual input channels.
    let button_controller_handle =
        button::cdev_gpio::CdevGpio::new_from_env(|cmd| Some(Input::Button(cmd)))?;
    let playback_controller_handle =
        playback::rfid::PlaybackRequestTransmitterRfid::new(|req| Some(Input::Playback(req)))?;

    // Execute Application Logic, producing Effects.
    let application = App::new(
        config,
        &vec![
            button_controller_handle.channel(),
            playback_controller_handle.channel(),
        ],
        tx,
    );
    application.run().map_err(|err| {
        warn!("Jukebox loop terminated, terminating application: {}", err);
        err
    })?;
    unreachable!();
}

struct App {
    config: Config,
    player_handle: player::Handle,
    inputs: Vec<Receiver<Input>>,
    effects_tx: Sender<Effects>,
}

impl App {
    pub fn new(config: Config, inputs: &[Receiver<Input>], effects_tx: Sender<Effects>) -> Self {
        let player_handle = Player::new(effects_tx.clone());
        Self {
            config,
            inputs: inputs.to_vec(),
            effects_tx,
            player_handle,
        }
    }

    pub fn run(self) -> Fallible<()> {
        let mut sel = Select::new();
        for r in &self.inputs {
            sel.recv(r);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            let res = self.inputs[index].try_recv();

            let effects: Vec<Effects> = match res {
                Err(err) => {
                    if err.is_empty() {
                        // If the operation turns out not to be ready, retry.
                        vec![]
                    } else {
                        // FIXME
                        error!("Failed to receive input event: {}", err);
                        vec![]
                    }
                }
                Ok(input) => match input {
                    Input::Button(cmd) => match cmd {
                        button::Command::Shutdown => vec![Effects::GenericCommand(
                            self.config
                                .shutdown_command
                                .clone()
                                .unwrap_or("sudo shutdown -h now".to_string()),
                        )],
                        button::Command::VolumeUp => vec![Effects::GenericCommand(
                            self.config
                                .volume_up_command
                                .clone()
                                .unwrap_or("amixer -q -M set PCM 10%+".to_string()),
                        )],
                        button::Command::VolumeDown => vec![Effects::GenericCommand(
                            self.config
                                .volume_up_command
                                .clone()
                                .unwrap_or("amixer -q -M set PCM 10%-".to_string()),
                        )],
                    },
                    Input::Playback(request) => {
                        if let Err(err) = self.player_handle.playback(request.clone()) {
                            error!(
                                "Failed to send playback request {:?} to Player: {}",
                                request, err
                            );
                        }
                        vec![]
                    }
                },
            };
            for effect in effects {
                self.effects_tx.send(effect)?;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use rustberry::config::Config;
    use rustberry::effects::Effects;
    use rustberry::input_controller::{button, Input};

    use super::*;

    #[test]
    fn jukebox_can_be_shut_down() {
        let (effects_tx, effects_rx) = crossbeam_channel::bounded(10);
        let config: Config = Config {
            refresh_token: "token".to_string(),
            client_id: "client".to_string(),
            client_secret: "secret".to_string(),
            device_name: "device".to_string(),
            post_init_command: None,
            shutdown_command: None,
            volume_up_command: None,
            volume_down_command: None,
        };
        let inputs = vec![Input::Button(button::Command::Shutdown)];
        let effects_expected = vec![Effects::GenericCommand("sudo shutdown -h now".to_string())];
        let (input_tx, input_rx) = crossbeam_channel::bounded(10);
        let app = App::new(config, &vec![input_rx], effects_tx);
        for input in inputs {
            input_tx.send(input).unwrap();
        }
        drop(input_tx);
        app.run();
        let produced_effects: Vec<_> = effects_rx.iter().collect();

        assert_eq!(produced_effects, effects_expected);
    }
}
