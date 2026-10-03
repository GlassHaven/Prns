use std::{
    fs,
    io::Read,
    net::SocketAddr,
    num::{NonZeroU64, NonZeroU8},
    path::{Path, PathBuf},
    process::Command,
};

use clap::{Parser, Subcommand};
use personal_hopspot_appliance::{
    Appliance, Board, Budgets, CandidateId, Error, LaunchBudget, RadioProfileError, SpaceBudget,
    UnobservedWrites,
};
use serde::Deserialize;

#[derive(Parser)]
pub struct Options {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    max_compressed_bytes: u64,
    #[arg(long)]
    max_executable_bytes: u64,
    #[arg(long)]
    flash_reserve_bytes: u64,
    #[arg(long)]
    ram_reserve_bytes: u64,
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    Radio {
        #[command(subcommand)]
        action: radio::RadioAction,
    },
    RadioPlan {
        #[arg(long)]
        profile: PathBuf,
    },
    Status,
    Stage {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        trial_launches: NonZeroU8,
    },
    Confirm {
        #[arg(long)]
        revision: NonZeroU64,
        #[arg(long)]
        executable_sha256: String,
    },
    Rollback,
    Run {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        ram_directory: PathBuf,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchConfiguration {
    listen: SocketAddr,
    tcp_mode: TcpMode,
    radio: Radio,
}

#[derive(Deserialize)]
enum TcpMode {
    Gateway,
    PointToPoint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum Radio {
    Disabled,
    HaLow { device: String, scope: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Appliance(#[from] Error),
    #[error(transparent)]
    Package(#[from] personal_hopspot_appliance::PackageError),
    #[error("host I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration or output: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid explicit launch configuration")]
    Configuration,
    #[error(transparent)]
    RadioProfile(#[from] RadioProfileError),
    #[error(transparent)]
    RadioTransaction(#[from] personal_hopspot_appliance::RadioTransactionError),
    #[error("named radio/interface sections do not match the Morse binding")]
    RadioBinding,
    #[error("pending UCI changes must be resolved before radio planning")]
    PendingUciChanges,
    #[error("could not inspect filesystem capacity")]
    Capacity,
    #[cfg(not(unix))]
    #[error("this launcher requires Unix exec")]
    UnsupportedHost,
}

pub fn run(options: Options, public_key: &str) -> Result<(), CommandError> {
    if !options.root.is_absolute() {
        return Err(CommandError::Configuration);
    }
    let board = Board::from_vendor_name(&fs::read_to_string("/tmp/sysinfo/board_name")?)?;
    if let Action::Radio { action } = options.action {
        return radio::run(
            action,
            radio::RadioContext {
                manager_root: &options.root,
                flash_reserve_bytes: options.flash_reserve_bytes,
                board,
            },
        );
    }
    if let Action::RadioPlan { profile } = &options.action {
        radio::print_plan(profile, &board)?;
        return Ok(());
    }
    let mut appliance = Appliance::open(
        &options.root,
        public_key,
        board,
        Budgets {
            compressed_bytes: options.max_compressed_bytes,
            executable_bytes: options.max_executable_bytes,
        },
        UnobservedWrites,
    )?;
    match options.action {
        Action::Radio { .. } => {
            unreachable!("radio commands returned before opening application slots")
        }
        Action::RadioPlan { .. } => unreachable!("radio planning returned before opening slots"),
        Action::Status => println!("{}", serde_json::to_string(appliance.activation())?),
        Action::Stage {
            package,
            trial_launches,
        } => {
            let space = filesystem_space(&options.root, options.flash_reserve_bytes)?;
            let verified = appliance.verify_directory(&package)?;
            let candidate = appliance.stage(verified, LaunchBudget::new(trial_launches), space)?;
            println!("{}", serde_json::to_string(&candidate)?);
        }
        Action::Confirm {
            revision,
            executable_sha256,
        } => {
            appliance.confirm(&CandidateId {
                revision,
                executable_sha256: executable_sha256
                    .try_into()
                    .map_err(|_| CommandError::Configuration)?,
            })?;
            println!("{}", serde_json::to_string(appliance.activation())?);
        }
        Action::Rollback => {
            appliance.rollback()?;
            println!("{}", serde_json::to_string(appliance.activation())?);
        }
        Action::Run {
            config,
            ram_directory,
        } => {
            if !ram_directory.starts_with("/tmp")
                || ram_directory.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
                || ram_directory.starts_with(&options.root)
            {
                return Err(CommandError::Configuration);
            }
            let mut config_bytes = Vec::new();
            fs::File::open(config)?
                .take(4097)
                .read_to_end(&mut config_bytes)?;
            if config_bytes.len() > 4096 {
                return Err(CommandError::Configuration);
            }
            let launch: LaunchConfiguration = serde_json::from_slice(&config_bytes)?;
            let mut arguments = vec![
                "--state-dir".to_owned(),
                options.root.join("state").to_string_lossy().into_owned(),
                "--listen".to_owned(),
                launch.listen.to_string(),
                "--tcp-mode".to_owned(),
                match launch.tcp_mode {
                    TcpMode::Gateway => "gateway",
                    TcpMode::PointToPoint => "point-to-point",
                }
                .to_owned(),
            ];
            match launch.radio {
                Radio::Disabled => {}
                Radio::HaLow { device, scope } => {
                    if device.is_empty()
                        || device.len() > 15
                        || !device
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                        || scope.is_empty()
                        || scope.len() > 64
                        || !scope
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                    {
                        return Err(CommandError::Configuration);
                    }
                    arguments.extend([
                        "--halow-device".to_owned(),
                        device,
                        "--halow-scope".to_owned(),
                        scope,
                    ]);
                }
            }
            let mut space = filesystem_space(Path::new("/tmp"), options.ram_reserve_bytes)?;
            let memory = fs::read_to_string("/proc/meminfo")?;
            let available_kib = memory
                .lines()
                .find_map(|line| line.strip_prefix("MemAvailable:"))
                .ok_or(CommandError::Capacity)?
                .split_whitespace()
                .next()
                .ok_or(CommandError::Capacity)?
                .parse::<u64>()
                .map_err(|_| CommandError::Capacity)?;
            space.available_bytes = space.available_bytes.min(
                available_kib
                    .checked_mul(1024)
                    .ok_or(CommandError::Capacity)?,
            );
            let (executable, candidate) = appliance.prepare_launch(&ram_directory, space)?;
            eprintln!("appliance_launch {}", serde_json::to_string(&candidate)?);
            drop(appliance);
            let mut command = Command::new(executable);
            command.args(arguments);
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                return Err(command.exec().into());
            }
            #[cfg(not(unix))]
            {
                return Err(CommandError::UnsupportedHost);
            }
        }
    }
    Ok(())
}

fn filesystem_space(path: &Path, reserve_bytes: u64) -> Result<SpaceBudget, CommandError> {
    let result = Command::new("/bin/df").arg("-Pk").arg(path).output()?;
    if !result.status.success() {
        return Err(CommandError::Capacity);
    }
    let output = String::from_utf8(result.stdout).map_err(|_| CommandError::Capacity)?;
    let row = output.lines().last().ok_or(CommandError::Capacity)?;
    let kib = row
        .split_whitespace()
        .nth(3)
        .ok_or(CommandError::Capacity)?
        .parse::<u64>()
        .map_err(|_| CommandError::Capacity)?;
    let available_bytes = kib.checked_mul(1024).ok_or(CommandError::Capacity)?;
    Ok(SpaceBudget {
        available_bytes,
        reserve_bytes,
    })
}

mod radio;

#[cfg(test)]
mod tests;
