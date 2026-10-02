use super::*;
use personal_hopspot_appliance::RadioProfile;

const MAX_PROFILE_BYTES: u64 = 4096;
const MAX_BOOT_ADAPTER_BYTES: u64 = 131072;

pub(super) fn print_plan(profile: &Path, board: &Board) -> Result<(), CommandError> {
    let mut bytes = Vec::new();
    fs::File::open(profile)?
        .take(MAX_PROFILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err(CommandError::Configuration);
    }
    let profile: RadioProfile = serde_json::from_slice(&bytes)?;
    let mut adapter = Vec::new();
    fs::File::open("/lib/netifd/wireless/morse.sh")?
        .take(MAX_BOOT_ADAPTER_BYTES + 1)
        .read_to_end(&mut adapter)?;
    if adapter.len() as u64 > MAX_BOOT_ADAPTER_BYTES {
        return Err(CommandError::Configuration);
    }
    let plan = profile.plan(board, &adapter)?;
    let changes = Command::new("/sbin/uci").args(["-q", "changes"]).output()?;
    if !changes.status.success() {
        return Err(CommandError::UciInspection);
    }
    if !changes.stdout.is_empty() {
        return Err(CommandError::PendingUciChanges);
    }
    let radio = profile.binding.radio.as_str();
    let interface = profile.binding.interface.as_str();
    for (key, expected) in [
        (format!("wireless.{radio}"), "wifi-device"),
        (format!("wireless.{radio}.type"), "morse"),
        (format!("wireless.{interface}"), "wifi-iface"),
        (format!("wireless.{interface}.device"), radio),
        ("mesh11sd.mesh_params".to_owned(), "mesh11sd"),
    ] {
        let result = Command::new("/sbin/uci")
            .args(["-q", "get", &key])
            .output()?;
        if !result.status.success() || result.stdout != format!("{expected}\n").as_bytes() {
            return Err(CommandError::RadioBinding);
        }
    }
    println!("{}", serde_json::to_string(&plan)?);
    Ok(())
}
