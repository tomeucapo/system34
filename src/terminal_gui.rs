use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, Vec2};
use fontdue::{Font, FontSettings};
use system34::{memory::Memory, System34, TerminalKey};

const SCREEN_BACKGROUND: Color32 = Color32::from_rgb(4, 15, 9);
const PHOSPHOR: Color32 = Color32::from_rgb(117, 255, 157);
const PHOSPHOR_DIM: Color32 = Color32::from_rgb(54, 139, 84);
const ACTIVITY_PULSE: Duration = Duration::from_millis(450);
const FONT_SIZE: f32 = 16.0;

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
}

impl TerminalGui {
    fn new(context: &eframe::CreationContext<'_>) -> Self {
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
}

impl eframe::App for TerminalGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.finish_timed_activity();
        if let Some(deadline) = self.activity_until {
            context.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }

        egui::Frame::new()
            .fill(Color32::from_rgb(35, 34, 29))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("IBM System/34");
                    ui.label("5251 MODEL 11  ·  VIRTUAL TWINAX");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label("SYSTEM CONSOLE  /  STATION 00");
                    });
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
                            indicator(ui, "ATTENTION", indicators.attention, Color32::YELLOW);
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
                                self.message = if self.machine.virtual_twinax_bus().is_online() {
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
                                        "Poll rejected: station offline or in CHECK".to_owned();
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
                                self.message = "Workstation attachment CHECK simulated".to_owned();
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
                                    self.message = "Host clear-screen command sent".to_owned();
                                    self.pulse(&context);
                                } else {
                                    self.message =
                                        "Clear rejected: station offline or in CHECK".to_owned();
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
    }
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
            .with_inner_size([1260.0, 1000.0])
            .with_min_inner_size([960.0, 760.0]),
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
    use super::{screen_image, ScreenFont, SCREEN_BACKGROUND};
    use system34::{memory::Memory, OperatorConsole, System34, TerminalKey};

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
        assert_eq!(&machine.operator_console().screen()[0][..4], "S/34");
        assert!(machine.receive_operator_keys(&[TerminalKey::Attention]));
        assert_eq!(machine.cp().pending_interrupt(), Some(4));
        assert!(machine.virtual_twinax_bus().indicators().attention);
    }
}
