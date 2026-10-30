use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, Vec2};
use fontdue::{Font, FontSettings};
use system34::system34::{cp::MAIN_LEVEL_MAR_LSR, memory::Memory, msp::StepOutcome, System34};
use system34::{OperatorConsole, TerminalKey};

const SCREEN_BACKGROUND: Color32 = Color32::from_rgb(4, 15, 9);
const PHOSPHOR: Color32 = Color32::from_rgb(117, 255, 157);
const PHOSPHOR_DIM: Color32 = Color32::from_rgb(54, 139, 84);
const ACTIVITY_PULSE: Duration = Duration::from_millis(450);
const FONT_SIZE: f32 = 16.0;
const INITIAL_UI_ZOOM: f32 = 1.5;
const PANEL_METAL: Color32 = Color32::from_rgb(55, 57, 56);
const PANEL_EDGE: Color32 = Color32::from_rgb(143, 146, 143);
const PANEL_INSET: Color32 = Color32::from_rgb(25, 26, 25);
const DISPLAY_FIRST_LAMP_X: f32 = 116.0;
const DISPLAY_SECOND_BYTE_FIRST_LAMP_X: f32 = 641.0;
const DISPLAY_LAMP_SPACING: f32 = 58.0;
const DISPLAY_BYTE_CENTERS: [f32; 2] = [
    DISPLAY_FIRST_LAMP_X + 3.5 * DISPLAY_LAMP_SPACING,
    DISPLAY_SECOND_BYTE_FIRST_LAMP_X + 3.5 * DISPLAY_LAMP_SPACING,
];
const ADDRESS_DIAL_CENTERS: [f32; 4] = [140.0, 420.0, 720.0, 1020.0];
const ADDRESS_DIAL_GROUP_CENTER: f32 = 580.0;
const DISPLAY_DIAL_GROUP_CENTER: f32 = 870.0;
const CE_VERTICAL_SCALE: f32 = 0.90;
const FACE_DESIGN_HEIGHT: f32 = 1060.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PanelMode {
    #[default]
    ProcessorRun,
    SystemInstructionStep,
    InstructionStepDisplayLsr,
    AlterStorage,
    AlterMarInterrupt,
    DisplayStorage,
    InstructionStepDisplayPcr,
    DisplayChecks,
}

impl PanelMode {
    const ALL: [Self; 8] = [
        Self::ProcessorRun,
        Self::SystemInstructionStep,
        Self::InstructionStepDisplayLsr,
        Self::AlterStorage,
        Self::AlterMarInterrupt,
        Self::DisplayStorage,
        Self::InstructionStepDisplayPcr,
        Self::DisplayChecks,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ProcessorRun => "PROC RUN",
            Self::SystemInstructionStep => "SYS INSN STEP",
            Self::InstructionStepDisplayLsr => "INSN STEP / DPLY LSR",
            Self::AlterStorage => "ALTER STOR",
            Self::AlterMarInterrupt => "ALTER MAR / IRPT",
            Self::DisplayStorage => "DPLY STOR",
            Self::InstructionStepDisplayPcr => "INSN STEP / DPLY PCR",
            Self::DisplayChecks => "DPLY CHKS",
        }
    }
}

#[derive(Clone, Copy)]
enum PanelAction {
    Power,
    Load,
    MountDiskette,
    Start,
    Stop,
    CeStart,
    DisplayChecks,
    Reset,
    LampTest,
}

#[derive(Clone, Copy)]
enum PanelSwitch {
    PreviousPowerFault,
    CheckRun,
    ForceClock,
    MsiplDiskette,
    CsiplDiskette,
    StorageSelect,
    AddressCompare,
}

#[derive(Clone, Copy)]
struct CeButtonSpec {
    label: &'static str,
    center_x: f32,
    center_y: f32,
    action: PanelAction,
}

struct ScreenFont {
    font: Font,
    cell_width: usize,
    line_height: usize,
    baseline: i32,
}

impl ScreenFont {
    fn new() -> Self {
        let font = Font::from_bytes(
            include_bytes!("../assets/3270-Regular.ttf") as &[u8],
            FontSettings::default(),
        )
        .expect("bundled IBM 3270 font must be a valid TrueType font");
        let metrics = font
            .horizontal_line_metrics(FONT_SIZE)
            .expect("3270 font must contain horizontal line metrics");

        Self {
            cell_width: font.metrics('M', FONT_SIZE).advance_width.round() as usize,
            line_height: metrics.new_line_size.ceil() as usize,
            baseline: metrics.ascent.ceil() as i32,
            font,
        }
    }
}

struct TerminalGui {
    machine: System34,
    screen_font: ScreenFont,
    screen_texture: TextureHandle,
    host_message: String,
    keyboard_text: String,
    last_key: String,
    message: String,
    activity_until: Option<Instant>,
    control_panel_open: bool,
    panel_power: bool,
    panel_running: bool,
    processor_check: bool,
    lamp_test: bool,
    previous_power_fault: bool,
    check_run: bool,
    force_clock_on: bool,
    msipl_diskette: bool,
    csipl_diskette: bool,
    address_compare_stop: bool,
    panel_mode: PanelMode,
    address_switches: u16,
    data_switches: u16,
    display_word: u16,
    control_storage_selected: bool,
    selected_lsr: usize,
    steps_per_frame: usize,
    diskette_path: String,
}

impl TerminalGui {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
        context.egui_ctx.set_zoom_factor(INITIAL_UI_ZOOM);
        let mut machine = System34::new(Memory::new(48));
        machine.operator_console_mut().write_text(
            "IBM SYSTEM/34    5251 DISPLAY STATION\nMODEL 11 - 24 X 80\n\nVIRTUAL TWINAX CONSOLE READY\n\nUse the host controls below to write to the display.\nSend keyboard events back to the host with the station controls.",
        );
        let screen_font = ScreenFont::new();
        let screen_texture = context.egui_ctx.load_texture(
            "5251-screen",
            screen_image(machine.operator_console(), &screen_font),
            egui::TextureOptions::NEAREST,
        );

