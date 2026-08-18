pub const TERMS: &str = include_str!("../legal/terms.md");
pub const PRIVACY: &str = include_str!("../legal/privacy.md");
pub const LICENSE: &str = include_str!("../legal/license.md");

pub const SUMMARY: [&str; 3] = [
    "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.",
    "The app is provided as is, with no warranties express or implied. Use is at your own risk. Nexatrode LLC is not liable for lost assets, user errors, downtime, or issues with third-party services or nodes.",
    "Running or connecting to your own Monero node is recommended. Public defaults are for convenience only.",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Document {
    Terms,
    Privacy,
    License,
}

impl Document {
    pub fn title(self) -> &'static str {
        match self {
            Self::Terms => "Terms of Use",
            Self::Privacy => "Privacy Policy",
            Self::License => "License",
        }
    }

    pub fn body(self) -> &'static str {
        match self {
            Self::Terms => TERMS,
            Self::Privacy => PRIVACY,
            Self::License => LICENSE,
        }
    }
}
