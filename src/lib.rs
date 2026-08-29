//! Client for the [MariaDB Downloads REST API](https://mariadb.org/downloads-rest-api/).
//!
//! The crate is split so that the binary stays a thin shim: [`api`] owns HTTP
//! and response models, [`cli`] owns argument parsing and table rendering, and
//! [`install`] owns distribution repository setup.

pub mod api;
pub mod cli;
pub mod install;