        Self {
            machine,
            screen_font,
            screen_texture,
            host_message: String::new(),
            keyboard_text: String::new(),
            last_key: "(none)".to_owned(),
            message: "Virtual workstation bus initialized".to_owned(),
            activity_until: None,
            control_panel_open: true,
            panel_power: true,
            panel_running: false,
            processor_check: false,
            lamp_test: false,
            previous_power_fault: false,
            check_run: false,
            force_clock_on: false,
            msipl_diskette: true,
            csipl_diskette: true,
            address_compare_stop: false,
            panel_mode: PanelMode::ProcessorRun,
            address_switches: 0,
            data_switches: 0,
            display_word: 0,
            control_storage_selected: true,
            selected_lsr: MAIN_LEVEL_MAR_LSR,
            steps_per_frame: 32,
            diskette_path: String::new(),
        }
    }

    fn refresh_screen(&mut self) {
        self.screen_texture.set(
            screen_image(self.machine.operator_console(), &self.screen_font),
            egui::TextureOptions::NEAREST,
        );
    }

    fn pulse(&mut self, context: &egui::Context) {
        self.activity_until = Some(Instant::now() + ACTIVITY_PULSE);
        context.request_repaint_after(ACTIVITY_PULSE);
    }

    fn send_host_message(&mut self, context: &egui::Context) {
        let text = std::mem::take(&mut self.host_message);
        let display_text = format!("{text}\n");
        if !self.machine.write_operator_display(&display_text) {
            self.message = "Host write rejected: station offline or in CHECK".to_owned();
            return;
        }
        self.message = format!("Host sent {} byte(s) to station", display_text.len());
        self.refresh_screen();
        self.pulse(context);
    }

    fn send_keyboard_text(&mut self, context: &egui::Context) {
        let text = std::mem::take(&mut self.keyboard_text);
        let keys: Vec<_> = text.chars().map(TerminalKey::Character).collect();
        if !self.machine.receive_operator_keys(&keys) {
            self.message = "Keyboard input rejected: station offline or in CHECK".to_owned();
            return;
        }
        if let Some(TerminalKey::Character(character)) = keys.last() {
            self.last_key = format!("Character({character:?})");
        }
        self.message = format!("Station returned {} keyboard event(s)", keys.len());
        self.pulse(context);
    }

    fn send_station_key(&mut self, key: TerminalKey, context: &egui::Context) {
        if !self.machine.receive_operator_keys(&[key]) {
            self.message = "Keyboard input rejected: station offline or in CHECK".to_owned();
            return;
        }
        self.last_key = format!("{key:?}");
        self.message = format!("Station returned {key:?}");
        self.pulse(context);
    }

    fn finish_timed_activity(&mut self) {
        if self
            .activity_until
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.machine.virtual_twinax_bus_mut().finish_exchange();
            self.activity_until = None;
        }
    }

    fn service_processor_run(&mut self, context: &egui::Context) {
        if !self.panel_power || !self.panel_running || self.panel_mode != PanelMode::ProcessorRun {
            return;
        }

        for _ in 0..self.steps_per_frame {
            match self.machine.step() {
                Ok(StepOutcome::Executed { .. }) => {}
                Ok(outcome) => {
                    self.panel_running = false;
                    self.message = format!("Processor stopped: {outcome:?}");
                    break;
                }
                Err(error) => {
                    self.panel_running = false;
                    self.processor_check = true;
                    self.message = format!("Processor check: {error}");
                    break;
                }
            }
        }

        if self.panel_running {
            context.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn ce_start(&mut self) {
        if !self.panel_power {
            self.message = "CE Start ignored: system power is off".to_owned();
            return;
        }

        match self.panel_mode {
            PanelMode::ProcessorRun => {
                self.panel_running = true;
                self.message = "Processor run started".to_owned();
            }
            PanelMode::SystemInstructionStep => match self.machine.step() {
                Ok(outcome) => {
                    self.display_word = self.machine.msp().registers.psr;
                    self.message = format!("System instruction step: {outcome:?}");
                }
                Err(error) => {
                    self.processor_check = true;
                    self.message = format!("Processor check: {error}");
                }
            },
            PanelMode::InstructionStepDisplayLsr => match self.machine.step_control_processor() {
                Ok(outcome) => {
                    self.display_word = self.machine.cp().lsr[self.selected_lsr];
                    self.message = format!(
                        "CP step at {:04X}; displayed LSR {:02X}",
                        outcome.address, self.selected_lsr
                    );
                }
                Err(error) => {
                    self.processor_check = true;
                    self.message = format!("Control processor check: {error}");
                }
            },
            PanelMode::AlterStorage => {
                let address = self.address_switches as usize;
                if self.control_storage_selected {
                    if let Some(word) = self.machine.cp_mut().control_storage.get_mut(address) {
                        *word = self.data_switches;
                        self.display_word = *word;
                        self.message = format!("Control storage[{address:04X}] altered");
                    } else {
                        self.message = format!("Control-storage address {address:04X} is invalid");
                    }
                } else {
                    match self
                        .machine
                        .memory_mut()
                        .write(address, self.data_switches as u8)
                    {
                        Ok(()) => {
                            self.display_word = self.data_switches & 0x00ff;
                            self.message = format!("Main storage[{address:04X}] altered");
                        }
                        Err(error) => self.message = format!("Storage check: {error}"),
                    }
                }
            }
            PanelMode::AlterMarInterrupt => {
                let address = self.address_switches & 0x3fff;
                self.machine.cp_mut().lsr[MAIN_LEVEL_MAR_LSR] = address;
                self.display_word = address;
                self.message = format!("CP MAR set to {address:04X}");
            }
            PanelMode::DisplayStorage => {
                let address = self.address_switches as usize;
                if self.control_storage_selected {
                    if let Some(word) = self.machine.cp().control_storage.get(address) {
                        self.display_word = *word;
                        self.message = format!("Control storage[{address:04X}] = {word:04X}");
                    } else {
                        self.message = format!("Control-storage address {address:04X} is invalid");
                    }
                } else {
                    let memory = self.machine.memory();
                    match (memory.read(address), memory.read(address.saturating_add(1))) {
                        (Ok(high), Ok(low)) => {
                            self.display_word = u16::from_be_bytes([high, low]);
                            self.message =
                                format!("Main storage[{address:04X}] = {:04X}", self.display_word);
                        }
                        (Err(error), _) | (_, Err(error)) => {
                            self.message = format!("Storage check: {error}");
                        }
                    }
                }
            }
            PanelMode::InstructionStepDisplayPcr => match self.machine.step_control_processor() {
                Ok(outcome) => {
                    self.display_word = self.machine.cp().pcr;
                    self.message = format!("CP step at {:04X}; PCR displayed", outcome.address);
                }
                Err(error) => {
                    self.processor_check = true;
                    self.message = format!("Control processor check: {error}");
                }
            },
            PanelMode::DisplayChecks => {
                self.display_word = self.machine.cp().pcr;
                self.message = format!(
                    "CHECK display: CP PCR={:04X}, MSP PSR={:04X}",
                    self.machine.cp().pcr,
                    self.machine.msp().registers.psr
                );
            }
        }
    }

    fn draw_physical_panel(&mut self, ui: &mut egui::Ui, action: &mut Option<PanelAction>) {
        const FACE_WIDTH: f32 = 1600.0;
        const FACE_HEIGHT: f32 = FACE_DESIGN_HEIGHT;

        let available = ui.available_size_before_wrap();
        let width_from_height = (available.y - 4.0).max(0.0) * FACE_WIDTH / FACE_HEIGHT;
        let width = ui.available_width().min(FACE_WIDTH).min(width_from_height);
        let scale = width / FACE_WIDTH;
        let size = Vec2::new(width, FACE_HEIGHT * scale);
        let (face, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(face);

        draw_5340_faceplate(&painter, face);

        // CE panel plate; the operator panel is drawn beside the register display.
        paint_ce_text(
            &painter,
            face,
            52.0,
            179.0,
            "CE PANEL",
            14.0,
            Color32::LIGHT_GRAY,
        );
        paint_ce_text(
            &painter,
            face,
            ADDRESS_DIAL_GROUP_CENTER,
            545.0,
            "ADDRESS / DATA",
            14.0,
            Color32::LIGHT_GRAY,
        );
        paint_ce_text(
            &painter,
            face,
            DISPLAY_DIAL_GROUP_CENTER,
            520.0,
            "DISPLAY / DATA",
            14.0,
            Color32::LIGHT_GRAY,
        );

        for (index, x) in ADDRESS_DIAL_CENTERS.into_iter().enumerate() {
            let dial_rect = ce_panel_rect(face, x - 67.0, 280.0, 134.0, 134.0);
            let nibble = ((self.address_switches >> (12 - 4 * index)) & 0x0f) as u8;
            draw_hex_dial(
                &painter,
                dial_rect,
                &format!("{}", index + 1),
                nibble,
                scale,
            );
            let response = ui.interact(
                dial_rect,
                egui::Id::unique(("s34-address-dial", index)),
                Sense::click(),
            );
            if response.clicked() {
                let shift = 12 - index * 4;
                let next = (nibble + 1) & 0x0f;
                self.address_switches =
                    (self.address_switches & !(0x0f << shift)) | ((next as u16) << shift);
                self.message = format!("Address/Data dial {} → {next:X}", index + 1);
            }
            panel_tooltip(&response, "Selector hex — clic para girar una posición");
        }

        let mode_dial_rect = ce_panel_rect(face, 1245.0, 280.0, 134.0, 134.0);
        let mode_index = PanelMode::ALL
            .iter()
            .position(|mode| *mode == self.panel_mode)
            .unwrap_or_default();
        draw_mode_dial(&painter, mode_dial_rect, mode_index, scale);
        let mode_response = ui.interact(
            mode_dial_rect,
            egui::Id::unique("s34-mode-selector"),
            Sense::click(),
        );
        if mode_response.clicked() {
            self.select_next_mode();
        }
        panel_tooltip(
            &mode_response,
            "MODE SELECTOR — clic para avanzar de posición",
        );
        paint_ce_text(
            &painter,
            face,
            1312.0,
            470.0,
            "MODE SELECTOR",
            13.0,
            Color32::LIGHT_GRAY,
        );

        for (index, mode) in PanelMode::ALL.into_iter().enumerate() {
            let row = ce_panel_rect(face, 1370.0, 270.0 + index as f32 * 26.0, 205.0, 24.0);
            let active = self.panel_mode == mode;
            if active {
                painter.rect_filled(row, 3.0 * scale, Color32::from_rgb(89, 82, 57));
                painter.rect_stroke(
                    row,
                    3.0 * scale,
                    egui::Stroke::new(1.2 * scale, Color32::from_rgb(212, 186, 119)),
                    egui::StrokeKind::Inside,
                );
            }
            painter.text(
                row.left_center() + Vec2::new(5.0 * scale, 0.0),
                egui::Align2::LEFT_CENTER,
                mode.label(),
                egui::FontId::monospace(14.0 * scale),
                if active {
                    Color32::WHITE
                } else {
                    Color32::LIGHT_GRAY
                },
            );
            let response = ui.interact(
                row,
                egui::Id::unique(("s34-mode-position", index)),
                Sense::click(),
            );
            if response.clicked() {
                self.select_mode(mode);
            }
            panel_tooltip(&response, mode.label());
        }

        // Red CE pushbuttons.
        self.draw_ce_button(
            ui,
            &painter,
            face,
            CeButtonSpec {
                label: "DPLY PWR CHK",
                center_x: 312.0,
                center_y: 660.0,
                action: PanelAction::DisplayChecks,
            },
            action,
        );
        self.draw_ce_button(
            ui,
            &painter,
            face,
            CeButtonSpec {
                label: "RESET",
                center_x: 835.0,
                center_y: 660.0,
                action: PanelAction::Reset,
            },
            action,
        );
        self.draw_ce_button(
            ui,
            &painter,
            face,
            CeButtonSpec {
                label: "CE START",
                center_x: 1316.0,
                center_y: 660.0,
                action: PanelAction::CeStart,
            },
            action,
        );
        self.draw_ce_button(
            ui,
            &painter,
            face,
            CeButtonSpec {
                label: "LAMP TEST",
                center_x: 1300.0,
                center_y: 850.0,
                action: PanelAction::LampTest,
            },
            action,
        );

        // Lever switches along the lower CE panel.
        let switches = [
            (
                "PWR FAULT DPLY",
                "PREV",
                "SEARCH",
                self.previous_power_fault,
                PanelSwitch::PreviousPowerFault,
            ),
            (
                "CHECK",
                "STOP",
                "RUN",
                self.check_run,
                PanelSwitch::CheckRun,
            ),
            (
                "FORCE CLOCK",
                "OFF",
                "ON",
                self.force_clock_on,
                PanelSwitch::ForceClock,
            ),
            (
                "MSIPL",
                "DISK",
                "DISKETTE",
                self.msipl_diskette,
                PanelSwitch::MsiplDiskette,
            ),
            (
                "CSIPL",
                "DISK",
                "DISKETTE",
                self.csipl_diskette,
                PanelSwitch::CsiplDiskette,
            ),
            (
                "STOR SEL",
                "MAIN",
                "CTL",
                self.control_storage_selected,
                PanelSwitch::StorageSelect,
            ),
            (
                "ADD COMP",
                "RUN",
                "STOP",
                self.address_compare_stop,
                PanelSwitch::AddressCompare,
            ),
        ];
        for (index, (label, upper, lower, down, target)) in switches.into_iter().enumerate() {
            let switch_rect =
                ce_panel_rect(face, 120.0 + index as f32 * 150.0, 740.0, 130.0, 130.0);
            draw_lever_switch(&painter, switch_rect, label, upper, lower, down, scale);
            let response = ui.interact(
                switch_rect,
                egui::Id::unique(("s34-ce-switch", index)),
                Sense::click(),
            );
            if response.clicked() {
                self.toggle_ce_switch(target);
            }
            panel_tooltip(&response, format!("{label}: clic para cambiar de posición"));
        }

        // Clock and interrupt status lamps, then the two display bytes.
        draw_labeled_lamp(
            &painter,
            ce_panel_rect(face, 112.0, 890.0, 112.0, 58.0),
            "CLOCK",
            self.panel_running || self.force_clock_on || self.lamp_test,
            Color32::RED,
            scale,
        );
        for (index, level) in [4, 2, 1].into_iter().enumerate() {
            let lit = self.machine.cp().pending_interrupt() == Some(level) || self.lamp_test;
            draw_labeled_lamp(
                &painter,
                ce_panel_rect(face, 477.0 + index as f32 * 66.0, 890.0, 66.0, 58.0),
                &format!("IRPT {level}"),
                lit,
                Color32::RED,
                scale,
            );
        }
        draw_labeled_lamp(
            &painter,
            ce_panel_rect(face, DISPLAY_BYTE_CENTERS[0] - 50.0, 890.0, 100.0, 58.0),
            "P0",
            self.panel_running || self.lamp_test,
            Color32::RED,
            scale,
        );
        draw_labeled_lamp(
            &painter,
            ce_panel_rect(face, DISPLAY_BYTE_CENTERS[1] - 50.0, 890.0, 100.0, 58.0),
            "P1",
            !self.panel_running && self.lamp_test,
            Color32::RED,
            scale,
        );
        paint_ce_text(
            &painter,
            face,
            DISPLAY_BYTE_CENTERS[0],
            1000.0,
            "BYTE 0",
            11.0,
            Color32::LIGHT_GRAY,
        );
        paint_ce_text(
            &painter,
            face,
            DISPLAY_BYTE_CENTERS[1],
            1000.0,
            "BYTE 1",
            11.0,
            Color32::LIGHT_GRAY,
        );
        for index in 0..16 {
            let bit = 1 << (15 - index);
            let lit = self.display_word & bit != 0 || self.lamp_test;
            let x = DISPLAY_FIRST_LAMP_X + index as f32 * DISPLAY_LAMP_SPACING;
            draw_led(
                &painter,
                ce_panel_pos(face, x, 1030.0),
                11.0 * scale,
                lit,
                Color32::RED,
            );
            paint_ce_text(
                &painter,
                face,
                x,
                1055.0,
                &format!("{}", 15 - index),
                10.0,
                Color32::LIGHT_GRAY,
            );
        }

        // The separate CE Start/Stop subpanel below the CE panel.
        let stop_light = panel_design_rect(face, 260.0, 870.0, 100.0, 70.0);
        draw_labeled_lamp(
            &painter,
            stop_light,
            "STOP",
            !self.panel_running || self.lamp_test,
            Color32::RED,
            scale,
        );
        let stop_rect = panel_design_rect(face, 402.0, 880.0, 116.0, 70.0);
        let stop_response = ui.interact(stop_rect, egui::Id::unique("s34-stop"), Sense::click());
        draw_red_panel_button(
            &painter,
            stop_rect,
            "STOP",
            stop_response.is_pointer_button_down_on(),
            false,
        );
        if stop_response.clicked() {
            *action = Some(PanelAction::Stop);
        }
        panel_tooltip(&stop_response, "STOP — detener MSP");

        let running_light = panel_design_rect(face, 245.0, 960.0, 130.0, 70.0);
        draw_labeled_lamp(
            &painter,
            running_light,
            "MSP RUNNING",
            self.panel_running || self.lamp_test,
            Color32::GREEN,
            scale,
        );
        let start_rect = panel_design_rect(face, 402.0, 970.0, 116.0, 70.0);
        let start_response = ui.interact(start_rect, egui::Id::unique("s34-start"), Sense::click());
        draw_red_panel_button(
            &painter,
            start_rect,
            "START",
            start_response.is_pointer_button_down_on(),
            false,
        );
        if start_response.clicked() {
            *action = Some(PanelAction::Start);
        }
        panel_tooltip(&start_response, "START — iniciar MSP");
    }

    fn draw_ce_button(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        face: Rect,
        spec: CeButtonSpec,
        action: &mut Option<PanelAction>,
    ) {
        let rect = ce_panel_rect(
            face,
            spec.center_x - 58.0,
            spec.center_y - 47.0,
            116.0,
            94.0,
        );
        let response = ui.interact(
            rect,
            egui::Id::unique(("s34-ce-button", spec.label)),
            Sense::click(),
        );
        draw_red_panel_button(
            painter,
            rect,
            spec.label,
            response.is_pointer_button_down_on(),
            matches!(spec.action, PanelAction::LampTest) && self.lamp_test,
        );
        if response.clicked() {
            *action = Some(spec.action);
        }
        panel_tooltip(&response, spec.label);
    }

    fn select_mode(&mut self, mode: PanelMode) {
        let changed = self.panel_mode != mode;
        self.panel_mode = mode;
        if changed && mode != PanelMode::ProcessorRun && self.panel_running {
            self.panel_running = false;
        }
        self.message = format!("Mode selector: {}", mode.label());
    }

    fn select_next_mode(&mut self) {
        let index = PanelMode::ALL
            .iter()
            .position(|mode| *mode == self.panel_mode)
            .unwrap_or_default();
        self.select_mode(PanelMode::ALL[(index + 1) % PanelMode::ALL.len()]);
    }

    fn toggle_ce_switch(&mut self, switch: PanelSwitch) {
        let label = match switch {
            PanelSwitch::PreviousPowerFault => {
                self.previous_power_fault = !self.previous_power_fault;
                "Power fault display"
            }
            PanelSwitch::CheckRun => {
                self.check_run = !self.check_run;
                "Check response"
            }
            PanelSwitch::ForceClock => {
                self.force_clock_on = !self.force_clock_on;
                "Force clock"
            }
            PanelSwitch::MsiplDiskette => {
                self.msipl_diskette = !self.msipl_diskette;
                "MSIPL source"
            }
            PanelSwitch::CsiplDiskette => {
                self.csipl_diskette = !self.csipl_diskette;
                "CSIPL source"
            }
            PanelSwitch::StorageSelect => {
                self.control_storage_selected = !self.control_storage_selected;
                "Storage select"
            }
            PanelSwitch::AddressCompare => {
                self.address_compare_stop = !self.address_compare_stop;
                "Address compare"
            }
        };
        self.message = format!("{label}: switch toggled");
    }

    fn draw_panel_side_controls(&mut self, ui: &mut egui::Ui, action: &mut Option<PanelAction>) {
        ui.heading("DISPLAY / DATA");
        ui.horizontal(|ui| {
            hex_display(ui, self.display_word);
            ui.vertical(|ui| {
                ui.monospace(format!("MAR {:04X}", self.machine.cp().microaddress()));
                ui.monospace(format!("PCR {:04X}", self.machine.cp().pcr));
            });
        });
        ui.label(egui::RichText::new(&self.message).small().weak());
        ui.separator();

        ui.label("ADDRESS / DATA");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.address_switches)
                    .hexadecimal(4, false, true)
                    .prefix("ADR "),
            );
            ui.add(
                egui::DragValue::new(&mut self.data_switches)
                    .hexadecimal(4, false, true)
                    .prefix("DATA "),
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.selected_lsr)
                    .range(0..=63)
                    .prefix("LSR "),
            );
            ui.add(
                egui::DragValue::new(&mut self.steps_per_frame)
                    .range(1..=512)
                    .prefix("RUN/FRAME "),
            );
        });

        let mut zoom = ui.ctx().zoom_factor();
        if ui
            .add(egui::Slider::new(&mut zoom, 0.8..=2.5).text("UI scale"))
            .changed()
        {
            ui.ctx().set_zoom_factor(zoom);
        }

        ui.separator();
        ui.label("IPL DISKETTE");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.diskette_path)
                    .desired_width((ui.available_width() - 70.0).max(100.0))
                    .hint_text("path to .IMD image"),
            );
            if ui
                .add_enabled(
                    self.panel_power && !self.diskette_path.is_empty(),
                    egui::Button::new("MOUNT"),
                )
                .clicked()
            {
                *action = Some(PanelAction::MountDiskette);
            }
        });

        egui::CollapsingHeader::new("MSP / CP REGISTERS")
            .default_open(true)
            .show(ui, |ui| {
                let msp = &self.machine.msp().registers;
                let cp = self.machine.cp();
                egui::Grid::new("system34-side-registers")
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        register_cell(ui, "MSP IAR", format_args!("{:05X}", msp.iar));
                        register_cell(ui, "MSP ARR", format_args!("{:05X}", msp.arr));
                        register_cell(
                            ui,
                            "XR1 / XR2",
                            format_args!(
                                "{:04X} / {:04X}",
                                msp.index_registers[0], msp.index_registers[1]
                            ),
                        );
                        register_cell(ui, "MSP PSR", format_args!("{:04X}", msp.psr));
                        register_cell(ui, "MSP ACR", format_args!("{:06X}", msp.acr));
                        register_cell(
                            ui,
                            "MSP Q / Y",
                            format_args!("{:02X} / {:02X}", msp.q, msp.y),
                        );
                        register_cell(ui, "CP MAR", format_args!("{:04X}", cp.microaddress()));
                        register_cell(ui, "CP MOR", format_args!("{:04X}", cp.mor));
                        register_cell(ui, "CP PCR", format_args!("{:04X}", cp.pcr));
                        register_cell(
                            ui,
                            "CP SAR / SDR",
                            format_args!("{:04X} / {:04X}", cp.sar, cp.sdr),
                        );
                        register_cell(ui, "CP X / Y", format_args!("{:04X} / {:04X}", cp.x, cp.y));
                        register_cell(
                            ui,
                            &format!("LSR {:02X}", self.selected_lsr),
                            format_args!("{:04X}", cp.lsr[self.selected_lsr]),
                        );
                    });
            });

        self.draw_operator_panel_side(ui, action);
    }

    fn draw_operator_panel_side(&mut self, ui: &mut egui::Ui, action: &mut Option<PanelAction>) {
        ui.separator();
        egui::Frame::new()
            .fill(Color32::from_rgb(42, 44, 43))
            .stroke(egui::Stroke::new(1.0, PANEL_EDGE))
            .inner_margin(8.0)
            .corner_radius(egui::CornerRadius::same(4))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("OPERATOR PANEL").strong());
                ui.horizontal(|ui| {
                    let (power_rect, power_response) =
                        ui.allocate_exact_size(Vec2::new(76.0, 76.0), Sense::click());
                    draw_power_rocker(
                        &ui.painter_at(power_rect),
                        power_rect,
                        self.panel_power,
                        power_response.is_pointer_button_down_on(),
                    );
                    if power_response.clicked() {
                        *action = Some(PanelAction::Power);
                    }
                    panel_tooltip(&power_response, "POWER — I/O");

                    let (load_rect, load_response) =
                        ui.allocate_exact_size(Vec2::new(72.0, 76.0), Sense::click());
                    draw_square_load_button(
                        &ui.painter_at(load_rect),
                        load_rect,
                        load_response.is_pointer_button_down_on(),
                    );
                    if load_response.clicked() && self.panel_power {
                        *action = Some(PanelAction::Load);
                    }
                    panel_tooltip(&load_response, "LOAD — iniciar CSIPL");

                    ui.add_space(4.0);
                    ui.columns(2, |columns| {
                        compact_status_lamp(
                            &mut columns[0],
                            "POWER",
                            self.panel_power,
                            Color32::from_rgb(83, 255, 118),
                        );
                        compact_status_lamp(
                            &mut columns[1],
                            "SYSTEM IN USE",
                            self.panel_running,
                            Color32::from_rgb(83, 255, 118),
                        );
                        compact_status_lamp(&mut columns[0], "POWER CHECK", false, Color32::RED);
                        compact_status_lamp(&mut columns[1], "THERMAL CHECK", false, Color32::RED);
                        compact_status_lamp(
                            &mut columns[0],
                            "PROCESSOR CHECK",
                            self.processor_check,
                            Color32::RED,
                        );
                        compact_status_lamp(
                            &mut columns[1],
                            "CONSOLE CHECK",
                            self.machine.virtual_twinax_bus().indicators().check,
                            Color32::RED,
                        );
                    });
                });
            });
    }

    fn draw_control_panel(&mut self, ui: &mut egui::Ui) {
        let mut action = None;
        egui::Frame::new()
            .fill(PANEL_METAL)
            .stroke(egui::Stroke::new(1.5, PANEL_EDGE))
            .inner_margin(12.0)
            .corner_radius(egui::CornerRadius::same(8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("IBM 5340  ·  CONTROL PANEL");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label("SYSTEM / 34");
                    });
                });
                ui.separator();
                let available = ui.available_size_before_wrap();
                let column_gap = ui.spacing().item_spacing.x;
                let left_width = ((available.x - column_gap).max(0.0) * 0.66).max(0.0);
                let right_width = (available.x - left_width - column_gap).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(left_width, available.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.draw_physical_panel(ui, &mut action),
                    );
                    ui.add_space(column_gap);
                    ui.allocate_ui_with_layout(
                        Vec2::new(right_width, available.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.draw_panel_side_controls(ui, &mut action),
                    );
                });
            });

        match action {
            Some(PanelAction::DisplayChecks) => {
                self.select_mode(PanelMode::DisplayChecks);
                self.ce_start();
            }
            Some(PanelAction::Start) => {
                if self.panel_power && self.panel_mode == PanelMode::ProcessorRun {
                    self.panel_running = true;
                    self.message = "START pressed".to_owned();
                } else if !self.panel_power {
                    self.message = "START ignored: system power is off".to_owned();
                } else {
                    self.message = "Select PROC RUN to start continuous execution".to_owned();
                }
            }
            Some(PanelAction::Stop) => {
                self.panel_running = false;
                self.message = "STOP pressed; MSP execution halted".to_owned();
            }
            Some(PanelAction::CeStart) => self.ce_start(),
            Some(PanelAction::Reset) => {
                self.machine.reset(0);
                self.panel_running = false;
                self.processor_check = false;
                self.display_word = 0;
                self.message = "System reset; MSP entry address 00000".to_owned();
            }
            Some(PanelAction::Load) => {
                if !self.csipl_diskette {
                    self.message =
                        "CSIPL source is DISK; fixed-disk CSIPL is not implemented".to_owned();
                } else {
                    match self.machine.load_csipl_bootstrap_from_diskette() {
                        Ok(state) => {
                            self.panel_running = false;
                            self.message = format!("LOAD completed: {state:?}");
                        }
                        Err(error) => self.message = format!("LOAD failed: {error}"),
                    }
                }
            }
            Some(PanelAction::MountDiskette) => {
                match self.machine.mount_diskette_imd_file(&self.diskette_path) {
                    Ok(()) => self.message = format!("Mounted diskette {}", self.diskette_path),
                    Err(error) => self.message = format!("Diskette mount failed: {error}"),
                }
            }
            Some(PanelAction::LampTest) => {
                self.lamp_test = !self.lamp_test;
                self.message = if self.lamp_test {
                    "CE lamp test active".to_owned()
                } else {
                    "CE lamp test complete".to_owned()
                };
            }
            Some(PanelAction::Power) => {
                if self.panel_power {
                    self.panel_power = false;
                    self.panel_running = false;
                    self.message = "Power off".to_owned();
                } else {
                    self.panel_power = true;
                    self.machine.reset(0);
                    self.processor_check = false;
                    self.message = "Power on; system reset complete".to_owned();
                }
            }
            None => {}
        }
    }
}

