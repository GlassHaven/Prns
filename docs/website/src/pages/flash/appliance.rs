use dioxus::prelude::*;

use crate::components::MarkdownBody;
use crate::routes::Route;

const G4_SLUG: &str = "thinknode-g4";
const HELTEC_SLUG: &str = "heltec-ht-hd01-v2";

pub(super) enum Appliance {
    ThinkNodeG4,
    HeltecHtHd01V2,
}

impl Appliance {
    pub(super) fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            G4_SLUG => Some(Self::ThinkNodeG4),
            HELTEC_SLUG => Some(Self::HeltecHtHd01V2),
            _ => None,
        }
    }

    fn guide(&self) -> &'static str {
        match self {
            Self::ThinkNodeG4 => G4_GUIDE,
            Self::HeltecHtHd01V2 => HELTEC_GUIDE,
        }
    }
}

const G4_GUIDE: &str =
    include_str!("../../../../../personal-hopspot/headless/docs/g4-installation.md");
const HELTEC_GUIDE: &str =
    include_str!("../../../../../personal-hopspot/headless/docs/heltec-installation.md");
const SHARED_GUIDE: &str =
    include_str!("../../../../../personal-hopspot/appliance/docs/guided-installation.md");

pub(super) fn installation_guide(appliance: &Appliance) -> Element {
    rsx! { MarkdownBody { source: format!("{}\n\n{SHARED_GUIDE}", appliance.guide()) } }
}

#[component]
pub(super) fn LinuxApplianceCard() -> Element {
    rsx! {
        section { class: "mt-10 rounded-card border border-line/60 bg-layer/40 p-5",
            h2 { class: "text-xl font-semibold text-paper", "Linux appliances" }
            p { class: "mt-3 leading-relaxed text-soft",
                "Install Hopspot as an application on a device's existing Linux system."
            }
            Link {
                to: Route::FlashBoardPage { board: G4_SLUG.to_string() },
                class: "mt-4 inline-block text-accent hover:underline",
                "ThinkNode G4 — development installation guide"
            }
            Link {
                to: Route::FlashBoardPage { board: HELTEC_SLUG.to_string() },
                class: "mt-4 block text-accent hover:underline",
                "Heltec HT-HD01-V2 — development installation guide"
            }
        }
    }
}
