//! Explicit Remote Control provisioning; no autonomous application announcements.
use personal_rns::identity::{PublicIdentityMaterial, IDENTITY_PUBLIC_KEY_LEN};
use personal_rns::prelude::*;
use personal_rns::remote_control::RemoteControlControllerAuthority;

#[derive(Debug, clap::Args)]
pub struct ControlOptions {
    /// Authorized controller public identity (128 hex characters); repeat for more controllers.
    /// Grants Describe and AnnounceSelf only. No value supplies no initial grants.
    #[arg(long, value_parser = controller)]
    pub controller_public_key: Vec<RemoteControlControllerIdentity>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("controller grants are invalid: {0:?}")]
    Grants(RemoteControlControllerGrantsError),
    #[error("controller permissions are invalid: {0:?}")]
    Permissions(RemoteControlControllerGrantError),
}

pub fn operator_requests() -> RemoteControlRequestSet {
    let mut requests = RemoteControlRequestSet::only(RemoteControlRequestKind::Describe);
    requests.insert(RemoteControlRequestKind::AnnounceSelf);
    requests
}

pub fn public_identity(value: &str) -> Result<PublicIdentityMaterial, hex::FromHexError> {
    let mut bytes = [0; IDENTITY_PUBLIC_KEY_LEN];
    hex::decode_to_slice(value, &mut bytes)?;
    Ok(PublicIdentityMaterial::from_bytes(bytes))
}

fn controller(value: &str) -> Result<RemoteControlControllerIdentity, hex::FromHexError> {
    public_identity(value)
        .map(|material| RemoteControlControllerIdentity::new(material.public_keys()))
}

impl ControlOptions {
    pub fn grants(&self) -> Result<Vec<RemoteControlControllerGrant>, Error> {
        let grants = self
            .controller_public_key
            .iter()
            .map(|controller| {
                RemoteControlControllerGrant::new(
                    *controller,
                    RemoteControlControllerAuthority::Operator,
                    operator_requests(),
                )
                .map_err(Error::Permissions)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !grants.is_empty() {
            RemoteControlControllerGrants::try_from(grants.as_slice()).map_err(Error::Grants)?;
        }
        Ok(grants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        control: ControlOptions,
    }

    #[test]
    fn grants_are_explicit_bounded_and_nonadministrative() {
        assert!(Cli::try_parse_from(["host"])
            .unwrap()
            .control
            .grants()
            .unwrap()
            .is_empty());
        assert!(Cli::try_parse_from(["host", "--controller-public-key", "12"]).is_err());
        let key = "21".repeat(64);
        let options = Cli::try_parse_from(["host", "--controller-public-key", &key]).unwrap();
        let grants = options.control.grants().unwrap();
        assert_eq!(
            grants[0].authority(),
            RemoteControlControllerAuthority::Operator
        );
        assert_eq!(grants[0].effective_requests(), operator_requests());
        let duplicate = Cli::try_parse_from([
            "host",
            "--controller-public-key",
            &key,
            "--controller-public-key",
            &key,
        ])
        .unwrap();
        assert!(matches!(
            duplicate.control.grants(),
            Err(Error::Grants(
                RemoteControlControllerGrantsError::Duplicate { .. }
            ))
        ));
    }
}