fn panel_design_rect(face: Rect, x: f32, y: f32, width: f32, height: f32) -> Rect {
    let sx = face.width() / 1600.0;
    let sy = face.height() / FACE_DESIGN_HEIGHT;
    Rect::from_min_size(
        Pos2::new(face.left() + x * sx, face.top() + y * sy),
        Vec2::new(width * sx, height * sy),
    )
}

fn panel_design_pos(face: Rect, x: f32, y: f32) -> Pos2 {
    let point = panel_design_rect(face, x, y, 0.0, 0.0);
    point.min
}

fn ce_y(y: f32) -> f32 {
    18.0 + (y - 160.0) * CE_VERTICAL_SCALE
}

fn ce_panel_rect(face: Rect, x: f32, y: f32, width: f32, height: f32) -> Rect {
    panel_design_rect(face, x, ce_y(y), width, height * CE_VERTICAL_SCALE)
}

fn ce_panel_pos(face: Rect, x: f32, y: f32) -> Pos2 {
    panel_design_pos(face, x, ce_y(y))
}

fn paint_ce_text(
    painter: &egui::Painter,
    face: Rect,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    color: Color32,
) {
    paint_panel_text(painter, face, x, ce_y(y), text, size, color);
}

fn draw_5340_faceplate(painter: &egui::Painter, face: Rect) {
    painter.rect_filled(face, 9.0, Color32::from_rgb(27, 28, 28));
    painter.rect_stroke(
        face,
        9.0,
        egui::Stroke::new(2.0, Color32::from_rgb(151, 153, 148)),
        egui::StrokeKind::Inside,
    );

    for (y, height, fill) in [
        (18.0, ce_y(1080.0) - 18.0, PANEL_METAL),
        (
            ce_y(1095.0),
            FACE_DESIGN_HEIGHT - ce_y(1095.0),
            Color32::from_rgb(40, 42, 42),
        ),
    ] {
        let rect = panel_design_rect(face, 18.0, y, 1564.0, height);
        painter.rect_filled(rect, 4.0, fill);
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.2, PANEL_EDGE),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                rect.left_top() + Vec2::new(2.0, 2.0),
                rect.right_top() - Vec2::new(2.0, 2.0),
            ],
            egui::Stroke::new(1.0, Color32::from_rgb(174, 174, 166)),
        );
    }

    for (x, y) in [
        (30.0, 30.0),
        (1570.0, 30.0),
        (30.0, FACE_DESIGN_HEIGHT - 20.0),
        (1570.0, FACE_DESIGN_HEIGHT - 20.0),
        (30.0, ce_y(700.0)),
        (1570.0, ce_y(700.0)),
    ] {
        let center = panel_design_pos(face, x, y);
        let radius = face.width() / 1600.0 * 8.0;
        painter.circle_filled(center, radius, Color32::from_rgb(130, 134, 132));
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0, Color32::from_rgb(25, 26, 26)),
        );
        painter.line_segment(
            [
                center - Vec2::new(radius * 0.45, 0.0),
                center + Vec2::new(radius * 0.45, 0.0),
            ],
            egui::Stroke::new(1.0, Color32::from_rgb(43, 45, 44)),
        );
    }
}

