mod config;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use config::{Config, UserConfig};
use futures::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest::multipart;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use walkdir::WalkDir;

/// Command-line arguments for the Immich uploader.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,

    /// Immich server URL (e.g., http://192.168.1.10:2283).
    /// Overrides configuration file settings.
    #[arg(short, long, env = "IMMICH_SERVER_URL")]
    server: Option<String>,

    /// Immich API key.
    /// Overrides configuration file settings.
    #[arg(short, long, env = "IMMICH_API_KEY")]
    key: Option<String>,

    /// Use a specific user from the configuration.
    /// Overrides the default current user.
    #[arg(short, long)]
    user: Option<String>,

    /// Number of concurrent uploads to perform.
    #[arg(short, long, default_value_t = 10)]
    concurrent: usize,
}

/// Main subcommands for the application.
#[derive(Subcommand)]
enum Commands {
    /// Upload photos and videos from a directory to the Immich server.
    Upload {
        /// Directory to scan for media files.
        directory: PathBuf,

        /// Whether to scan subdirectories recursively.
        #[arg(short, long, default_value_t = true)]
        recursive: bool,

        /// Skip files that have already been uploaded (if possible).
        #[arg(short, long, default_value_t = false)]
        skip_existing: bool,
    },
    /// Manage stored user credentials and server URLs.
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
}

/// Subcommands for user management.
#[derive(Subcommand)]
enum UserCommands {
    /// Add a new user configuration.
    Add {
        /// Name to identify the user configuration.
        name: String,
        /// Immich server URL.
        #[arg(short, long)]
        server: String,
        /// Immich API key.
        #[arg(short, long)]
        key: String,
        /// Whether to set this as the default user.
        #[arg(short, long, default_value_t = false)]
        default: bool,
    },
    /// List all configured users.
    List,
    /// Delete a user configuration by name.
    Delete {
        /// Name of the user to remove.
        name: String,
    },
    /// Set a specific user as the default for uploads.
    Default {
        /// Name of the user to set as default.
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let mut config = Config::load()?;

    match cli.command {
        Commands::User { command } => match command {
            UserCommands::Add {
                name,
                server,
                key,
                default,
            } => {
                config.users.insert(
                    name.clone(),
                    UserConfig {
                        api_key: key,
                        server_url: server,
                    },
                );
                if default || config.current_user.is_none() {
                    config.current_user = Some(name.clone());
                }
                config.save()?;
                println!("User '{}' added successfully.", name);
            }
            UserCommands::List => {
                if config.users.is_empty() {
                    println!("No users configured.");
                } else {
                    println!("Users:");
                    for (name, user) in &config.users {
                        let current = if config.current_user.as_ref() == Some(name) {
                            "*"
                        } else {
                            " "
                        };
                        println!(" {} {}: {}", current, name, user.server_url);
                    }
                }
            }
            UserCommands::Delete { name } => {
                if config.users.remove(&name).is_some() {
                    if config.current_user.as_ref() == Some(&name) {
                        config.current_user = None;
                    }
                    config.save()?;
                    println!("User '{}' deleted.", name);
                } else {
                    anyhow::bail!("User '{}' not found.", name);
                }
            }
            UserCommands::Default { name } => {
                if config.users.contains_key(&name) {
                    config.current_user = Some(name.clone());
                    config.save()?;
                    println!("Default user set to '{}'.", name);
                } else {
                    anyhow::bail!("User '{}' not found.", name);
                }
            }
        },
        Commands::Upload {
            directory,
            recursive,
            skip_existing: _,
        } => {
            let (server_url, api_key) = if let (Some(s), Some(k)) = (cli.server, cli.key) {
                (s, k)
            } else if let Some(user_name) = cli.user {
                let user = config
                    .users
                    .get(&user_name)
                    .with_context(|| format!("User '{}' not found in config", user_name))?;
                (user.server_url.clone(), user.api_key.clone())
            } else {
                let (_, user) = config.get_current_user().context(
                    "No current user set and no server/key or --user provided. Use 'rimmich-uploader user add' to configure one.",
                )?;
                (user.server_url.clone(), user.api_key.clone())
            };

            let server_url = server_url.trim_end_matches('/').to_string();
            let client = reqwest::Client::new();

            // Verify connectivity
            check_connection(&client, &server_url)
                .await
                .context("Failed to connect to Immich server")?;

            upload_directory(
                client,
                &server_url,
                &api_key,
                &directory,
                recursive,
                cli.concurrent,
            )
            .await?;
        }
    }

    Ok(())
}

/// Pings the Immich server to verify connectivity.
async fn check_connection(client: &reqwest::Client, server_url: &str) -> Result<()> {
    let url = format!("{}/api/server/ping", server_url);
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Server ping failed: {}", resp.status());
    }
    let body = resp.text().await?;
    // Immich ping returns "pong" on success.
    if !body.contains("pong") {
        anyhow::bail!("Unexpected response from ping: {}", body);
    }
    Ok(())
}

