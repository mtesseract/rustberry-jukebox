use rustberry::components::access_token_provider::AccessTokenProvider;
use rustberry::config::Config;
use rustberry::effects::spotify::connect::external_command::ExternalCommand;
use slog::{self, o, Drain};
use slog_async;
use slog_term;

fn main() {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = slog::Logger::root(drain, o!());
    let _guard = slog_scope::set_global_logger(logger);

    slog_scope::scope(&slog_scope::logger().new(o!()), || main_with_log())
}

fn main_with_log() {
    let config = envy::from_env::<Config>().unwrap();
    let access_token_provider = AccessTokenProvider::new(
        &config.client_id,
        &config.client_secret,
        &config.refresh_token,
    );
    let cmd = ExternalCommand::new_from_env(&access_token_provider, "rustberry-test".to_string());
    std::thread::sleep(std::time::Duration::from_secs(60));
}
