use failure::Fallible;
use slog::{self, o, Drain};
use slog_async;
use slog_scope::{error, info, warn};
use slog_term;
use tokio::stream::StreamExt;

use futures_util::TryFutureExt;
use rustberry::config::Config;
use rustberry::effects::{test::{TestInterpreterFactory,  TestInterpreter}, DynInterpreter, DynInterpreterFactory, Interpreter, ProdInterpreter, ProdInterpreterFactory};
use rustberry::input_controller::{
    button, mock, playback, InputSourceFactory, ProdInputSourceFactory,
};

use rustberry::led::{self, Blinker};

use rustberry::meta_app::{AppMode, MetaApp};

fn main() -> Fallible<()> {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || main_with_log())
}

async fn create_mock_meta_app(config: Config) -> Fallible<MetaApp> {
    warn!("Creating Mock Application");

    let isf = Box::new(mock::MockInputSourceFactory::new()?)
        as Box<dyn InputSourceFactory + Sync + Send + 'static>;

    let (interpreter_factory, mut interpreted_effects) = TestInterpreterFactory::new();
    let interpreter_factory =
        Box::new(interpreter_factory) as DynInterpreterFactory;

    // let blinker = Blinker::new(interpreter.clone())?;

    tokio::spawn(async move {
        while let Some(eff) = interpreted_effects.next().await {
            info!("Mock interpreter received effect: {:?}", eff);
        }
    });

    let application = MetaApp::new(config, interpreter_factory, isf).await?;
    Ok(application)
}

async fn create_production_meta_app(config: Config) -> Fallible<MetaApp> {
    info!("Creating Production Application");
    // Create Effects Channel and Interpreter.
    let interpreter_factory = ProdInterpreterFactory::new(&config);
    let interpreter_factory =
        Box::new(interpreter_factory) as DynInterpreterFactory;

    info!("Creating Input Source Factory");
    let mut isf = ProdInputSourceFactory::new()?;
    isf.with_buttons(Box::new(|| button::cdev_gpio::CdevGpio::new_from_env()));
    isf.with_playback(Box::new(|| {
        playback::rfid::PlaybackRequestTransmitterRfid::new()
    }));

    Ok(MetaApp::new(config, interpreter_factory, Box::new(isf)).await?)
}

fn main_with_log() -> Fallible<()> {
    let config = envy::from_env::<Config>()?;
    info!("Configuration"; o!("device_name" => &config.device_name));

    let mut runtime = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let application = if std::env::var("MOCK_MODE")
            .map(|x| x == "YES")
            .unwrap_or(false)
        {
            create_mock_meta_app(config).await?
        } else {
            create_production_meta_app(config).await?
        };

        {
            let handle = application.handle();
            tokio::spawn(async move {
                handle.is_ready().await;
                if let Err(err) = handle.set_mode(AppMode::Jukebox).await {
                    error!("Failed to activate Jukebox mode: {}", err);
                }
            });
        }

        application
            .run(None)
            .map_err(|err| {
                warn!("Meta App loop terminated, terminating application: {}", err);
                err
            })
            .await
    })
}

#[cfg(test)]
mod test {
    use rustberry::config::Config;
    use rustberry::effects::{test::TestInterpreter, Effects};
    use rustberry::input_controller::{button, Input};

    use super::*;

    #[test]
    fn jukebox_can_be_shut_down() -> Fallible<()> {
        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_async::Async::new(drain).build().fuse();
        let logger = slog::Logger::root(drain, o!());
        let _guard = slog_scope::set_global_logger(logger);
        slog_scope::scope(&slog_scope::logger().new(o!()), || {
            let mut runtime = tokio::runtime::Builder::new()
                .threaded_scheduler()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let (interpreter, effects_rx) = TestInterpreter::new();
                let interpreter =
                    Arc::new(Box::new(interpreter) as Box<dyn Interpreter + Send + Sync + 'static>);
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
                let blinker = Blinker::new(interpreter.clone())?;
                let inputs = vec![Input::Button(button::Command::Shutdown)];
                let effects_expected =
                    vec![Effects::GenericCommand("sudo shutdown -h now".to_string())];
                let app = MetaApp::new(config, interpreter, blinker, Box::new(inputs)).await?;
                let handle = app.handle();

                let (f, abortable) = futures::future::abortable(app.run(Some(AppMode::Jukebox)));
                tokio::spawn(f);
                tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
                abortable.abort();

                let produced_effects: Vec<Effects> = effects_rx.collect().await;

                assert_eq!(produced_effects, effects_expected);
                Ok(())
            })
        })
    }
}
