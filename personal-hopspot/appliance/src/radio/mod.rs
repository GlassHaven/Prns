use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Board;

const G4_BOOT_ADAPTER_SHA256: &str =
    "134f5a4d6378c73d9f6670a3dfda4f10edc91f1488c01e2178eb8ca088e70782";
const HELTEC_BOOT_ADAPTER_SHA256: &str =
    "7254d7930320e502c9a4e30a88b99e9202be3b1d1990a563e784df7a93be6805";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RadioProfileError {
    #[error("invalid named UCI section")]
    Section,
    #[error("invalid Linux interface name")]
    Device,
    #[error("invalid mesh ID")]
    MeshId,
    #[error("vendor boot adapter differs from the qualified board contract")]
    BootAdapter,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct UciSection(String);

impl TryFrom<String> for UciSection {
    type Error = RadioProfileError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(RadioProfileError::Section);
        }
        Ok(Self(value))
    }
}

impl UciSection {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct RadioDevice(String);

impl TryFrom<String> for RadioDevice {
    type Error = RadioProfileError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 15
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(RadioProfileError::Device);
        }
        Ok(Self(value))
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct MeshId(String);

impl TryFrom<String> for MeshId {
    type Error = RadioProfileError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(RadioProfileError::MeshId);
        }
        Ok(Self(value))
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionalChannel {
    Us924Mhz8Mhz,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadioPreset {
    Mcs2LongGuard18Dbm,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeshPathSetup {
    ProactiveRequests,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioBinding {
    pub radio: UciSection,
    pub interface: UciSection,
    pub device: RadioDevice,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioProfile {
    pub binding: RadioBinding,
    pub channel: RegionalChannel,
    pub preset: RadioPreset,
    pub mesh_paths: MeshPathSetup,
    pub mesh_id: MeshId,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct RadioPlan {
    pub board: Board,
    pub boot_adapter_sha256: String,
    pub uci_batch: String,
}

impl RadioProfile {
    pub fn plan(&self, board: &Board, boot_adapter: &[u8]) -> Result<RadioPlan, RadioProfileError> {
        let expected = match board {
            Board::ThinkNodeG4 => G4_BOOT_ADAPTER_SHA256,
            Board::HeltecHtHd01V2 => HELTEC_BOOT_ADAPTER_SHA256,
        };
        let actual = hex::encode(Sha256::digest(boot_adapter));
        if actual != expected {
            return Err(RadioProfileError::BootAdapter);
        }
        Ok(self.qualified_plan(board, actual))
    }

    fn qualified_plan(&self, board: &Board, boot_adapter_sha256: String) -> RadioPlan {
        let radio = self.binding.radio.as_str();
        let interface = self.binding.interface.as_str();
        let device = &self.binding.device.0;
        let mesh_id = &self.mesh_id.0;
        let (country, channel) = match self.channel {
            RegionalChannel::Us924Mhz8Mhz => ("US", "44"),
        };
        let (mcs, guard, power) = match self.preset {
            RadioPreset::Mcs2LongGuard18Dbm => ("2", "0", "18"),
        };
        let mut commands = String::new();
        for (option, value) in [
            ("country", country),
            ("channel", channel),
            ("disabled", "0"),
            ("enable_fixed_rate", "1"),
            ("fixed_mcs", mcs),
            // These qualified MMRC versions use the bandwidth enum, not MHz.
            ("fixed_bw", "3"),
            ("fixed_ss", "1"),
            ("fixed_guard", guard),
            ("enable_ps", "0"),
            ("txpower", power),
        ] {
            commands.push_str(&format!("set wireless.{radio}.{option}='{value}'\n"));
        }
        commands.push_str(&format!(
            "del_list wireless.{radio}.s1g_capab='[SHORT-GI-NONE]'\n"
        ));
        commands.push_str(&format!(
            "add_list wireless.{radio}.s1g_capab='[SHORT-GI-NONE]'\n"
        ));
        for (option, value) in [
            ("ifname", device.as_str()),
            ("mode", "mesh"),
            ("mesh_id", mesh_id.as_str()),
            ("encryption", "none"),
            ("network", ""),
            ("powersave", "0"),
            ("wds", "0"),
        ] {
            commands.push_str(&format!("set wireless.{interface}.{option}='{value}'\n"));
        }
        commands.push_str(&format!("delete wireless.{interface}.key\n"));
        let root_mode = match self.mesh_paths {
            MeshPathSetup::ProactiveRequests => "2",
        };
        for (option, value) in [
            ("mesh_fwding", "0"),
            ("mesh_hwmp_rootmode", root_mode),
            ("mesh_gate_announcements", "0"),
        ] {
            commands.push_str(&format!("set mesh11sd.mesh_params.{option}='{value}'\n"));
        }
        RadioPlan {
            board: board.clone(),
            boot_adapter_sha256,
            uci_batch: commands,
        }
    }
}

#[cfg(test)]
mod tests;
