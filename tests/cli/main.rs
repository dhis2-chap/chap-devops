//! End-to-end tests for the commands that write a deployment directory.
//!
//! Every run is `--offline` with its own cache directory, which starts with
//! the frozen marketplace data of `tests/fixtures/marketplace/`, so no test
//! touches the network or the developer's real cache.

mod adopt;
mod auth;
mod backup;
mod chap;
mod chapkit;
mod cleanup;
mod common;
mod components;
mod components_again;
mod dhis2;
mod dhis2_stand_in;
mod digest;
mod doctor;
mod expose;
mod help;
mod init;
mod jobs_api;
mod lock;
mod logs;
mod models;
mod models_add;
mod models_configs;
mod models_configs_edit;
mod models_configs_update;
mod models_new;
mod models_test;
mod models_test_config;
mod preflight;
mod retire;
mod run;
mod run_stop;
mod status;
mod top;
mod update;
mod volumes;
mod wait;
