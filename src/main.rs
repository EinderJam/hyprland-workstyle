mod args;
mod config;
mod util;

use std::{error::Error, process, sync::Arc};

use args::Args;
use config::Config;
use fslock::LockFile;
use log::{debug, error};
use simple_logger::SimpleLogger;
use hyprland::{event_listener::{EventListener}, shared::{HyprError,HyprData}};
use hyprland::data::{Client,Clients,Workspaces};
use hyprland::dispatch::{Dispatch,DispatchType::RenameWorkspace};
use std::collections::HashMap;

fn update_workspace_name(
    config: & Config,
    workspace: &(i32, Vec<Client>),
    args: &Args
) -> Result<(), Box<dyn Error>> {
    let mut icons: Vec<String> = workspace.1
        .iter()
        .map(|window| {
            //ignore unmapped windows
            if window.mapped == false {
                debug!("Ignoring unmapped window: {:?}", window);
                return String::new();
            }
            // Wayland Exact app
            let exact_name = match window.class.len() {
                0 => None,
                _ => Some(&window.class),
            };

            if let Some(exact_name) = exact_name {
                config
                    .fetch_icon(exact_name, Some(&window.title))
            } else {
                error!(
                    "No exact name found for class={:?} and title={:?}",
                    window.class, window.title
                );
                // Overwrite right to left characters: https://www.unicode.org/versions/Unicode12.0.0/UnicodeStandard-12.0.pdf#G26.16327
                format!("\u{202D}{}\u{202C}",
                    config.fetch_icon(&String::new(), Some(&window.title))
                )
                    
            }
        })
        .collect();

    let name = match &workspace.1.len() {
        0 => "",
        _ => &workspace.1[0].workspace.name,
    };

    let index = workspace.0;

    if args.deduplicate {
        icons.dedup();
    }

    let mut icons = icons.join(" ");
    if !icons.is_empty() {
        icons.push(' ');
    }

    let new_name = if !icons.is_empty() {
        format!("{}: {}", index, icons)
    } else {
        format!("{}", index)
    };

    if *name != new_name {
        debug!("rename workspace {} to \"{}\"", index, new_name);

        Dispatch::call(RenameWorkspace(index, Some(&new_name))).unwrap_or_else(|_| panic!("Failed to rename workspace number {}", index));
    }

    Ok(())
}

fn update_workspaces(
    config: &Config,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let clients: Clients = Clients::get().unwrap_or_else(|e| {
        error!("failed to get clients: {}", e);
        process::exit(1)
    });

    let mut workspaces: HashMap<i32, Vec<Client>> = HashMap::new();
    for client in clients {
        let workspace_clients = workspaces.entry(client.workspace.id).or_default();
        workspace_clients.push(client);
    }

    for workspace in workspaces {
        if let Err(e) = update_workspace_name(config, &workspace, args) {
            error!("failed to update workspace: {}", e);
        }
    }
    //rename empty workspaces to their index
    Workspaces::get().unwrap().for_each(|workspace| {
        if workspace.windows == 0 {
            update_workspace_name(config, &(workspace.id,vec![]), args).unwrap();
        }
    });


    Ok(())
}



async fn subscribe_to_window_events(
    config: Config,
    args: Args,
) -> Result<(), HyprError> {
    debug!("Subscribing to window events");
    let mut event_listener = EventListener::new();
    let config = Arc::new(config);
    let config1 = config.clone();
    let config2 = config.clone();
    let args = Arc::new(args);
    let args1 = args.clone();
    let args2 = args.clone();

    event_listener.add_window_open_handler(move |_| {
        if let Err(e) = update_workspaces(&config, &args) {
            error!("failed to update workspaces: {}", e);
        }
    });
    event_listener.add_window_close_handler(move |_| {
        if let Err(e) = update_workspaces(&config1, &args1) {
            error!("failed to update workspaces: {}", e);
        }
    });
    event_listener.add_window_moved_handler(move |_| {
        if let Err(e) = update_workspaces(&config2, &args2) {
            error!("failed to update workspaces: {}", e);
        }
    });

    event_listener.start_listener()
}

fn check_already_running() {
    let mut file = match LockFile::open("/tmp/hworkstyle.lock") {
        Ok(f) => f,
        _ => return,
    };

    let locked = file.try_lock().unwrap();

    if !locked {
        error!("hypr-workstyle already running");
        process::exit(1)
    }

    ctrlc::set_handler(move || {
        debug!("Unlocking /tmp/hworkstyle.lock");
        file.unlock().unwrap();
        process::exit(0)
    })
    .expect("Could not set ctrlc handler")
}

#[async_std::main]
async fn main() {
    let args = Args::from_cli();

    SimpleLogger::new()
        .with_level(args.log_level)
        .init()
        .expect("Could not load simple logger");

    check_already_running();
    
    let config = Config::new(args.config_path.clone());
    update_workspaces(&config, &args).expect("failed to update workspaces");

    subscribe_to_window_events(config, args).await.expect("failed to subscribe to window events");
}
