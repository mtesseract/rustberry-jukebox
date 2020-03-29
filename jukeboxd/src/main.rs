use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{self, Receiver, Select};
use failure::Fallible;
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;

use rustberry::config::Config;
use rustberry::effects::{Interpreter, ProdInterpreter};
use rustberry::input_controller::{button, playback, Input};
use rustberry::player::{self, PlaybackRequest, Player};

use led::Blinker;

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
    let interpreter = ProdInterpreter::new(&config).unwrap();

    interpreter.wait_until_ready().map_err(|err| {
        error!("Failed to wait for interpreter readiness: {}", err);
        err
    })?;

    let interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>> =
        Arc::new(Box::new(interpreter));

    let blinker = Blinker::new(interpreter.clone()).unwrap();

    // Prepare individual input channels.
    let button_controller_handle =
        button::cdev_gpio::CdevGpio::new_from_env(|cmd| Some(Input::Button(cmd)))?;
    let playback_controller_handle =
        playback::rfid::PlaybackRequestTransmitterRfid::new(|req| Some(Input::Playback(req)))?;

    // Execute Application Logic, producing Effects.
    let application = App::new(
        config,
        interpreter.clone(),
        blinker,
        &vec![
            button_controller_handle.channel(),
            playback_controller_handle.channel(),
        ],
    );
    application.run().map_err(|err| {
        warn!("Jukebox loop terminated, terminating application: {}", err);
        err
    })?;
    unreachable!();
}

struct App {
    config: Config,
    player: player::Player,
    interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
    inputs: Vec<Receiver<Input>>,
    blinker: Blinker,
}

impl App {
    pub fn new(
        config: Config,
        interpreter: Arc<Box<dyn Interpreter + Sync + Send + 'static>>,
        blinker: Blinker,
        inputs: &[Receiver<Input>],
    ) -> Self {
        let player = Player::new(interpreter.clone());
        Self {
            config,
            inputs: inputs.to_vec(),
            player,
            interpreter,
            blinker,
        }
    }

    pub fn run(self) -> Fallible<()> {
        self.blinker.run_async(&led::XXX::Repeat(
            20,
            Box::new(led::XXX::Many(vec![
                led::XXX::On(Duration::from_millis(5)),
                led::XXX::Off(Duration::from_millis(5)),
            ])),
        ));

        let mut sel = Select::new();
        for r in &self.inputs {
            sel.recv(r);
        }

        loop {
            // Wait until a receive operation becomes ready and try executing it.
            let index = sel.ready();
            let res = self.inputs[index].try_recv();

            match res {
                Err(err) => {
                    if err.is_empty() {
                        // If the operation turns out not to be ready, retry.
                        continue;
                    } else {
                        // FIXME
                        error!("Failed to receive input event: {}", err);
                    }
                }
                Ok(input) => match input {
                    Input::Button(cmd) => match cmd {
                        button::Command::Shutdown => {
                            if let Err(err) = self.interpreter.generic_command(
                                self.config
                                    .shutdown_command
                                    .clone()
                                    .unwrap_or("sudo shutdown -h now".to_string()),
                            ) {
                                error!("Failed to execute shutdown command: {}", err);
                            }
                        }
                        button::Command::VolumeUp => {
                            if let Err(err) = self.interpreter.generic_command(
                                self.config
                                    .volume_up_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%+".to_string()),
                            ) {
                                error!("Failed to increase volume: {}", err);
                            }
                        }
                        button::Command::VolumeDown => {
                            if let Err(err) = self.interpreter.generic_command(
                                self.config
                                    .volume_down_command
                                    .clone()
                                    .unwrap_or("amixer -q -M set PCM 10%-".to_string()),
                            ) {
                                error!("Failed to decrease volume: {}", err);
                            }
                        }
                    },
                    Input::Playback(request) => {
                        if let Err(err) = self.player.playback(request.clone()) {
                            error!("Failed to execute playback request {:?}: {}", request, err);
                        }
                        match request {
                            PlaybackRequest::Start(_) => {
                                let _ = self.interpreter.led_on();
                            }
                            PlaybackRequest::Stop => {
                                let _ = self.interpreter.led_off();
                            }
                        }
                    }
                },
            };
        }
    }
}

#[cfg(test)]
mod test {
    use rustberry::config::Config;
    use rustberry::effects::{test::TestInterpreter, Effects};
    use rustberry::input_controller::{button, Input};

    use super::*;

    #[test]
    fn jukebox_can_be_shut_down() {
        let (interpreter, effects_rx) = TestInterpreter::new();
        let interpreter =
            Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
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
        let (input_tx, input_rx) = crossbeam_channel::unbounded();
        let app = App::new(config, interpreter, &vec![input_rx]);
        for input in inputs {
            input_tx.send(input).unwrap();
        }
        drop(input_tx);
        app.run();
        let produced_effects: Vec<Effects> = effects_rx.iter().collect();

        assert_eq!(produced_effects, effects_expected);
    }
}

mod led {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::thread::{self};
    use std::time::Duration;

    use failure::Fallible;
    use rustberry::effects::Interpreter;
    use tokio::task::JoinHandle;

    pub struct Blinker {
        interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        handle: RefCell<Option<JoinHandle<()>>>,
        runtime: tokio::runtime::Runtime,
    }

    #[derive(Debug, Clone)]
    pub enum XXX {
        On(Duration),
        Off(Duration),
        Many(Vec<XXX>),
        Repeat(u32, Box<XXX>),
    }

    impl Blinker {
        pub fn new(
            interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>,
        ) -> Fallible<Self> {
            let handle: RefCell<Option<JoinHandle<()>>> = RefCell::new(None);
            let runtime = tokio::runtime::Builder::new().basic_scheduler().build()?;
            let blinker = Self {
                interpreter,
                handle,
                runtime,
            };
            Ok(blinker)
        }

        fn run(interpreter: Arc<Box<dyn Send + Sync + 'static + Interpreter>>, xxx: &XXX) {
            match xxx {
                XXX::On(duration) => {
                    let _ = interpreter.led_on();
                    thread::sleep(*duration);
                }
                XXX::Off(duration) => {
                    let _ = interpreter.led_off();
                    thread::sleep(*duration);
                }
                XXX::Many(xxxs) => {
                    for xxx in xxxs {
                        Self::run(interpreter.clone(), xxx);
                    }
                }
                XXX::Repeat(n, xxx) => {
                    for _i in 0..*n {
                        Self::run(interpreter.clone(), xxx)
                    }
                }
            }
        }

        pub fn run_async(&self, spec: &XXX) {
            // TODO: destroy old
            let interpreter = self.interpreter.clone();
            let spec = spec.clone();
            let handle = self.runtime.spawn(async move {
                Self::run(interpreter, &spec);
            });
            *(self.handle.borrow_mut()) = Some(handle);
        }
    }
}
