#![warn(unused, missing_docs)]
//!
//! This is an alternative crate registry for use with Cargo, written in Rust.
//!
//! This repository implements the Cargo APIs and interacts with a crate index as specified in the [Cargo's Alternative Registries RFC].  
//! This allows to have a private registry to host crates that are specific to what your doing and not suitable for publication on [crates.io] while maintaining the same build experience as with crates from [crates.io].  
//!
//! [crates.io]: https://crates.io
//! [Cargo's Alternative Registries RFC]: https://github.com/rust-lang/rfcs/blob/master/text/2141-alternative-registries.md#registry-index-format-specification
//!
//! Goals
//! -----
//!
//! - Offer customizable crate storage strategies (local on-disk, S3, Git Repo, etc...).
//! - Offer multiple backing database options (MySQL, PostgreSQL or SQLite).
//! - An optional integrated (server-side rendered) front-end.
//!

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
#[macro_use]
extern crate log;
#[macro_use(slog_o)]
extern crate slog;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_std::fs;

use clap::{App, Arg};
use tide::http::mime;
use tide::utils::After;
use tide::{Body, Response, Server};

/// API endpoints definitions.
pub mod api;
/// Configuration and internal state type definitions.
pub mod config;
/// Database abstractions module.
pub mod db;
/// Error-related type definitions.
pub mod error;
/// Logs initialisation.
pub mod logs;
/// Various utilities and helpers.
pub mod utils;

/// Frontend endpoints definitions.
#[cfg(feature = "frontend")]
pub mod frontend;

use crate::config::Config;
use crate::error::Error;
use crate::utils::request_log::RequestLogger;

#[cfg(feature = "frontend")]
use crate::utils::auth::AuthMiddleware;
#[cfg(feature = "frontend")]
use crate::utils::cookies::CookiesMiddleware;

/// The instantiated [`crate::db::Repo`] type alias.
pub type Repo = db::Repo<db::Connection>;

/// The application state type used for the web server.
pub type State = Arc<config::State>;

#[cfg(feature = "mysql")]
embed_migrations!("../migrations/mysql");
#[cfg(feature = "sqlite")]
embed_migrations!("../migrations/sqlite");
#[cfg(feature = "postgres")]
embed_migrations!("../migrations/postgres");

#[cfg(feature = "frontend")]
fn frontend_routes(state: State, assets_path: PathBuf) -> io::Result<Server<State>> {
    let mut app = tide::with_state(Arc::clone(&state));

    info!("setting up cookie middleware");
    app.middleware(CookiesMiddleware::new());
    info!("setting up authentication middleware");
    app.middleware(AuthMiddleware::new());

    info!("mounting '/'");
    app.at("/").get(frontend::index::get);
    info!("mounting '/me'");
    app.at("/me").get(frontend::me::get);
    info!("mounting '/search'");
    app.at("/search").get(frontend::search::get);
    info!("mounting '/most-downloaded'");
    app.at("/most-downloaded")
        .get(frontend::most_downloaded::get);
    info!("mounting '/last-updated'");
    app.at("/last-updated").get(frontend::last_updated::get);
    info!("mounting '/crates/:crate'");
    app.at("/crates/:crate").get(frontend::krate::get);

    info!("mounting '/account/login'");
    app.at("/account/login")
        .get(frontend::account::login::get)
        .post(frontend::account::login::post);
    info!("mounting '/account/logout'");
    app.at("/account/logout")
        .get(frontend::account::logout::get);
    info!("mounting '/account/register'");
    app.at("/account/register")
        .get(frontend::account::register::get)
        .post(frontend::account::register::post);
    info!("mounting '/account/manage'");
    app.at("/account/manage")
        .get(frontend::account::manage::get);
    info!("mounting '/account/manage/password'");
    app.at("/account/manage/password")
        .post(frontend::account::manage::passwd::post);
    info!("mounting '/account/manage/tokens'");
    app.at("/account/manage/tokens")
        .post(frontend::account::manage::tokens::post);
    info!("mounting '/account/manage/tokens/:token-id/revoke'");
    app.at("/account/manage/tokens/:token-id/revoke")
        .get(frontend::account::manage::tokens::revoke::get);

    info!("mounting '/assets/*path'");
    app.at("/assets").serve_dir(assets_path)?;

    Ok(app)
}