fn paint_panel_text(
    painter: &egui::Painter,
    face: Rect,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    color: Color32,
) {
    let position = panel_design_pos(face, x, y);
    painter.text(
        position,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(size * face.width() / 1600.0),
        color,
    );
}

fn panel_tooltip(response: &egui::Response, text: impl ToString) {
    if response.hovered() {
        response.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let _ = response.clone().on_hover_text(text.to_string());
}

fn draw_led(painter: &egui::Painter, center: Pos2, radius: f32, lit: bool, color: Color32) {
    let dark = Color32::from_rgb(37, 20, 18);
    painter.circle_filled(center, radius + 3.0, Color32::from_rgb(27, 28, 27));
    painter.circle_filled(center, radius, if lit { color } else { dark });
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0, Color32::from_rgb(146, 144, 132)),
    );
    if lit {
        painter.circle_filled(
            center,
            radius * 0.42,
            Color32::from_rgba_unmultiplied(255, 245, 215, 170),
        );
    }
}

fn draw_labeled_lamp(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    lit: bool,
    color: Color32,
    scale: f32,
) {
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 9.0 * scale),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(10.0 * scale),
        Color32::from_rgb(215, 213, 199),
    );
    draw_led(
        painter,
        Pos2::new(rect.center().x, rect.center().y + 10.0 * scale),
        8.5 * scale,
        lit,
        color,
    );
}

