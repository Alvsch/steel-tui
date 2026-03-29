use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;
use steel_host::wasmtime::{Config, OptLevel};
use steel_host::{PluginHost, discover_plugins};
use tokio::fs::create_dir_all;

pub async fn init(plugins_folder: impl Into<PathBuf>) -> anyhow::Result<Arc<PluginHost>> {
    let mut config = Config::new();
    config.cranelift_opt_level(OptLevel::Speed);
    config.wasm_multi_memory(false);

    let plugins_folder = plugins_folder.into();
    create_dir_all(&plugins_folder)
        .await
        .context("failed to create plugin directory")?;

    let host = Arc::new(
        PluginHost::new(config, plugins_folder.clone()).expect("failed to create PluginHost"),
    );

    let discovered_plugins = discover_plugins(&plugins_folder)
        .await
        .context("failed to discover plugins")?;

    let mut plugins = Vec::new();
    for plugin_meta in discovered_plugins {
        let cloned = host.clone();
        plugins.push(tokio::spawn(async move {
            cloned.prepare_plugin(plugin_meta).await
        }));
    }

    let mut enabled = Vec::new();
    for handle in plugins.drain(..) {
        let plugin = handle
            .await
            .context("tokio thread panicked")?
            .context("failed to prepare plugin")?;

        host.load_plugin(&plugin)
            .await
            .context("failed to load plugin")?;

        host.enable_plugin(&plugin)
            .await
            .context("failed to enable plugin")?;

        enabled.push(plugin);
    }

    Ok(host)
}