fn api_routes(state: State) -> Server<State> {
    let mut app = tide::with_state(state);

    // Transform endpoint errors into the format expected by Cargo.
    app.middleware(After(|mut res: Response| async {
        if let Some(err) = res.error() {
            let payload = json::json!({
                "errors": [{
                    "detail": err.to_string(),
                }]
            });
            res.set_status(200);
            res.set_content_type(mime::JSON);
            res.set_body(Body::from_json(&payload)?);
        }
        Ok(res)
    }));

    info!("mounting '/api/v1/account/register'");
    app.at("/account/register")
        .post(api::account::register::post);
    info!("mounting '/api/v1/account/login'");
    app.at("/account/login").post(api::account::login::post);
    info!("mounting '/api/v1/account/tokens'");
    app.at("/account/tokens")
        .post(api::account::token::info::post)
        .put(api::account::token::generate::put)
        .delete(api::account::token::revoke::delete);
    info!("mounting '/api/v1/account/tokens/:name'");
    app.at("/account/tokens/:name")
        .get(api::account::token::info::get);
    info!("mounting '/api/v1/categories'");
    app.at("/categories").get(api::categories::get);
    info!("mounting '/api/v1/crates'");
    app.at("/crates").get(api::crates::search::get);
    info!("mounting '/api/v1/crates/new'");
    app.at("/crates/new").put(api::crates::publish::put);
    info!("mounting '/api/v1/crates/suggest'");
    app.at("/crates/suggest").get(api::crates::suggest::get);
    info!("mounting '/api/v1/crates/:name'");
    app.at("/crates/:name").get(api::crates::info::get);
    info!("mounting '/api/v1/crates/:name/owners'");
    app.at("/crates/:name/owners")
        .get(api::crates::owners::get)
        .put(api::crates::owners::put)
        .delete(api::crates::owners::delete);
    info!("mounting '/api/v1/crates/:name/:version/yank'");
    app.at("/crates/:name/:version/yank")
        .delete(api::crates::yank::delete);
    info!("mounting '/api/v1/crates/:name/:version/unyank'");
    app.at("/crates/:name/:version/unyank")
        .put(api::crates::unyank::put);
    info!("mounting '/api/v1/crates/:name/:version/download'");
    app.at("/crates/:name/:version/download")
        .get(api::crates::download::get);

    app
}

async fn run() -> Result<(), Error> {
    let matches = App::new("alexandrie")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("alexandrie.toml")
                .help("config file path")
                .takes_value(true),
        )
        .get_matches();
    let config = matches.value_of("config").unwrap_or("alexandrie.toml");

    let contents = fs::read(config).await?;
    let config: Config = toml::from_slice(contents.as_slice())?;
    let addr = config.general.bind_address.clone();

    #[cfg(feature = "frontend")]
    let frontend_enabled = config.frontend.enabled;
    #[cfg(feature = "frontend")]
    let assets_path = config.frontend.assets.path.clone();
    let state: Arc<config::State> = Arc::new(config.into());

    info!("running database migrations");
    #[rustfmt::skip]
    state.repo.run(|conn| embedded_migrations::run(conn)).await
        .expect("migration execution error");

    let mut app = tide::with_state(Arc::clone(&state));

    info!("setting up request logger middleware");
    app.middleware(RequestLogger::new());

    #[cfg(feature = "frontend")]
    if frontend_enabled {
        let frontend = frontend_routes(Arc::clone(&state), assets_path)?;
        app.at("/").nest(frontend);
    }
    app.at("/api/v1").nest(api_routes(state));

    info!("listening on '{0}'", addr);
    app.listen(addr).await?;

    Ok(())
}

#[async_std::main]
async fn main() {
    let _guard = logs::init();

    if let Err(err) = run().await {
        log::error!("{}", err);
    }
}