fn compact_status_lamp(ui: &mut egui::Ui, label: &str, lit: bool, color: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
        draw_led(ui.painter(), rect.center(), 4.5, lit, color);
        ui.label(egui::RichText::new(label).small());
    });
}

fn draw_red_panel_button(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    pressed: bool,
    illuminated: bool,
) {
    let scale = rect.width() / 116.0;
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 9.0 * scale),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(10.0 * scale),
        Color32::from_rgb(224, 221, 203),
    );
    let center = Pos2::new(rect.center().x, rect.center().y + 8.0 * scale);
    let radius = 30.0 * scale;
    painter.circle_filled(
        center + Vec2::new(0.0, if pressed { 2.0 * scale } else { 6.0 * scale }),
        radius + 8.0 * scale,
        Color32::from_rgb(21, 22, 21),
    );
    painter.circle_filled(
        center,
        radius + 5.0 * scale,
        Color32::from_rgb(151, 151, 140),
    );
    if illuminated {
        painter.circle_filled(
            center,
            radius + 11.0 * scale,
            Color32::from_rgba_unmultiplied(255, 45, 20, 62),
        );
    }
    painter.circle_filled(
        center + Vec2::new(0.0, if pressed { 1.0 * scale } else { 0.0 }),
        radius,
        if pressed {
            Color32::from_rgb(151, 31, 22)
        } else {
            Color32::from_rgb(205, 47, 34)
        },
    );
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.5 * scale, Color32::from_rgb(244, 119, 93)),
    );
    painter.line_segment(
        [
            center + Vec2::new(-radius * 0.48, -radius * 0.58),
            center + Vec2::new(radius * 0.48, -radius * 0.58),
        ],
        egui::Stroke::new(
            2.0 * scale,
            Color32::from_rgba_unmultiplied(255, 190, 160, 180),
        ),
    );
}

