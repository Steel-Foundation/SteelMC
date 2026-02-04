use steel_macros::ClientPacket;
use steel_registry::packets::play::C_TAB_LIST;
use text_components::{TextComponent, resolving::TextResolutor};

/// Packet to set the tab list header and footer.
/// This allows servers to display custom text above and below the player list.
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_TAB_LIST)]
pub struct CTabList {
    /// The header text component (displayed above the player list)
    pub header: TextComponent,
    /// The footer text component (displayed below the player list)
    pub footer: TextComponent,
}

impl CTabList {
    /// Creates a new tab list packet with the specified header and footer.
    #[must_use]
    pub fn new<T: TextResolutor>(
        header: &TextComponent,
        footer: &TextComponent,
        player: &T,
    ) -> Self {
        Self {
            header: header.resolve(player),
            footer: footer.resolve(player),
        }
    }

    /// Creates a tab list packet with empty header and footer (clears them).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            header: TextComponent::new(),
            footer: TextComponent::new(),
        }
    }

    /// Creates a tab list packet with only a header.
    #[must_use]
    pub fn header_only<T: TextResolutor>(header: &TextComponent, player: &T) -> Self {
        Self {
            header: header.resolve(player),
            footer: TextComponent::new(),
        }
    }

    /// Creates a tab list packet with only a footer.
    #[must_use]
    pub fn footer_only<T: TextResolutor>(footer: &TextComponent, player: &T) -> Self {
        Self {
            header: TextComponent::new(),
            footer: footer.resolve(player),
        }
    }
}

impl steel_utils::serial::WriteTo for CTabList {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.header.write(writer)?;
        self.footer.write(writer)?;
        Ok(())
    }
}