/// Scans a directory for media files and uploads them concurrently.
async fn upload_directory(
    client: reqwest::Client,
    server_url: &str,
    api_key: &str,
    directory: &Path,
    recursive: bool,
    concurrent: usize,
) -> Result<()> {
    if !directory.is_dir() {
        anyhow::bail!("Path {:?} is not a directory", directory);
    }

    println!("Scanning directory: {:?}", directory);
    let mut files = Vec::new();
    let walker = if recursive {
        WalkDir::new(directory)
    } else {
        WalkDir::new(directory).max_depth(1)
    };

    // Filter files by mime type (images and videos).
    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path();
            if is_image_or_video(path) {
                files.push(path.to_path_buf());
            }
        }
    }

    if files.is_empty() {
        println!("No supported files found in {:?}", directory);
        return Ok(());
    }

    println!(
        "Found {} files to upload. Starting upload with concurrency {}...",
        files.len(),
        concurrent
    );

    let m = MultiProgress::new();
    let pb = m.add(ProgressBar::new(files.len() as u64));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")?
            .progress_chars("#>-"),
    );

    let client = Arc::new(client);
    let server_url = Arc::new(server_url.to_string());
    let api_key = Arc::new(api_key.to_string());
    let device_id = "rimmich-uploader";

    // Use a stream to process uploads concurrently with a limit.
    let mut requests = futures::stream::iter(files)
        .map(|path| {
            let client = Arc::clone(&client);
            let server_url = Arc::clone(&server_url);
            let api_key = Arc::clone(&api_key);
            let pb = pb.clone();
            async move {
                let result = upload_file(&client, &server_url, &api_key, &path, device_id).await;
                match result {
                    Ok(_) => {
                        pb.inc(1);
                    }
                    Err(e) => {
                        pb.println(format!("Failed to upload {:?}: {}", path, e));
                        pb.inc(1); // Still increment but mark failure in output
                    }
                }
            }
        })
        .buffer_unordered(concurrent);

    // Consume the stream.
    while requests.next().await.is_some() {}

    pb.finish_with_message("Upload complete");

    Ok(())
}

/// Checks if a file path corresponds to a supported image or video mime type.
fn is_image_or_video(path: &Path) -> bool {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mime_str = mime.to_string();
    mime_str.starts_with("image/") || mime_str.starts_with("video/")
}

/// Uploads a single file to the Immich server with appropriate metadata.
async fn upload_file(
    client: &reqwest::Client,
    server_url: &str,
    api_key: &str,
    path: &Path,
    device_id: &str,
) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    // Use file creation time if available, otherwise fallback to modification time or current time.
    let created_at: DateTime<Utc> = metadata
        .created()
        .or_else(|_| metadata.modified())
        .unwrap_or_else(|_| SystemTime::now())
        .into();
    let modified_at: DateTime<Utc> = metadata
        .modified()
        .unwrap_or_else(|_| SystemTime::now())
        .into();

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid filename")?;

    // Create a stable deviceAssetId from path hash to avoid duplicate uploads in some contexts.
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let device_asset_id = format!("{}-{}", device_id, hasher.finish());

    let file_bytes = tokio::fs::read(path).await?;
    let part = multipart::Part::bytes(file_bytes)
        .file_name(filename.to_string())
        .mime_str(
            &mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
        )?;

    let form = multipart::Form::new()
        .part("assetData", part)
        .text("deviceAssetId", device_asset_id)
        .text("deviceId", device_id.to_string())
        .text("fileCreatedAt", created_at.to_rfc3339())
        .text("fileModifiedAt", modified_at.to_rfc3339())
        .text("isFavorite", "false");

    let url = format!("{}/api/assets", server_url);

    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .multipart(form)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        // If it's 409 Conflict, it means it's already there (behavior depends on Immich API version).
        if status == reqwest::StatusCode::CONFLICT || body.contains("already exists") {
            return Ok(());
        }
        anyhow::bail!("Server returned error {}: {}", status, body);
    }

    Ok(())
}