fn draw_square_load_button(painter: &egui::Painter, rect: Rect, pressed: bool) {
    let scale = rect.width() / 86.0;
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 9.0 * scale),
        egui::Align2::CENTER_CENTER,
        "LOAD",
        egui::FontId::monospace(10.0 * scale),
        Color32::from_rgb(225, 222, 205),
    );
    let center = Pos2::new(rect.center().x, rect.center().y + 10.0 * scale);
    let button_rect = Rect::from_center_size(
        center + Vec2::new(0.0, if pressed { 2.0 * scale } else { 0.0 }),
        Vec2::splat(51.0 * scale),
    );
    painter.rect_filled(
        button_rect.expand(6.0 * scale),
        5.0 * scale,
        Color32::from_rgb(27, 28, 26),
    );
    painter.rect_filled(
        button_rect,
        3.0 * scale,
        if pressed {
            Color32::from_rgb(166, 139, 48)
        } else {
            Color32::from_rgb(218, 190, 72)
        },
    );
    painter.rect_stroke(
        button_rect,
        3.0 * scale,
        egui::Stroke::new(2.0 * scale, Color32::from_rgb(247, 222, 123)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            button_rect.left_top() + Vec2::new(4.0 * scale, 4.0 * scale),
            button_rect.right_top() - Vec2::new(4.0 * scale, 4.0 * scale),
        ],
        egui::Stroke::new(
            2.0 * scale,
            Color32::from_rgba_unmultiplied(255, 243, 176, 190),
        ),
    );
}

fn draw_power_rocker(painter: &egui::Painter, rect: Rect, on: bool, pressed: bool) {
    let scale = rect.width() / 125.0;
    painter.rect_filled(rect, 4.0 * scale, Color32::from_rgb(22, 23, 23));
    painter.rect_stroke(
        rect,
        4.0 * scale,
        egui::Stroke::new(2.0 * scale, Color32::from_rgb(131, 133, 126)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 10.0 * scale),
        egui::Align2::CENTER_CENTER,
        "POWER",
        egui::FontId::monospace(10.0 * scale),
        Color32::LIGHT_GRAY,
    );
    let center = Pos2::new(rect.center().x, rect.center().y + 11.0 * scale);
    painter.rect_filled(
        Rect::from_center_size(
            center + Vec2::new(0.0, if pressed { 2.0 * scale } else { 0.0 }),
            Vec2::new(33.0 * scale, 38.0 * scale),
        ),
        4.0 * scale,
        Color32::from_rgb(172, 37, 29),
    );
    painter.text(
        center + Vec2::new(0.0, if on { -4.0 } else { 4.0 } * scale),
        egui::Align2::CENTER_CENTER,
        if on { "I" } else { "O" },
        egui::FontId::monospace(18.0 * scale),
        Color32::WHITE,
    );
}

fn draw_hex_dial(painter: &egui::Painter, rect: Rect, number: &str, value: u8, scale: f32) {
    let center = rect.center() + Vec2::new(0.0, -6.0 * scale);
    let radius = rect.width() * 0.34;
    painter.circle_filled(center, radius + 9.0 * scale, Color32::from_rgb(29, 30, 29));
    painter.circle_filled(
        center,
        radius + 4.0 * scale,
        Color32::from_rgb(116, 117, 109),
    );
    painter.circle_filled(center, radius, Color32::from_rgb(35, 36, 35));
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.4 * scale, Color32::from_rgb(177, 177, 164)),
    );
    for digit in 0..16 {
        let angle = -std::f32::consts::FRAC_PI_2 + digit as f32 * std::f32::consts::TAU / 16.0;
        let direction = Vec2::new(angle.cos(), angle.sin());
        let tick_start = center + direction * (radius + 9.0 * scale);
        let tick_end = center + direction * (radius + 14.0 * scale);
        painter.line_segment(
            [tick_start, tick_end],
            egui::Stroke::new(1.0 * scale, Color32::LIGHT_GRAY),
        );
        let label_pos = center + direction * (radius + 27.0 * scale);
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            format!("{digit:X}"),
            egui::FontId::monospace(9.0 * scale),
            Color32::from_rgb(219, 217, 201),
        );
    }
    let angle = -std::f32::consts::FRAC_PI_2 + value as f32 * std::f32::consts::TAU / 16.0;
    let pointer = Vec2::new(angle.cos(), angle.sin());
    painter.line_segment(
        [
            center - pointer * radius * 0.38,
            center + pointer * radius * 0.72,
        ],
        egui::Stroke::new(5.0 * scale, Color32::from_rgb(187, 190, 180)),
    );
    painter.circle_filled(center, radius * 0.30, Color32::from_rgb(63, 65, 63));
    painter.circle_stroke(
        center,
        radius * 0.30,
        egui::Stroke::new(1.0 * scale, Color32::from_rgb(180, 180, 170)),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() + 64.0 * scale),
        egui::Align2::CENTER_CENTER,
        number,
        egui::FontId::monospace(12.0 * scale),
        Color32::WHITE,
    );
}

fn draw_mode_dial(painter: &egui::Painter, rect: Rect, mode: usize, scale: f32) {
    let center = rect.center();
    let radius = rect.width() * 0.37;
    painter.circle_filled(center, radius + 8.0 * scale, Color32::from_rgb(27, 28, 27));
    painter.circle_filled(
        center,
        radius + 3.0 * scale,
        Color32::from_rgb(121, 123, 116),
    );
    painter.circle_filled(center, radius, Color32::from_rgb(34, 35, 34));
    for index in 0..8 {
        let angle = -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::TAU / 8.0;
        let direction = Vec2::new(angle.cos(), angle.sin());
        painter.line_segment(
            [
                center + direction * (radius + 5.0 * scale),
                center + direction * (radius + 12.0 * scale),
            ],
            egui::Stroke::new(2.0 * scale, Color32::LIGHT_GRAY),
        );
    }
    let angle = -std::f32::consts::FRAC_PI_2 + mode as f32 * std::f32::consts::TAU / 8.0;
    let direction = Vec2::new(angle.cos(), angle.sin());
    painter.line_segment(
        [center, center + direction * radius * 0.82],
        egui::Stroke::new(6.0 * scale, Color32::from_rgb(198, 199, 188)),
    );
    painter.circle_filled(center, radius * 0.24, Color32::from_rgb(61, 63, 61));
}

fn draw_lever_switch(
    painter: &egui::Painter,
    rect: Rect,
    label: &str,
    upper_label: &str,
    lower_label: &str,
    lower_position: bool,
    scale: f32,
) {
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 10.0 * scale),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(10.0 * scale),
        Color32::from_rgb(220, 218, 205),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 29.0 * scale),
        egui::Align2::CENTER_CENTER,
        upper_label,
        egui::FontId::monospace(9.0 * scale),
        if lower_position {
            Color32::GRAY
        } else {
            Color32::WHITE
        },
    );
    let slot = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.center().y + 6.0 * scale),
        Vec2::new(22.0 * scale, 36.0 * scale),
    );
    painter.rect_filled(slot, 10.0 * scale, Color32::from_rgb(21, 22, 22));
    painter.rect_stroke(
        slot,
        10.0 * scale,
        egui::Stroke::new(1.0 * scale, PANEL_EDGE),
        egui::StrokeKind::Inside,
    );
    let upper = Pos2::new(slot.center().x, slot.top() + 5.0 * scale);
    let lower = Pos2::new(slot.center().x, slot.bottom() - 5.0 * scale);
    let endpoint = if lower_position { lower } else { upper };
    painter.line_segment(
        [slot.center(), endpoint],
        egui::Stroke::new(5.0 * scale, Color32::from_rgb(184, 185, 176)),
    );
    painter.circle_filled(endpoint, 10.0 * scale, Color32::from_rgb(103, 105, 101));
    painter.circle_stroke(
        endpoint,
        10.0 * scale,
        egui::Stroke::new(1.2 * scale, Color32::from_rgb(215, 215, 202)),
    );
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 3.0 * scale),
        egui::Align2::CENTER_BOTTOM,
        lower_label,
        egui::FontId::monospace(9.0 * scale),
        if lower_position {
            Color32::WHITE
        } else {
            Color32::GRAY
        },
    );
}

