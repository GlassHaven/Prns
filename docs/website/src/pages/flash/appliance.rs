use dioxus::prelude::*;

use crate::components::MarkdownBody;
use crate::routes::Route;

pub(super) const G4_SLUG: &str = "thinknode-g4";
const G4_GUIDE: &str =
    include_str!("../../../../../personal-hopspot/headless/docs/g4-installation.md");

#[component]
pub(super) fn G4InstallationGuide() -> Element {
    rsx! { MarkdownBody { source: G4_GUIDE.to_string() } }
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
        }
    }
}
