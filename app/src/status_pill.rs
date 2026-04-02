use pepperx_ipc::LiveStatus;

/// Status pill indicator.
///
/// On compositors that support the wlr-layer-shell protocol (Sway, Hyprland)
/// this could be a floating overlay. GNOME/Mutter does NOT support layer shell,
/// so on GNOME this is a no-op — status feedback comes from sound effects and
/// the in-window overlay instead.
#[derive(Clone)]
pub struct StatusPill;

impl StatusPill {
    pub fn new() -> Self {
        Self
    }

    pub fn set_live_status(&self, _status: &LiveStatus) {
        // No-op on GNOME. Status is shown via sound effects + in-window overlay.
    }
}