impl eframe::App for TerminalGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.finish_timed_activity();
        self.service_processor_run(&context);
        if let Some(deadline) = self.activity_until {
            context.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }

        let control_panel_id = egui::ViewportId::from_hash_of("system34-control-panel");
        if self.control_panel_open {
            context.show_viewport_immediate(
                control_panel_id,
                egui::ViewportBuilder::default()
                    .with_title("IBM 5340 — Control Panel")
                    .with_inner_size([2000.0, 1250.0])
                    .with_min_inner_size([1600.0, 1000.0]),
                |ui, _class| {
                    if ui.input(|input| input.viewport().close_requested()) {
                        self.control_panel_open = false;
                    }
                    egui::CentralPanel::default().show(ui, |ui| self.draw_control_panel(ui));
                },
            );
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(35, 34, 29))
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading("IBM System/34");
                            ui.label("5251 MODEL 11  ·  VIRTUAL TWINAX");
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("5340 CONTROL PANEL").clicked() {
                                        self.control_panel_open = true;
                                    }
                                    if ui.small_button("A−").clicked() {
                                        egui::gui_zoom::zoom_out(ui.ctx());
                                    }
                                    if ui.small_button("A+").clicked() {
                                        egui::gui_zoom::zoom_in(ui.ctx());
                                    }
                                    if ui.small_button("100%").clicked() {
                                        ui.ctx().set_zoom_factor(1.0);
                                    }
                                    ui.label("SYSTEM CONSOLE  /  STATION 00");
                                },
                            );
                        });
                        ui.add_space(8.0);

                        let indicators = self.machine.virtual_twinax_bus().indicators();
                        egui::Frame::new()
                            .fill(Color32::from_rgb(23, 23, 20))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(77, 74, 62)))
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    indicator(ui, "ONLINE", indicators.online, Color32::GREEN);
                                    indicator(ui, "SELECTED", indicators.selected, Color32::YELLOW);
                                    indicator(ui, "POLL", indicators.polling, Color32::LIGHT_BLUE);
                                    indicator(
                                        ui,
                                        "HOST → STATION",
                                        indicators.transmitting,
                                        Color32::GREEN,
                                    );
                                    indicator(
                                        ui,
                                        "STATION → HOST",
                                        indicators.receiving,
                                        Color32::LIGHT_BLUE,
                                    );
                                    indicator(
                                        ui,
                                        "ATTENTION",
                                        indicators.attention,
                                        Color32::YELLOW,
                                    );
                                    indicator(ui, "CHECK", indicators.check, Color32::RED);
                                });
                            });

                        ui.add_space(12.0);
                        draw_crt(
                            ui,
                            self.machine.operator_console(),
                            &self.screen_font,
                            &self.screen_texture,
                        );
                        ui.add_space(12.0);

                        egui::Frame::new()
                            .fill(Color32::from_rgb(23, 23, 20))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(77, 74, 62)))
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .button(if self.machine.virtual_twinax_bus().is_online() {
                                            "Take station offline"
                                        } else {
                                            "Bring station online"
                                        })
                                        .clicked()
                                    {
                                        let online = !self.machine.virtual_twinax_bus().is_online();
                                        self.machine.virtual_twinax_bus_mut().set_online(online);
                                        self.message =
                                            if self.machine.virtual_twinax_bus().is_online() {
                                                "Station online".to_owned()
                                            } else {
                                                "Station offline".to_owned()
                                            };
                                    }
                                    if ui.button("Poll station").clicked() {
                                        if self.machine.virtual_twinax_bus_mut().poll_station() {
                                            self.message = format!(
                                                "Station polled ({} total)",
                                                self.machine.virtual_twinax_bus().poll_count()
                                            );
                                            self.pulse(&context);
                                        } else {
                                            self.message =
                                                "Poll rejected: station offline or in CHECK"
                                                    .to_owned();
                                        }
                                    }
                                    if ui.button("Raise ATTENTION").clicked() {
                                        self.machine.virtual_twinax_bus_mut().signal_attention();
                                        self.message = "Attention condition latched".to_owned();
                                    }
                                    if ui.button("Acknowledge").clicked() {
                                        self.machine
                                            .virtual_twinax_bus_mut()
                                            .acknowledge_attention();
                                        self.message = "Attention acknowledged".to_owned();
                                    }
                                    if ui.button("Force CHECK").clicked() {
                                        self.machine.virtual_twinax_bus_mut().signal_check();
                                        self.message =
                                            "Workstation attachment CHECK simulated".to_owned();
                                    }
                                    if ui.button("Reset CHECK").clicked() {
                                        self.machine.virtual_twinax_bus_mut().reset_check();
                                        self.message = "CHECK reset".to_owned();
                                    }
                                    if ui.button("Clear display").clicked() {
                                        if self
                                            .machine
                                            .virtual_twinax_bus_mut()
                                            .send_to_station(&[0x0c])
                                        {
                                            self.machine.operator_console_mut().clear_screen();
                                            self.refresh_screen();
                                            self.message =
                                                "Host clear-screen command sent".to_owned();
                                            self.pulse(&context);
                                        } else {
                                            self.message =
                                                "Clear rejected: station offline or in CHECK"
                                                    .to_owned();
                                        }
                                    }
                                });

                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.label("Host → screen");
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut self.host_message)
                                            .desired_width(360.0)
                                            .hint_text("Message to display on the 5251"),
                                    );
                                    if response.lost_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                    {
                                        self.send_host_message(&context);
                                    }
                                    if ui.button("Write").clicked() {
                                        self.send_host_message(&context);
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Keyboard → host");
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut self.keyboard_text)
                                            .desired_width(360.0)
                                            .hint_text("Text to send as 5251 keystrokes"),
                                    );
                                    if response.lost_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                    {
                                        self.send_keyboard_text(&context);
                                    }
                                    if ui.button("Send keys").clicked() {
                                        self.send_keyboard_text(&context);
                                    }
                                    if ui.button("ENTER").clicked() {
                                        self.send_station_key(TerminalKey::Enter, &context);
                                    }
                                    if ui.button("ATTN").clicked() {
                                        self.send_station_key(TerminalKey::Attention, &context);
                                    }
                                });

                                ui.separator();
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(&self.message);
                                    ui.separator();
                                    ui.label(format!(
                                        "Polls: {}",
                                        self.machine.virtual_twinax_bus().poll_count()
                                    ));
                                    ui.label(format!(
                                        "Host bytes: {}",
                                        self.machine.virtual_twinax_bus().host_bytes_sent()
                                    ));
                                    ui.label(format!(
                                        "Keyboard events: {}",
                                        self.machine
                                            .virtual_twinax_bus()
                                            .keyboard_events_received()
                                    ));
                                    ui.label(format!(
                                        "Queued keys: {}",
                                        self.machine.operator_console().pending_keys()
                                    ));
                                    ui.label(format!("Last key: {}", self.last_key));
                                });
                            });
                    });
            });
    }
}

fn hex_display(ui: &mut egui::Ui, value: u16) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(112.0, 36.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, PANEL_INSET);
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, PANEL_EDGE),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("{value:04X}"),
        egui::FontId::monospace(25.0),
        Color32::from_rgb(152, 255, 170),
    );
}

fn register_cell(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    ui.label(egui::RichText::new(label).small().weak());
    ui.label(
        egui::RichText::new(value.to_string())
            .monospace()
            .color(Color32::from_rgb(160, 238, 174)),
    );
}

fn indicator(ui: &mut egui::Ui, label: &str, lit: bool, color: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
        let painter = ui.painter();
        painter.circle_filled(
            rect.center(),
            5.0,
            if lit {
                color
            } else {
                Color32::from_rgb(54, 52, 46)
            },
        );
        if lit {
            painter.circle_stroke(rect.center(), 6.5, egui::Stroke::new(1.0, color));
        }
        ui.label(label);
        ui.add_space(8.0);
    });
}

