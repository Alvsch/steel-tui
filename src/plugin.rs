use std::path::PathBuf;
use steel_host::error::PluginLoaderError;
use steel_host::wasmtime::{Config, OptLevel};
use steel_host::{PluginHost, discover_plugins};
use tokio::fs::create_dir_all;
use tracing::error;

pub async fn init(plugins_folder: impl Into<PathBuf>) -> Result<PluginHost, PluginLoaderError> {
    let mut config = Config::new();
    config.cranelift_opt_level(OptLevel::Speed);
    config.wasm_multi_memory(false);

    let plugins_folder = plugins_folder.into();
    create_dir_all(&plugins_folder).await?;

    let mut host = PluginHost::new(config, plugins_folder.clone())?;

    let discovered_plugins = discover_plugins(&plugins_folder).await?;
    for plugin_meta in discovered_plugins {
        let store = host.load_plugin(plugin_meta).await?;
        if let Err(err) = store.enable_plugin().await {
            let lock = store.lock().await;
            let name = lock.data().meta.name.as_str();
            error!("Error enabling plugin {name}: {err}");
        }
    }

    Ok(host)
}
