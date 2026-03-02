use std::path::PathBuf;
use std::sync::Arc;
use steel_host::wasmtime::{Config, Engine, Linker, OptLevel};
use steel_host::{EventRegistry, PluginLoader, PluginLoaderError, PluginManager, configure_linker};
use tokio::fs::create_dir_all;

pub async fn init(
    plugins_path: impl Into<PathBuf>,
) -> Result<(PluginManager, Arc<EventRegistry>), PluginLoaderError> {
    let mut config = Config::new();
    config.cranelift_opt_level(OptLevel::Speed);
    config.wasm_multi_memory(false);

    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);
    configure_linker(&mut linker);

    let plugins_path = plugins_path.into();
    create_dir_all(&plugins_path).await?;

    let registry = Arc::new(EventRegistry::new());
    let loader = PluginLoader::new(engine, linker, plugins_path.clone(), registry.clone());

    let discovered_plugins = loader.discover_plugins(&plugins_path).await?;

    let mut loaded_plugins = Vec::new();
    for plugin_meta in discovered_plugins {
        let loaded_plugin = loader.load_plugin(plugin_meta).await?;
        loaded_plugins.push(loaded_plugin);
    }

    let mut manager = PluginManager::new(registry.clone());
    manager.add_all(loaded_plugins);

    Ok((manager, registry))
}