fn draw_crt(
    ui: &mut egui::Ui,
    console: &OperatorConsole,
    font: &ScreenFont,
    texture: &TextureHandle,
) {
    let available = ui.available_size_before_wrap();
    let native_width = console.columns() as f32 * font.cell_width as f32;
    let native_height = console.rows() as f32 * font.line_height as f32;
    let font_scale = (available.x / (native_width + 40.0))
        .min(available.y / (native_height + 300.0))
        .clamp(0.8, 1.6);
    let image_size = Vec2::new(native_width * font_scale, native_height * font_scale);
    let bezel_size = image_size + Vec2::splat(38.0);

    ui.vertical_centered(|ui| {
        let (bezel, _) = ui.allocate_exact_size(bezel_size, Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(bezel, 18.0, Color32::from_rgb(67, 64, 54));
        painter.rect_stroke(
            bezel,
            18.0,
            egui::Stroke::new(2.0, Color32::from_rgb(112, 106, 87)),
            egui::StrokeKind::Inside,
        );
        let screen = Rect::from_min_size(bezel.min + Vec2::splat(19.0), image_size);
        painter.rect_filled(screen.expand(3.0), 8.0, Color32::from_rgb(1, 7, 3));
        painter.image(
            texture.id(),
            screen,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        for y in (0..image_size.y as usize).step_by(4) {
            let scan_y = screen.top() + y as f32;
            painter.line_segment(
                [
                    Pos2::new(screen.left(), scan_y),
                    Pos2::new(screen.right(), scan_y),
                ],
                egui::Stroke::new(1.0, Color32::from_black_alpha(12)),
            );
        }
        painter.rect_stroke(
            screen,
            2.0,
            egui::Stroke::new(1.0, Color32::from_rgb(30, 70, 45)),
            egui::StrokeKind::Inside,
        );
    });
}

fn screen_image(console: &OperatorConsole, screen_font: &ScreenFont) -> ColorImage {
    let width = console.columns() * screen_font.cell_width;
    let height = console.rows() * screen_font.line_height;
    let mut image = ColorImage::filled([width, height], SCREEN_BACKGROUND);
    let cursor = console.cursor();

    for (row_index, row) in console.screen().iter().enumerate() {
        for (column_index, character) in row.iter().copied().enumerate() {
            let character = if screen_font.font.lookup_glyph_index(character) == 0 {
                '?'
            } else {
                character
            };
            let (metrics, bitmap) = screen_font.font.rasterize(character, FONT_SIZE);
            let cell_x = (column_index * screen_font.cell_width) as i32;
            let line_y = row_index * screen_font.line_height;
            let glyph_x = cell_x + metrics.xmin;
            let glyph_y =
                line_y as i32 + screen_font.baseline - metrics.ymin - metrics.height as i32;

            for bitmap_y in 0..metrics.height {
                for bitmap_x in 0..metrics.width {
                    let x = glyph_x + bitmap_x as i32;
                    let y = glyph_y + bitmap_y as i32;
                    if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                        let coverage = bitmap[bitmap_y * metrics.width + bitmap_x];
                        if coverage != 0 {
                            image.pixels[y as usize * width + x as usize] =
                                phosphor_coverage(coverage);
                        }
                    }
                }
            }

            if (row_index, column_index) == cursor {
                let underline_y = (line_y as i32 + screen_font.baseline + 1)
                    .clamp(0, height.saturating_sub(1) as i32)
                    as usize;
                let underline_start = (cell_x + 1).max(0) as usize;
                let underline_end =
                    ((cell_x + screen_font.cell_width as i32 - 1).clamp(0, width as i32)) as usize;
                for x in underline_start..underline_end {
                    image.pixels[underline_y * width + x] = PHOSPHOR_DIM;
                }
            }
        }
    }
    image
}

fn phosphor_coverage(coverage: u8) -> Color32 {
    let foreground = PHOSPHOR.to_array();
    let background = SCREEN_BACKGROUND.to_array();
    let coverage = coverage as u16;
    Color32::from_rgb(
        blend_channel(background[0], foreground[0], coverage),
        blend_channel(background[1], foreground[1], coverage),
        blend_channel(background[2], foreground[2], coverage),
    )
}

fn blend_channel(background: u8, foreground: u8, coverage: u16) -> u8 {
    ((background as u16 * (255 - coverage) + foreground as u16 * coverage) / 255) as u8
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 950.0])
            .with_min_inner_size([960.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "IBM System/34 — 5251 Operator Console",
        options,
        Box::new(|context| Ok(Box::new(TerminalGui::new(context)))),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        screen_image, PanelMode, ScreenFont, TerminalGui, ADDRESS_DIAL_GROUP_CENTER,
        DISPLAY_BYTE_CENTERS, DISPLAY_DIAL_GROUP_CENTER, SCREEN_BACKGROUND,
    };
    use eframe::egui;
    use system34::{memory::Memory, OperatorConsole, System34, TerminalKey};

    fn test_gui(machine: System34) -> (egui::Context, TerminalGui) {
        let context = egui::Context::default();
        let screen_font = ScreenFont::new();
        let screen_texture = context.load_texture(
            "test-5251-screen",
            screen_image(machine.operator_console(), &screen_font),
            egui::TextureOptions::NEAREST,
        );
        let gui = TerminalGui {
            machine,
            screen_font,
            screen_texture,
            host_message: String::new(),
            keyboard_text: String::new(),
            last_key: "(none)".to_owned(),
            message: String::new(),
            activity_until: None,
            control_panel_open: true,
            panel_power: true,
            panel_running: false,
            processor_check: false,
            lamp_test: false,
            previous_power_fault: false,
            check_run: false,
            force_clock_on: false,
            msipl_diskette: true,
            csipl_diskette: true,
            address_compare_stop: false,
            panel_mode: PanelMode::ProcessorRun,
            address_switches: 0,
            data_switches: 0,
            display_word: 0,
            control_storage_selected: true,
            selected_lsr: super::MAIN_LEVEL_MAR_LSR,
            steps_per_frame: 32,
            diskette_path: String::new(),
        };
        (context, gui)
    }

    #[test]
    fn rasterizes_the_3270_font_into_a_full_5251_screen() {
        let mut console = OperatorConsole::default();
        console.write_at(0, 0, "IBM 3270").unwrap();
        let font = ScreenFont::new();
        let image = screen_image(&console, &font);

        assert_eq!(image.size, [80 * font.cell_width, 24 * font.line_height]);
        assert!(image.pixels.iter().any(|pixel| *pixel != SCREEN_BACKGROUND));
    }

    #[test]
    fn terminal_gui_io_routes_through_the_system34_station() {
        let mut machine = System34::new(Memory::new(48));
        assert!(machine.write_operator_display("S/34"));
        assert_eq!(
            &machine.operator_console().screen()[0][..4],
            &['S', '/', '3', '4']
        );
        assert!(machine.receive_operator_keys(&[TerminalKey::Attention]));
        assert_eq!(machine.cp().pending_interrupt(), Some(4));
        assert!(machine.virtual_twinax_bus().indicators().attention);
    }

    #[test]
    fn ce_panel_alters_control_storage_and_microaddress_from_switches() {
        let (_, mut gui) = test_gui(System34::new(Memory::new(48)));
        gui.panel_mode = PanelMode::AlterStorage;
        gui.address_switches = 0x0123;
        gui.data_switches = 0xa55a;
        gui.ce_start();
        assert_eq!(gui.machine.cp().control_storage[0x0123], 0xa55a);

        gui.panel_mode = PanelMode::AlterMarInterrupt;
        gui.address_switches = 0x0234;
        gui.ce_start();
        assert_eq!(gui.machine.cp().microaddress(), 0x0234);
    }

    #[test]
    fn system_instruction_step_executes_once_and_updates_the_display() {
        let mut machine = System34::new(Memory::new(48));
        machine
            .memory_mut()
            .load(0, &[0x34, 0x00, 0x00, 0x00])
            .unwrap();
        let (_, mut gui) = test_gui(machine);
        gui.panel_mode = PanelMode::SystemInstructionStep;

        gui.ce_start();

        assert_eq!(gui.machine.msp().registers.iar, 4);
        assert_eq!(gui.display_word, gui.machine.msp().registers.psr);
        assert!(gui.message.contains("System instruction step"));
    }

    #[test]
    fn physical_switches_change_their_model_state() {
        let (_, mut gui) = test_gui(System34::new(Memory::new(48)));
        gui.toggle_ce_switch(super::PanelSwitch::StorageSelect);
        assert!(!gui.control_storage_selected);
        gui.toggle_ce_switch(super::PanelSwitch::ForceClock);
        assert!(gui.force_clock_on);
        gui.toggle_ce_switch(super::PanelSwitch::CsiplDiskette);
        assert!(!gui.csipl_diskette);
    }

    #[test]
    fn processor_lamps_are_centered_over_their_display_bytes() {
        assert_eq!(DISPLAY_BYTE_CENTERS, [319.0, 844.0]);
    }

    #[test]
    fn selector_group_labels_are_centered_under_their_dials() {
        assert_eq!(ADDRESS_DIAL_GROUP_CENTER, 580.0);
        assert_eq!(DISPLAY_DIAL_GROUP_CENTER, 870.0);
    }

    #[test]
    fn vector_control_panel_renders() {
        let (context, mut gui) = test_gui(System34::new(Memory::new(48)));
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            gui.draw_control_panel(ui);
        });
        output.textures_delta.clear();
    }
}
