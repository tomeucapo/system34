use super::terminal::TerminalKey;

/// Current operation shown by the virtual workstation link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TwinaxActivity {
    #[default]
    Idle,
    Polling,
    HostToStation,
    StationToHost,
    Attention,
    Check,
}

/// Observable state for the 5251 and System/34 workstation-attachment lamps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TwinaxIndicators {
    pub online: bool,
    pub selected: bool,
    pub polling: bool,
    pub transmitting: bool,
    pub receiving: bool,
    pub attention: bool,
    pub check: bool,
}

/// Logical Twinax workstation link used when no physical Twinax adapter is
/// attached. This models station selection, host/device transfers and status;
/// it does not simulate Twinax voltages, bit timing or electrical framing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualTwinaxBus {
    online: bool,
    selected: bool,
    activity: TwinaxActivity,
    attention: bool,
    check: bool,
    polls: u64,
    host_bytes_sent: u64,
    keyboard_events_received: u64,
}

impl Default for VirtualTwinaxBus {
    fn default() -> Self {
        Self {
            online: true,
            selected: false,
            activity: TwinaxActivity::Idle,
            attention: false,
            check: false,
            polls: 0,
            host_bytes_sent: 0,
            keyboard_events_received: 0,
        }
    }
}

impl VirtualTwinaxBus {
    pub fn is_online(&self) -> bool {
        self.online
    }

    pub fn activity(&self) -> TwinaxActivity {
        self.activity
    }

    pub fn indicators(&self) -> TwinaxIndicators {
        TwinaxIndicators {
            online: self.online,
            selected: self.selected,
            polling: self.activity == TwinaxActivity::Polling,
            transmitting: self.activity == TwinaxActivity::HostToStation,
            receiving: self.activity == TwinaxActivity::StationToHost,
            attention: self.attention || self.activity == TwinaxActivity::Attention,
            check: self.check || self.activity == TwinaxActivity::Check,
        }
    }

    pub fn poll_count(&self) -> u64 {
        self.polls
    }

    pub fn host_bytes_sent(&self) -> u64 {
        self.host_bytes_sent
    }

    pub fn keyboard_events_received(&self) -> u64 {
        self.keyboard_events_received
    }

    pub fn set_online(&mut self, online: bool) {
        self.online = online;
        if !online {
            self.selected = false;
            self.activity = TwinaxActivity::Idle;
        }
    }

    /// Simulates the host polling and selecting the console station.
    pub fn poll_station(&mut self) -> bool {
        if !self.online || self.check {
            return false;
        }
        self.polls += 1;
        self.selected = true;
        self.activity = TwinaxActivity::Polling;
        true
    }

    /// Simulates a host-to-display data transfer.
    pub fn send_to_station(&mut self, bytes: &[u8]) -> bool {
        if !self.online || self.check {
            return false;
        }
        self.selected = true;
        self.activity = TwinaxActivity::HostToStation;
        self.host_bytes_sent += bytes.len() as u64;
        true
    }

    /// Simulates keyboard data being returned from the station to the host.
    pub fn receive_from_station(&mut self, keys: &[TerminalKey]) -> bool {
        if !self.online || self.check {
            return false;
        }
        self.selected = true;
        self.activity = TwinaxActivity::StationToHost;
        self.keyboard_events_received += keys.len() as u64;
        true
    }

    pub fn signal_attention(&mut self) {
        if self.online {
            self.selected = true;
            self.attention = true;
            self.activity = TwinaxActivity::Attention;
        }
    }

    pub fn acknowledge_attention(&mut self) {
        self.attention = false;
        if self.activity == TwinaxActivity::Attention {
            self.finish_exchange();
        }
    }

    pub fn signal_check(&mut self) {
        self.check = true;
        self.selected = false;
        self.activity = TwinaxActivity::Check;
    }

    pub fn reset_check(&mut self) {
        self.check = false;
        self.finish_exchange();
    }

    /// Ends a simulated poll or data exchange, returning the line to idle.
    pub fn finish_exchange(&mut self) {
        self.selected = false;
        if !self.check && !self.attention {
            self.activity = TwinaxActivity::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TwinaxActivity, VirtualTwinaxBus};
    use crate::system34::terminal::TerminalKey;

    #[test]
    fn polling_and_transfers_drive_link_indicators_and_counters() {
        let mut bus = VirtualTwinaxBus::default();

        assert!(bus.poll_station());
        assert_eq!(bus.poll_count(), 1);
        assert!(bus.indicators().polling);
        assert!(bus.indicators().selected);

        assert!(bus.send_to_station(b"HELLO"));
        assert_eq!(bus.activity(), TwinaxActivity::HostToStation);
        assert_eq!(bus.host_bytes_sent(), 5);
        assert!(bus.indicators().transmitting);

        assert!(bus.receive_from_station(&[TerminalKey::Enter]));
        assert_eq!(bus.keyboard_events_received(), 1);
        assert!(bus.indicators().receiving);
        bus.finish_exchange();
        assert_eq!(bus.activity(), TwinaxActivity::Idle);
        assert!(!bus.indicators().selected);
    }

    #[test]
    fn offline_and_check_conditions_block_bus_operations() {
        let mut bus = VirtualTwinaxBus::default();
        bus.set_online(false);

        assert!(!bus.poll_station());
        assert!(!bus.send_to_station(b"NO"));
        assert!(!bus.receive_from_station(&[TerminalKey::Enter]));
        assert_eq!(bus.poll_count(), 0);

        bus.set_online(true);
        bus.signal_check();
        assert!(bus.indicators().check);
        assert!(!bus.poll_station());
        bus.reset_check();
        assert!(bus.poll_station());
    }

    #[test]
    fn attention_remains_latched_until_acknowledged() {
        let mut bus = VirtualTwinaxBus::default();
        bus.signal_attention();
        bus.finish_exchange();

        assert!(bus.indicators().attention);
        assert_eq!(bus.activity(), TwinaxActivity::Attention);
        bus.acknowledge_attention();
        assert!(!bus.indicators().attention);
        assert_eq!(bus.activity(), TwinaxActivity::Idle);
    }
}
