//! Desktop presentation only: analysis and file I/O run on a worker thread.
use crate::{
    analysis,
    domain::*,
    export, ingest,
    llama::{LlamaClient, LlamaConfig},
};
use eframe::egui::{self, Align2, Color32, FontId, RichText, Stroke, Vec2};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

// Editor-style light tokens, adapted from the requested FigCraft visual reference.
const BG: Color32 = Color32::from_rgb(245, 245, 245);
const PANEL: Color32 = Color32::WHITE;
const PANEL_ALT: Color32 = Color32::from_rgb(245, 245, 245);
const BORDER: Color32 = Color32::from_rgb(230, 230, 230);
const BORDER_STRONG: Color32 = Color32::from_rgb(217, 217, 217);
const TEXT: Color32 = Color32::from_rgb(30, 30, 30);
const MUTED: Color32 = Color32::from_rgb(110, 110, 110);
const ACCENT: Color32 = Color32::from_rgb(0, 122, 86);
const BRAND: Color32 = Color32::from_rgb(2, 218, 150);
const GREEN_INK: Color32 = Color32::from_rgb(7, 53, 42);
const SELECTED: Color32 = Color32::from_rgb(224, 251, 241);
const LAVENDER: Color32 = Color32::from_rgb(151, 71, 255);
const WARM: Color32 = Color32::from_rgb(157, 101, 0);
const TAB_BAR: Color32 = Color32::from_rgb(232, 232, 232);

pub fn run() -> anyhow::Result<()> {
    let initial = std::env::var_os("STRUCTUREWEAVER_OPEN_ANALYSIS")
        .map(|path| -> anyhow::Result<Analysis> {
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?;
    run_with_analysis(initial)
}

/// Open a previously exported analysis without invoking the model again.
pub fn run_with_analysis(initial: Option<Analysis>) -> anyhow::Result<()> {
    run_with_initial(initial, None)
}

pub fn run_with_document(path: PathBuf) -> anyhow::Result<()> {
    run_with_initial(None, Some(path))
}

fn run_with_initial(initial: Option<Analysis>, source: Option<PathBuf>) -> anyhow::Result<()> {
    let icon = image::load_from_memory(include_bytes!("../design/brand.png"))?.into_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("StructureWeaver")
            .with_icon(icon)
            .with_inner_size([1360.0, 900.0])
            .with_min_inner_size([1000.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "StructureWeaver",
        options,
        Box::new(move |cc| {
            let mut app = Desktop::new(cc);
            if let Some(result) = initial {
                app.accept_result(result);
            } else if let Some(path) = source {
                app.view = View::Structure;
                app.load(&cc.egui_ctx, path);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Could not open desktop window: {e}"))
}

enum Event {
    Loaded(PathBuf, Document),
    Models(Vec<String>),
    Progress(String),
    Finished(Analysis),
    Exported(PathBuf),
    Error(String),
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum View {
    Diagram,
    Structure,
    Report,
}

struct Desktop {
    endpoint: String,
    model: String,
    models: Vec<String>,
    connected: bool,
    document: Option<Document>,
    file: Option<PathBuf>,
    result: Option<Analysis>,
    receiver: Option<mpsc::Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    busy: bool,
    analyzing: bool,
    status: String,
    error: Option<String>,
    log: Vec<String>,
    view: View,
    selected_page: usize,
    selected_table: Option<String>,
    selected_entity: Option<String>,
    inspect_row: u32,
    inspect_col: u32,
    zoom: f32,
    sheet_view: crate::sheet_view::SheetView,
    excel_preview: crate::excel_preview::ExcelPreview,
}

impl Desktop {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = BG;
        visuals.faint_bg_color = PANEL_ALT;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.inactive.weak_bg_fill = PANEL_ALT;
        visuals.widgets.inactive.bg_fill = PANEL_ALT;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.selection.bg_fill = SELECTED;
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        visuals.widgets.active.bg_fill = SELECTED;
        visuals.widgets.hovered.bg_fill = PANEL_ALT;
        cc.egui_ctx.set_theme(egui::ThemePreference::Light);
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.interact_size.y = 30.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, semibold_font(13.5));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.5));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, semibold_font(24.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
        cc.egui_ctx.set_style(style);

        // Embedded faces keep Korean and Latin typography consistent on every machine.
        // The dedicated family uses real SemiBold outlines instead of synthetic emphasis.
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "pretendard-regular".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Pretendard-Regular.otf"))
                .into(),
        );
        fonts.font_data.insert(
            "pretendard-semibold".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Pretendard-SemiBold.otf"))
                .into(),
        );
        let proportional = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        proportional.insert(0, "pretendard-regular".into());
        let mut semibold = proportional.clone();
        semibold.insert(0, "pretendard-semibold".into());
        fonts.families.insert(semibold_family(), semibold);
        // Preserve the default monospace face for coordinates/data; Korean falls back to Pretendard.
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("pretendard-regular".into());
        cc.egui_ctx.set_fonts(fonts.clone());
        Self {
            endpoint: "http://127.0.0.1:9931/v1".into(),
            model: String::new(),
            models: vec![],
            connected: false,
            document: None,
            file: None,
            result: None,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            busy: false,
            analyzing: false,
            status: "엑셀 파일을 열어 분석을 시작하세요.".into(),
            error: None,
            log: vec![],
            view: View::Diagram,
            selected_page: 0,
            selected_table: None,
            selected_entity: None,
            inspect_row: 1,
            inspect_col: 1,
            zoom: 1.0,
            sheet_view: crate::sheet_view::SheetView::new(fonts),
            excel_preview: Default::default(),
        }
    }

    fn worker(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce(mpsc::Sender<Event>) + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.busy = true;
        self.error = None;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let failure_tx = tx.clone();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(tx))).is_err() {
                let _ = failure_tx.send(Event::Error("작업 중 예기치 않은 오류가 발생했습니다. 입력 파일을 확인한 뒤 다시 시도할 수 있습니다.".into()));
            }
            ctx.request_repaint();
        });
    }

    fn load(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.busy {
            return;
        }
        self.status = "통합 문서의 모든 시트를 읽고 있습니다…".into();
        self.worker(ctx, move |tx| {
            let event = match ingest::read_document(&path) {
                Ok(document) => Event::Loaded(path, document),
                Err(error) => Event::Error(format!("파일을 읽을 수 없습니다: {error:#}")),
            };
            let _ = tx.send(event);
        });
    }

    fn config(&self) -> LlamaConfig {
        LlamaConfig {
            base_url: self.endpoint.trim().trim_end_matches('/').into(),
            model: self.model.trim().into(),
            timeout_secs: 180,
        }
    }

    fn connect(&mut self, ctx: &egui::Context) {
        self.connected = false;
        self.status = "로컬 Llama에 연결하고 있습니다…".into();
        let config = self.config();
        self.worker(ctx, move |tx| {
            let event = match LlamaClient::new(config).and_then(|client| client.models()) {
                Ok(models) => Event::Models(models),
                Err(error) => Event::Error(format!("Llama 연결 실패: {error:#}")),
            };
            let _ = tx.send(event);
        });
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let config = self.config();
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.analyzing = true;
        self.result = None;
        self.selected_table = None;
        self.selected_entity = None;
        self.log.clear();
        self.status = "문서 구조 분석을 시작합니다…".into();
        self.worker(ctx, move |tx| {
            let progress_tx = tx.clone();
            let outcome = analysis::analyze(
                document,
                config,
                move |message| {
                    let _ = progress_tx.send(Event::Progress(message));
                },
                cancel,
            );
            let event = match outcome {
                Ok(result) => Event::Finished(result),
                Err(error) => Event::Error(format!("분석이 중단되었습니다: {error:#}")),
            };
            let _ = tx.send(event);
        });
    }

    fn export(&mut self, ctx: &egui::Context) {
        let Some(result) = self.result.clone() else {
            return;
        };
        if let Some(directory) = rfd::FileDialog::new()
            .set_title("분석 결과 · Mermaid ERD · SQL 초안 저장")
            .pick_folder()
        {
            self.status = "분석 결과를 내보내고 있습니다…".into();
            self.worker(ctx, move |tx| {
                let event = match export::write_outputs(&result, &directory) {
                    Ok(()) => Event::Exported(directory),
                    Err(error) => Event::Error(format!("내보내기 실패: {error:#}")),
                };
                let _ = tx.send(event);
            });
        }
    }

    fn accept_result(&mut self, result: Analysis) {
        self.sheet_view.invalidate();
        let incomplete = result
            .pages
            .iter()
            .filter(|p| !matches!(p.state.as_str(), "complete" | "empty"))
            .count();
        self.status = format!(
            "테이블 {}개 · 엔터티 {}개 · 검토가 필요한 시트 {}개",
            result.tables.len(),
            result.schema.entities.len(),
            incomplete
        );
        self.selected_table = result.tables.first().map(|t| t.id.clone());
        self.selected_entity = result.schema.entities.first().map(|e| e.id.clone());
        self.model = result.model.clone();
        self.document = Some(result.document.clone());
        self.view = if result.tables.is_empty() {
            View::Report
        } else {
            View::Diagram
        };
        self.selected_page = 0;
        self.result = Some(result);
        self.busy = false;
        self.analyzing = false;
    }

    fn poll(&mut self) {
        let events: Vec<_> = self
            .receiver
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(message) => {
                    self.status = message.clone();
                    self.log.push(message);
                }
                Event::Loaded(path, document) => {
                    self.sheet_view.invalidate();
                    self.status = format!(
                        "숨김 시트를 포함해 {}개 시트를 읽었습니다.",
                        document.pages.len()
                    );
                    self.file = Some(path);
                    self.document = Some(document);
                    self.result = None;
                    self.selected_page = 0;
                    self.selected_table = None;
                    self.selected_entity = None;
                    self.inspect_row = 1;
                    self.inspect_col = 1;
                    self.log.clear();
                    self.busy = false;
                }
                Event::Models(models) => {
                    self.connected = true;
                    self.status = format!("Llama 연결 완료 · 사용 가능한 모델 {}개", models.len());
                    if !models.contains(&self.model) {
                        self.model = models.first().cloned().unwrap_or_default();
                    }
                    if models.is_empty() {
                        self.status =
                            "Llama에 연결했지만 모델이 없습니다. 이미지 입력을 지원하는 모델을 불러오세요."
                                .into();
                    }
                    self.models = models;
                    self.busy = false;
                }
                Event::Finished(result) => self.accept_result(result),
                Event::Exported(path) => {
                    self.status = format!("저장 완료: {}", path.display());
                    self.busy = false;
                }
                Event::Error(message) => {
                    self.error = Some(message);
                    self.status = "작업을 완료하지 못했습니다. 아래 내용을 확인하세요.".into();
                    self.busy = false;
                    self.analyzing = false;
                }
            }
        }
        if !self.busy {
            self.receiver = None;
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("workspace")
            .resizable(false)
            .exact_width(236.0)
            .frame(egui::Frame::NONE.fill(PANEL).inner_margin(16.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_salt("workspace-scroll").show(ui, |ui| {
                    eyebrow(ui, "파일", MUTED);
                    ui.add_space(12.0);
                    if ui.add_enabled(!self.busy,
                        egui::Button::new(RichText::new("+  엑셀 파일 열기").color(TEXT))
                            .min_size(Vec2::new(ui.available_width(), 40.0))
                            .fill(PANEL_ALT).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(5.0)
                    ).clicked() && let Some(path) = rfd::FileDialog::new().add_filter("Excel", &["xlsx", "xlsm"]).pick_file() {
                        self.load(ctx, path);
                    }
                    ui.add_space(3.0);
                    ui.label(RichText::new("XLSX · XLSM / 파일을 끌어 놓으세요").size(11.5).color(MUTED));
                    ui.add_space(20.0);
                    if let Some(document) = &self.document {
                        card_frame().show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            eyebrow(ui, "현재 문서", ACCENT);
                            ui.add_space(6.0);
                            ui.label(RichText::new(&document.name).size(15.0).family(semibold_family()));
                            ui.add_space(3.0);
                            ui.label(RichText::new(format!("{}개 시트 · 원본 좌표 유지", document.pages.len())).size(11.5).color(MUTED));
                        });
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            eyebrow(ui, "시트 탐색", MUTED);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { pill(ui, &document.pages.len().to_string(), MUTED); });
                        });
                        ui.add_space(7.0);
                        egui::ScrollArea::vertical().id_salt("sheet-list").max_height(235.0).show(ui, |ui| {
                            for (index, page) in document.pages.iter().enumerate() {
                                let selected = self.selected_page == index;
                                let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 53.0), egui::Sense::click());
                                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, &page.name));
                                let painter = ui.painter();
                                if selected || response.hovered() {
                                    painter.rect_filled(rect, 5.0, if selected { SELECTED } else { PANEL_ALT });
                                }
                                if selected { painter.rect_filled(egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 11.0), Vec2::new(2.0, 31.0)), 1.0, ACCENT); }
                                let icon = rect.min + Vec2::new(16.0, 18.0);
                                painter.rect_stroke(egui::Rect::from_center_size(icon, Vec2::new(12.0, 13.0)), 2.0, Stroke::new(1.0_f32, if selected { ACCENT } else { MUTED }), egui::StrokeKind::Inside);
                                painter.line_segment([icon + Vec2::new(-4.0, -1.0), icon + Vec2::new(4.0, -1.0)], Stroke::new(1.0_f32, MUTED));
                                let sheet_font = if selected { semibold_font(13.5) } else { FontId::proportional(13.5) };
                                painter.text(rect.min + Vec2::new(31.0, 18.0), Align2::LEFT_CENTER, fit_label(painter, &page.name, sheet_font.clone(), rect.width() - 40.0), sheet_font, if selected { TEXT } else { MUTED });
                                if let Some(grid) = &page.grid {
                                    painter.text(rect.min + Vec2::new(31.0, 37.0), Align2::LEFT_CENTER, format!("{} × {}{}", grid.rows, grid.cols, if page.hidden { " · 숨김" } else { "" }), FontId::proportional(11.5), MUTED);
                                }
                                if response.clicked() {
                                    self.selected_page = index;
                                    self.inspect_row = 1;
                                    self.inspect_col = 1;
                                    self.selected_table = self.result.as_ref().and_then(|r| r.tables.iter().find(|t| t.page_id == page.id)).map(|t| t.id.clone());
                                    self.view = View::Structure;
                                }
                                response.on_hover_text(&page.name);
                            }
                        });
                    } else {
                        card_frame().show(ui, |ui| {
                            ui.label(RichText::new("문서를 기다리고 있어요").size(14.0).family(semibold_family()));
                            ui.add_space(6.0);
                            ui.label(RichText::new("숨김 시트를 포함한 전체 구조를 함께 분석합니다.").size(13.0).color(MUTED));
                        });
                    }
                    ui.add_space(24.0);
                    ui.separator();
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        eyebrow(ui, "LOCAL LLAMA", ACCENT);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            pill(ui, if self.connected { "연결됨" } else { "연결 대기" }, if self.connected { ACCENT } else { MUTED });
                        });
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("비전 모델").size(13.0).color(MUTED));
                    ui.add_enabled_ui(!self.busy, |ui| {
                        let label = if self.model.is_empty() { "모델을 선택하세요".into() } else { short_model(&self.model) };
                        egui::ComboBox::from_id_salt("model").selected_text(label).width(ui.available_width() - 10.0).show_ui(ui, |ui| {
                            for model in &self.models { ui.selectable_value(&mut self.model, model.clone(), model); }
                        }).response.on_hover_text(&self.model);
                        ui.add_space(4.0);
                        ui.collapsing(RichText::new("연결 설정").size(13.0).color(MUTED), |ui| {
                            ui.label(RichText::new("API URL").small().color(MUTED));
                            if ui.add(egui::TextEdit::singleline(&mut self.endpoint).desired_width(f32::INFINITY)).changed() { self.connected = false; }
                            ui.label(RichText::new("모델 ID 직접 입력").small().color(MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.model).desired_width(f32::INFINITY));
                        });
                    });
                    if ui.add_enabled(!self.busy, egui::Button::new("모델 연결 / 새로고침").min_size(Vec2::new(ui.available_width(), 33.0)).corner_radius(6.0)).clicked() { self.connect(ctx); }
                    ui.add_space(7.0);
                    ui.label(RichText::new("이 컴퓨터의 Llama에서 분석합니다.").size(11.5).color(MUTED));
                    ui.add_space(19.0);
                    if self.analyzing {
                        if ui.add_enabled(!self.cancel.load(Ordering::Relaxed), egui::Button::new("분석 취소").min_size(Vec2::new(ui.available_width(), 42.0))).clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                            self.status = "취소를 요청했습니다. 현재 Llama 응답을 기다립니다 (최대 180초).".into();
                        }
                        ui.label(RichText::new("진행 중인 요청은 응답 또는 제한 시간까지 기다립니다.").size(11.5).color(MUTED));
                    } else if ui.add_enabled(!self.busy && self.document.is_some() && !self.model.trim().is_empty(),
                        egui::Button::new(RichText::new("전체 시트 분석하기  →").family(semibold_family()).color(GREEN_INK))
                            .fill(BRAND).corner_radius(5.0).min_size(Vec2::new(ui.available_width(), 43.0))
                    ).clicked() { self.start(ctx); }
                    ui.add_space(18.0);
                    ui.label(RichText::new("문서 → 구조 → 관계").size(11.5).color(MUTED));
                });
            });
    }

    fn structure(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let Some(page) = document.pages.get(self.selected_page) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&page.name)
                    .size(17.0)
                    .family(semibold_family()),
            );
            if page.hidden {
                ui.label(
                    RichText::new("숨김 시트 · 분석에 포함됨")
                        .size(11.5)
                        .color(WARM),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("원본 서식 · 읽기 전용")
                        .size(11.5)
                        .color(MUTED),
                );
            });
        });
        let tables: Vec<_> = self
            .result
            .as_ref()
            .map(|r| r.tables.iter().filter(|t| t.page_id == page.id).collect())
            .unwrap_or_default();
        if !tables.is_empty() {
            ui.collapsing(
                format!("분석한 구조 · {}개 테이블", tables.len()),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for table in &tables {
                            let selected = self.selected_table.as_ref() == Some(&table.id);
                            let label = RichText::new(&table.name).family(if selected {
                                semibold_family()
                            } else {
                                egui::FontFamily::Proportional
                            });
                            if ui.selectable_label(selected, label).clicked() {
                                self.selected_table = Some(table.id.clone());
                            }
                        }
                    });
                    if let Some(table) = tables
                        .iter()
                        .find(|t| Some(&t.id) == self.selected_table.as_ref())
                    {
                        egui::ScrollArea::vertical()
                            .id_salt("source-analysis-details")
                            .max_height(155.0)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for (name, bounds) in [
                                        ("테이블", Some(table.bounds)),
                                        ("제목", table.title),
                                        ("열 헤더", table.column_header),
                                        ("행 헤더", table.row_header),
                                        ("값", Some(table.data)),
                                    ] {
                                        if let Some(bounds) = bounds {
                                            ui.label(format!("{name} {}", range_name(bounds)));
                                        }
                                    }
                                });
                                ui.collapsing(
                                    format!("헤더 계층 · {}개 노드", table.headers.len()),
                                    |ui| {
                                        for header in &table.headers {
                                            let parent = header
                                                .parent_id
                                                .as_ref()
                                                .and_then(|id| {
                                                    table.headers.iter().find(|h| &h.id == id)
                                                })
                                                .map(|h| h.label.as_str())
                                                .unwrap_or("root");
                                            ui.label(format!(
                                                "{}  {} → {}  [{}]",
                                                header.axis,
                                                parent,
                                                header.label,
                                                range_name(header.bounds)
                                            ));
                                        }
                                    },
                                );
                            });
                    }
                },
            );
        }
        ui.add_space(5.0);
        if let Some(grid) = &page.grid {
            self.excel_preview
                .controls(ui, &document.id, self.file.as_deref());
            self.sheet_view.show(
                ui,
                (&format!("{}:{}", document.id, page.id), &page.name),
                grid,
                (&mut self.inspect_row, &mut self.inspect_col),
                &tables,
                &mut self.excel_preview,
            );
        } else {
            ui.label("이 페이지는 시트 미리보기를 지원하지 않습니다.");
        }
    }

    fn diagram(&mut self, ui: &mut egui::Ui, compact_inspector: bool) {
        let Some(result) = self.result.as_ref() else {
            self.empty(ui);
            return;
        };
        if result.schema.entities.is_empty() {
            card_frame().show(ui, |ui| {
                ui.label("생성된 엔터티가 없습니다. 분석 범위에서 각 시트의 결과를 확인하세요.");
            });
            return;
        }
        let z = self.zoom;
        let card_width = 252.0 * z;
        let col_gap = 64.0 * z;
        let row_gap = 65.0 * z;
        let available = (ui.available_width() - 2.0).max(280.0);
        let columns = (((available - 36.0 + col_gap) / (card_width + col_gap)).floor() as usize)
            .clamp(1, 3)
            .min(result.schema.entities.len());
        let content_width =
            (columns as f32 * card_width + columns.saturating_sub(1) as f32 * col_gap + 36.0)
                .max(available);
        let mut positions = HashMap::new();
        let mut y = 28.0;
        for chunk in result.schema.entities.chunks(columns) {
            let row_width =
                chunk.len() as f32 * card_width + chunk.len().saturating_sub(1) as f32 * col_gap;
            let origin_x = (content_width - row_width) * 0.5;
            let max_height = chunk
                .iter()
                .map(|e| 92.0 + 27.0 * e.attributes.len().max(1) as f32)
                .fold(0.0f32, f32::max)
                * z;
            for (col, entity) in chunk.iter().enumerate() {
                positions.insert(
                    entity.id.clone(),
                    egui::Rect::from_min_size(
                        egui::pos2(origin_x + col as f32 * (card_width + col_gap), y),
                        Vec2::new(
                            card_width,
                            (92.0 + 27.0 * entity.attributes.len().max(1) as f32) * z,
                        ),
                    ),
                );
            }
            y += max_height + row_gap;
        }
        let reserved = if compact_inspector { 192.0 } else { 35.0 };
        let canvas_height = (ui.available_height() - reserved).max(220.0);
        let graph = egui::Frame::NONE
            .fill(BG)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(2.0)
            .show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("erd-canvas")
                    .auto_shrink([false, false])
                    .max_height(canvas_height)
                    .show(ui, |ui| {
                        let size =
                            Vec2::new(content_width, (y - row_gap + 90.0).max(canvas_height));
                        let (canvas, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        let painter = ui.painter_at(canvas);
                        let visible = painter.clip_rect().intersect(canvas);
                        let dot_step = 24.0;
                        let start_x = canvas.left()
                            + ((visible.left() - canvas.left()) / dot_step).floor() * dot_step;
                        let start_y = canvas.top()
                            + ((visible.top() - canvas.top()) / dot_step).floor() * dot_step;
                        for row in 0..=((visible.height() / dot_step) as usize + 1) {
                            for col in 0..=((visible.width() / dot_step) as usize + 1) {
                                painter.circle_filled(
                                    egui::pos2(
                                        start_x + col as f32 * dot_step,
                                        start_y + row as f32 * dot_step,
                                    ),
                                    0.85,
                                    BORDER,
                                );
                            }
                        }
                        for (index, relation) in result.schema.relationships.iter().enumerate() {
                            let (Some(from), Some(to)) = (
                                positions.get(&relation.from_entity),
                                positions.get(&relation.to_entity),
                            ) else {
                                continue;
                            };
                            let from = from.translate(canvas.min.to_vec2());
                            let to = to.translate(canvas.min.to_vec2());
                            let row_for = |id: &str, attr: &str| {
                                result
                                    .schema
                                    .entities
                                    .iter()
                                    .find(|e| e.id == id)
                                    .and_then(|e| e.attributes.iter().position(|a| a.name == attr))
                                    .unwrap_or(0) as f32
                            };
                            let fy = from.top()
                                + (72.0
                                    + row_for(&relation.from_entity, &relation.from_attribute)
                                        * 27.0)
                                    * z;
                            let ty = to.top()
                                + (72.0
                                    + row_for(&relation.to_entity, &relation.to_attribute) * 27.0)
                                    * z;
                            let mut points = Vec::new();
                            if (from.center().x - to.center().x).abs() < card_width * 0.6 {
                                let lane = from.right().max(to.right())
                                    + (22.0 + (index % 3) as f32 * 8.0) * z;
                                points.extend([
                                    egui::pos2(from.right(), fy),
                                    egui::pos2(lane, fy),
                                    egui::pos2(lane, ty),
                                    egui::pos2(to.right(), ty),
                                ]);
                            } else if from.center().x < to.center().x {
                                let lane = (from.right() + to.left()) * 0.5;
                                points.extend([
                                    egui::pos2(from.right(), fy),
                                    egui::pos2(lane, fy),
                                    egui::pos2(lane, ty),
                                    egui::pos2(to.left(), ty),
                                ]);
                            } else {
                                let lane = (from.left() + to.right()) * 0.5;
                                points.extend([
                                    egui::pos2(from.left(), fy),
                                    egui::pos2(lane, fy),
                                    egui::pos2(lane, ty),
                                    egui::pos2(to.right(), ty),
                                ]);
                            }
                            let color = if self.selected_entity.as_ref().is_some_and(|id| {
                                id == &relation.from_entity || id == &relation.to_entity
                            }) {
                                ACCENT
                            } else {
                                LAVENDER
                            };
                            painter.extend(egui::Shape::dashed_line(
                                &points,
                                Stroke::new(1.5_f32, color),
                                6.0,
                                4.0,
                            ));
                            let start = points[0];
                            let end = points[3];
                            painter.circle_filled(start, 3.0 * z, color);
                            let direction = if end.x >= points[2].x { -1.0 } else { 1.0 };
                            painter.line_segment(
                                [end, end + Vec2::new(7.0 * direction, -4.0) * z],
                                Stroke::new(1.5_f32, color),
                            );
                            painter.line_segment(
                                [end, end + Vec2::new(7.0 * direction, 4.0) * z],
                                Stroke::new(1.5_f32, color),
                            );
                            let label_pos = egui::pos2(points[1].x, (fy + ty) * 0.5 - 10.0);
                            let label_rect =
                                egui::Rect::from_center_size(label_pos, Vec2::new(42.0, 20.0));
                            painter.rect_filled(label_rect, 5.0, PANEL_ALT);
                            painter.text(
                                label_pos,
                                Align2::CENTER_CENTER,
                                cardinality_label(&relation.cardinality),
                                FontId::monospace(11.0),
                                color,
                            );
                        }
                        for entity in &result.schema.entities {
                            let rect = positions[&entity.id].translate(canvas.min.to_vec2());
                            let selected = self.selected_entity.as_ref() == Some(&entity.id);
                            let response =
                                ui.interact(rect, ui.id().with(&entity.id), egui::Sense::click());
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    ui.is_enabled(),
                                    selected,
                                    &entity.name,
                                )
                            });
                            painter.rect_filled(
                                rect.translate(Vec2::new(0.0, 4.0)),
                                10.0,
                                Color32::from_black_alpha(16),
                            );
                            painter.rect_filled(
                                rect,
                                10.0,
                                if response.hovered() { PANEL_ALT } else { PANEL },
                            );
                            painter.rect_stroke(
                                rect,
                                10.0,
                                Stroke::new(
                                    if selected { 1.7_f32 } else { 1.0_f32 },
                                    if selected { ACCENT } else { BORDER },
                                ),
                                egui::StrokeKind::Inside,
                            );
                            let accent = if selected { ACCENT } else { LAVENDER };
                            painter.line_segment(
                                [
                                    rect.min + Vec2::new(12.0, 1.0),
                                    egui::pos2(rect.right() - 12.0, rect.top() + 1.0),
                                ],
                                Stroke::new(2.0_f32, accent),
                            );
                            painter.text(
                                rect.min + Vec2::new(15.0, 23.0) * z,
                                Align2::LEFT_CENTER,
                                fit_label(
                                    &painter,
                                    &entity.name,
                                    semibold_font(16.0 * z),
                                    card_width - 30.0 * z,
                                ),
                                semibold_font(16.0 * z),
                                TEXT,
                            );
                            let sheet = result
                                .tables
                                .iter()
                                .find(|t| t.id == entity.table_id)
                                .and_then(|t| {
                                    result.document.pages.iter().find(|p| p.id == t.page_id)
                                })
                                .map(|p| p.name.as_str())
                                .unwrap_or(&entity.table_id);
                            painter.text(
                                rect.min + Vec2::new(15.0, 44.0) * z,
                                Align2::LEFT_CENTER,
                                fit_label(
                                    &painter,
                                    &format!("{sheet} · {}개 행", entity.row_count),
                                    FontId::proportional(11.0 * z),
                                    card_width - 30.0 * z,
                                ),
                                FontId::proportional(11.0 * z),
                                MUTED,
                            );
                            painter.line_segment(
                                [
                                    rect.min + Vec2::new(0.0, 58.0) * z,
                                    egui::pos2(rect.right(), rect.top() + 58.0 * z),
                                ],
                                Stroke::new(1.0_f32, BORDER),
                            );
                            for (index, attr) in entity.attributes.iter().enumerate() {
                                let point =
                                    rect.min + Vec2::new(14.0, 72.0 + index as f32 * 27.0) * z;
                                let foreign = result.schema.relationships.iter().any(|r| {
                                    r.from_entity == entity.id && r.from_attribute == attr.name
                                });
                                let (badge, color) = if attr.primary_key {
                                    ("PK", WARM)
                                } else if foreign {
                                    ("FK", LAVENDER)
                                } else {
                                    ("·", MUTED)
                                };
                                painter.text(
                                    point,
                                    Align2::LEFT_CENTER,
                                    badge,
                                    FontId::monospace(11.0 * z),
                                    color,
                                );
                                painter.text(
                                    point + Vec2::new(26.0, 0.0) * z,
                                    Align2::LEFT_CENTER,
                                    fit_label(
                                        &painter,
                                        &attr.name,
                                        FontId::proportional(12.5 * z),
                                        card_width - 104.0 * z,
                                    ),
                                    FontId::proportional(12.5 * z),
                                    TEXT,
                                );
                                painter.text(
                                    egui::pos2(rect.right() - 12.0 * z, point.y),
                                    Align2::RIGHT_CENTER,
                                    &attr.data_type,
                                    FontId::monospace(11.0 * z),
                                    MUTED,
                                );
                            }
                            painter.text(
                                egui::pos2(rect.left() + 14.0 * z, rect.bottom() - 14.0 * z),
                                Align2::LEFT_CENTER,
                                "원본 셀에서 추론한 모델",
                                FontId::proportional(11.0 * z),
                                MUTED,
                            );
                            if response.clicked() {
                                self.selected_entity = Some(entity.id.clone());
                            }
                            response.on_hover_text(format!(
                                "{} · {}개 속성\n선택하면 원본과 추론 근거를 볼 수 있습니다.",
                                entity.name,
                                entity.attributes.len()
                            ));
                        }
                    });
            });
        let toolbar = egui::Rect::from_center_size(
            egui::pos2(
                graph.response.rect.center().x,
                graph.response.rect.bottom() - 34.0,
            ),
            Vec2::new(278.0, 44.0),
        );
        ui.painter().rect_filled(
            toolbar.translate(Vec2::new(0.0, 3.0)),
            13.0,
            Color32::from_black_alpha(15),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(toolbar), |ui| {
            egui::Frame::NONE
                .fill(PANEL)
                .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
                .corner_radius(13.0)
                .inner_margin(egui::Margin::symmetric(9, 7))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        egui::Frame::NONE
                            .fill(BRAND)
                            .corner_radius(5.0)
                            .inner_margin(egui::Margin::symmetric(7, 4))
                            .show(ui, |ui| {
                                ui.label(RichText::new("↖").color(GREEN_INK).size(16.0))
                                    .on_hover_text("엔터티를 선택해 근거 보기");
                            });
                        ui.separator();
                        if ui.button("−").on_hover_text("축소").clicked() {
                            self.zoom = (self.zoom - 0.1).max(0.7);
                        }
                        ui.label(RichText::new(format!("{:.0}%", self.zoom * 100.0)).size(13.0));
                        if ui.button("+").on_hover_text("확대").clicked() {
                            self.zoom = (self.zoom + 0.1).min(1.35);
                        }
                        ui.separator();
                        if ui.button("화면 맞춤").clicked() {
                            self.zoom = 1.0;
                        }
                    });
                });
        });
        ui.add_space(7.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("점선 = 관계 후보").size(11.5).color(ACCENT));
            ui.label(RichText::new("·").color(MUTED));
            ui.label(
                RichText::new("PK / FK는 관측값 기반 추론입니다. 적용 전 검토하세요.")
                    .size(11.5)
                    .color(MUTED),
            );
        });
        if compact_inspector {
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .id_salt("compact-evidence")
                .max_height(145.0)
                .show(ui, |ui| {
                    self.evidence(ui);
                });
        }
    }

    fn evidence(&mut self, ui: &mut egui::Ui) {
        let Some(result) = self.result.as_ref() else {
            return;
        };
        eyebrow(ui, "원본 · 추론 근거", ACCENT);
        ui.add_space(10.0);
        let Some(entity) = result
            .schema
            .entities
            .iter()
            .find(|e| Some(&e.id) == self.selected_entity.as_ref())
        else {
            ui.label(
                RichText::new("엔터티를 선택하면 원본 셀과 추론 근거를 보여줍니다.").color(MUTED),
            );
            return;
        };
        ui.label(
            RichText::new(&entity.name)
                .size(21.0)
                .family(semibold_family()),
        );
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            pill(ui, &format!("{}개 행", entity.row_count), MUTED);
            pill(ui, &format!("{}개 속성", entity.attributes.len()), LAVENDER);
        });
        ui.add_space(17.0);
        card_frame().show(ui, |ui| {
            ui.label(
                RichText::new("원본과 연결된 설계")
                    .size(13.0)
                    .color(ACCENT)
                    .family(semibold_family()),
            );
            ui.add_space(5.0);
            ui.label(
                RichText::new("셀 범위를 눌러 실제 값과 헤더를 확인하세요.")
                    .size(13.0)
                    .color(MUTED),
            );
        });
        ui.add_space(18.0);
        eyebrow(ui, "속성 / SOURCE", MUTED);
        ui.add_space(8.0);
        for attribute in &entity.attributes {
            egui::CollapsingHeader::new(
                RichText::new(&attribute.name)
                    .size(13.5)
                    .family(semibold_family()),
            )
            .id_salt((&entity.id, &attribute.name))
            .default_open(attribute.primary_key)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    pill(ui, &attribute.data_type, MUTED);
                    if attribute.primary_key {
                        pill(ui, "PK 후보", WARM);
                    }
                    if attribute.nullable {
                        pill(ui, "NULL 허용", MUTED);
                    }
                });
                if let SourceRegion::Cells { bounds } = attribute.source.region {
                    let page = result
                        .document
                        .pages
                        .iter()
                        .find(|p| p.id == attribute.source.page_id);
                    let name = page
                        .map(|p| p.name.as_str())
                        .unwrap_or(&attribute.source.page_id);
                    if ui
                        .link(format!("{name} · {}  ↗", range_name(bounds)))
                        .clicked()
                        && let Some(index) = result
                            .document
                            .pages
                            .iter()
                            .position(|p| p.id == attribute.source.page_id)
                    {
                        self.selected_page = index;
                        self.selected_table = Some(entity.table_id.clone());
                        self.inspect_row = bounds.top;
                        self.inspect_col = bounds.left;
                        self.view = View::Structure;
                    }
                }
                ui.label(RichText::new("관측값").size(11.5).color(MUTED));
                ui.label(RichText::new(attribute.sample_values.join(" · ")).size(13.0));
            });
            ui.add_space(3.0);
        }
        ui.add_space(17.0);
        ui.separator();
        ui.add_space(15.0);
        eyebrow(ui, "관계 근거", LAVENDER);
        ui.add_space(9.0);
        let relations: Vec<_> = result
            .schema
            .relationships
            .iter()
            .filter(|r| r.from_entity == entity.id || r.to_entity == entity.id)
            .collect();
        if relations.is_empty() {
            ui.label(
                RichText::new("이 엔터티에서 확인된 관계 후보가 없습니다.")
                    .size(13.0)
                    .color(MUTED),
            );
        }
        for relation in relations {
            card_frame().show(ui, |ui| {
                let from = result
                    .schema
                    .entities
                    .iter()
                    .find(|e| e.id == relation.from_entity)
                    .map(|e| e.name.as_str())
                    .unwrap_or(&relation.from_entity);
                let to = result
                    .schema
                    .entities
                    .iter()
                    .find(|e| e.id == relation.to_entity)
                    .map(|e| e.name.as_str())
                    .unwrap_or(&relation.to_entity);
                ui.label(
                    RichText::new(format!("{from} → {to}"))
                        .size(13.0)
                        .family(semibold_family()),
                );
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    pill(ui, cardinality_label(&relation.cardinality), ACCENT);
                    pill(
                        ui,
                        &format!("{:.0}%", relation.confidence * 100.0),
                        LAVENDER,
                    );
                });
                ui.label(RichText::new(&relation.evidence).size(11.5).color(MUTED));
            });
            ui.add_space(8.0);
        }
    }

    fn report(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("report")
            .show(ui, |ui| {
                ui.heading("분석 범위와 추론 근거");
                if let Some(result) = &self.result {
                    ui.label(format!("사용 모델: {}", result.model));
                    ui.add_space(10.0);
                    for page in &result.pages {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&page.page_name).family(semibold_family()));
                                let failed = page.state.contains("fail")
                                    || page.state.contains("error")
                                    || page.state.contains("partial");
                                ui.colored_label(
                                    if failed { WARM } else { ACCENT },
                                    state_label(&page.state),
                                );
                                ui.label(format!(
                                    "{}/{}개 영역",
                                    page.tiles_completed, page.tiles_total
                                ));
                            });
                            ui.label(&page.message);
                        });
                    }
                    for warning in result
                        .document
                        .warnings
                        .iter()
                        .chain(&result.warnings)
                        .chain(&result.schema.warnings)
                    {
                        ui.colored_label(WARM, warning);
                    }
                    ui.add_space(15.0);
                    ui.label(RichText::new("관계 후보").small().color(ACCENT));
                    for relationship in &result.schema.relationships {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.monospace(format!(
                                "{}.{} → {}.{}",
                                relationship.from_entity,
                                relationship.from_attribute,
                                relationship.to_entity,
                                relationship.to_attribute
                            ));
                            ui.label(format!(
                                "{} · {} · 신뢰도 {:.0}%",
                                relationship.cardinality,
                                relationship.status,
                                relationship.confidence * 100.0
                            ));
                            ui.label(&relationship.evidence);
                        });
                    }
                } else if let Some(document) = &self.document {
                    for warning in &document.warnings {
                        ui.colored_label(WARM, warning);
                    }
                    ui.label("분석을 시작하면 전체 시트의 처리 범위와 관계 근거를 보여줍니다.");
                }
                ui.add_space(16.0);
                ui.separator();
                ui.label(RichText::new("실행 기록").small().color(MUTED));
                for message in &self.log {
                    ui.label(message);
                }
            });
    }

    fn empty(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("welcome")
            .show(ui, |ui| {
                ui.add_space(28.0);
                ui.vertical_centered(|ui| {
                    woven_mark(ui, 62.0);
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new("문서에 숨은 구조를 발견하세요.")
                            .size(28.0)
                            .color(TEXT)
                            .family(semibold_family()),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("흩어진 시트와 복잡한 헤더를, 연결된 데이터 모델로.")
                            .size(15.0)
                            .color(MUTED),
                    );
                    ui.add_space(30.0);
                    let width = ui.available_width().min(600.0);
                    for (number, title, detail, color) in [
                        (
                            "01",
                            "엑셀 파일을 불러오세요",
                            "모든 시트와 병합 헤더, 숨김 영역까지 읽습니다.",
                            ACCENT,
                        ),
                        (
                            "02",
                            "로컬 Llama로 구조를 분석하세요",
                            "시트 이미지와 정확한 셀 정보를 함께 해석합니다.",
                            LAVENDER,
                        ),
                        (
                            "03",
                            "관계를 검토하고 ERD를 내보내세요",
                            "원본 셀을 추적하며 키와 관계 후보를 검토합니다.",
                            WARM,
                        ),
                    ] {
                        card_frame().show(ui, |ui| {
                            ui.set_width((width - 26.0).max(200.0));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(number).size(26.0).color(color));
                                ui.add_space(10.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(title).size(15.0).family(semibold_family()),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(detail).size(13.0).color(MUTED));
                                });
                            });
                        });
                        ui.add_space(10.0);
                    }
                    ui.add_space(15.0);
                    ui.label(
                        RichText::new("EXCEL  →  STRUCTURE  →  ERD")
                            .size(11.5)
                            .color(MUTED),
                    );
                });
            });
    }

    fn overview(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.view {
                    View::Diagram => "관계 다이어그램",
                    View::Structure => "테이블 · 원본",
                    View::Report => "분석 범위 · 근거",
                })
                .size(13.5)
                .family(semibold_family()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(result) = &self.result {
                    ui.label(
                        RichText::new(format!(
                            "{}개 시트  ·  {}개 엔터티  ·  {}개 관계 후보",
                            result.document.pages.len(),
                            result.schema.entities.len(),
                            result.schema.relationships.len()
                        ))
                        .size(11.5)
                        .color(MUTED),
                    );
                } else {
                    ui.label(
                        RichText::new("Excel → Structure → ERD")
                            .size(11.5)
                            .color(MUTED),
                    );
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(10.0);
    }

    fn toolrail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("toolrail")
            .resizable(false)
            .exact_width(48.0)
            .frame(egui::Frame::NONE.fill(PANEL).inner_margin(6.0))
            .show(ctx, |ui| {
                ui.add_space(10.0);
                for (view, label, kind) in [
                    (View::Diagram, "관계", 0),
                    (View::Structure, "원본", 1),
                    (View::Report, "근거", 2),
                ] {
                    let selected = self.view == view;
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(35.0, 35.0), egui::Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            ui.is_enabled(),
                            selected,
                            label,
                        )
                    });
                    let painter = ui.painter();
                    if selected || response.hovered() {
                        painter.rect_filled(rect, 5.0, if selected { SELECTED } else { PANEL_ALT });
                    }
                    let color = if selected { ACCENT } else { TEXT };
                    let center = rect.center();
                    let stroke = Stroke::new(1.2_f32, color);
                    if kind == 0 {
                        for point in [
                            center + Vec2::new(-7.0, -6.0),
                            center + Vec2::new(7.0, -6.0),
                            center + Vec2::new(0.0, 7.0),
                        ] {
                            painter.line_segment([center, point], stroke);
                            painter.rect_filled(
                                egui::Rect::from_center_size(point, Vec2::splat(5.0)),
                                1.0,
                                color,
                            );
                        }
                    } else if kind == 1 {
                        let r = egui::Rect::from_center_size(center, Vec2::new(17.0, 16.0));
                        painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
                        for y in [-3.0, 3.0] {
                            painter.line_segment(
                                [center + Vec2::new(-7.0, y), center + Vec2::new(7.0, y)],
                                stroke,
                            );
                        }
                        painter.line_segment(
                            [
                                center + Vec2::new(-2.0, -7.0),
                                center + Vec2::new(-2.0, 7.0),
                            ],
                            stroke,
                        );
                    } else {
                        for y in [-6.0, 0.0, 6.0] {
                            painter.circle_filled(center + Vec2::new(-7.0, y), 1.6, color);
                            painter.line_segment(
                                [center + Vec2::new(-2.0, y), center + Vec2::new(8.0, y)],
                                stroke,
                            );
                        }
                    }
                    if response.clicked() {
                        self.view = view;
                    }
                    response.on_hover_text(label);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(label)
                                .size(11.0)
                                .family(if selected {
                                    semibold_family()
                                } else {
                                    egui::FontFamily::Proportional
                                })
                                .color(if selected { ACCENT } else { MUTED }),
                        );
                    });
                    ui.add_space(12.0);
                }
            });
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.load(ctx, path);
        }
        egui::TopBottomPanel::top("brand")
            .exact_height(40.0)
            .frame(
                egui::Frame::NONE
                    .fill(TAB_BAR)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    woven_mark(ui, 23.0);
                    ui.label(
                        RichText::new("StructureWeaver")
                            .size(13.5)
                            .family(semibold_family()),
                    );
                    ui.add_space(11.0);
                    egui::Frame::NONE
                        .fill(PANEL)
                        .inner_margin(egui::Margin::symmetric(14, 6))
                        .corner_radius(5.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(
                                    self.document
                                        .as_ref()
                                        .map(|d| d.name.as_str())
                                        .unwrap_or("새 문서"),
                                )
                                .size(13.0),
                            );
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                self.result.is_some() && !self.busy,
                                egui::Button::new(
                                    RichText::new("내보내기  ↗")
                                        .color(GREEN_INK)
                                        .size(13.0)
                                        .family(semibold_family()),
                                )
                                .fill(BRAND)
                                .corner_radius(5.0)
                                .min_size(Vec2::new(98.0, 28.0)),
                            )
                            .on_hover_text("JSON 분석 결과, Mermaid ERD, SQL 초안을 저장합니다.")
                            .clicked()
                        {
                            self.export(ctx);
                        }
                        ui.add_space(7.0);
                        ui.label(RichText::new("로컬 워크스페이스").size(11.5).color(MUTED));
                    });
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(13, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    } else {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            2.5,
                            if self.error.is_some() { WARM } else { ACCENT },
                        );
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).size(11.5).color(MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&self.status);
                });
                if let Some(error) = &self.error {
                    egui::ScrollArea::vertical()
                        .id_salt("status-error")
                        .max_height(70.0)
                        .show(ui, |ui| {
                            ui.colored_label(Color32::from_rgb(183, 45, 28), error);
                        });
                }
            });
        let right_inspector = ctx.screen_rect().width() >= 1250.0
            && self.result.is_some()
            && self.view == View::Diagram;
        self.toolrail(ctx);
        self.sidebar(ctx);
        if right_inspector {
            egui::SidePanel::right("evidence-sidebar")
                .resizable(false)
                .exact_width(252.0)
                .frame(egui::Frame::NONE.fill(PANEL).inner_margin(15.0))
                .show(ctx, |ui| {
                    ui.add_space(3.0);
                    egui::ScrollArea::vertical()
                        .id_salt("evidence-scroll")
                        .show(ui, |ui| {
                            self.evidence(ui);
                        });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(14.0))
            .show(ctx, |ui| {
                self.overview(ui);
                if let Some(result) = &self.result {
                    let incomplete = result
                        .pages
                        .iter()
                        .filter(|p| !matches!(p.state.as_str(), "complete" | "empty"))
                        .count();
                    if incomplete > 0 {
                        egui::Frame::NONE
                            .fill(Color32::from_rgb(255, 246, 224))
                            .inner_margin(8.0)
                            .corner_radius(5.0)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "{incomplete}개 시트에 검토가 필요한 영역이 있습니다."
                                        ))
                                        .size(13.0)
                                        .color(WARM),
                                    );
                                    if ui.link("분석 범위 확인 →").clicked() {
                                        self.view = View::Report;
                                    }
                                });
                            });
                        ui.add_space(8.0);
                    }
                }
                match self.view {
                    View::Diagram => self.diagram(ui, !right_inspector),
                    View::Structure if self.document.is_some() => self.structure(ui),
                    View::Structure => self.empty(ui),
                    View::Report => self.report(ui),
                }
            });
    }
}

fn semibold_family() -> egui::FontFamily {
    egui::FontFamily::Name("Pretendard SemiBold".into())
}
fn semibold_font(size: f32) -> FontId {
    FontId::new(size, semibold_family())
}

fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(5.0)
        .inner_margin(12.0)
}
fn eyebrow(ui: &mut egui::Ui, label: &str, color: Color32) {
    ui.label(
        RichText::new(label)
            .size(11.5)
            .color(color)
            .family(semibold_family()),
    );
}
fn pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::NONE
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            18,
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(color));
        });
}
fn woven_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, size * 0.22, Color32::from_rgb(11, 16, 32));
    let area = |x: f32, y: f32, w: f32, h: f32| {
        egui::Rect::from_min_size(rect.min + Vec2::new(x, y) * size, Vec2::new(w, h) * size)
    };
    let mint = Color32::from_rgb(111, 228, 204);
    let violet = Color32::from_rgb(170, 164, 255);
    for x in [0.30, 0.57] {
        painter.rect_filled(area(x, 0.19, 0.10, 0.62), size * 0.045, mint);
    }
    for y in [0.32, 0.57] {
        painter.rect_filled(area(0.19, y, 0.62, 0.10), size * 0.045, violet);
    }
    painter.rect_filled(area(0.30, 0.27, 0.10, 0.20), 0.0, mint);
    painter.rect_filled(area(0.57, 0.52, 0.10, 0.20), 0.0, mint);
}

fn short_model(model: &str) -> String {
    let name = model.rsplit('/').next().unwrap_or(model);
    truncate(name, 25)
}
fn cardinality_label(value: &str) -> &str {
    match value {
        "many_to_one" => "N : 1",
        "one_to_many" => "1 : N",
        "one_to_one" => "1 : 1",
        "many_to_many" => "N : M",
        _ => value,
    }
}
fn state_label(value: &str) -> &str {
    match value {
        "complete" => "완료",
        "partial" => "검토 필요",
        "failed" => "실패",
        "empty" => "빈 시트",
        "unsupported" => "미지원",
        _ => value,
    }
}

fn column_name(mut column: u32) -> String {
    let mut letters = Vec::new();
    while column > 0 {
        column -= 1;
        letters.push((b'A' + (column % 26) as u8) as char);
        column /= 26;
    }
    letters.iter().rev().collect()
}
fn cell_name(row: u32, column: u32) -> String {
    format!("{}{row}", column_name(column))
}
fn range_name(bounds: Bounds) -> String {
    format!(
        "{}:{}",
        cell_name(bounds.top, bounds.left),
        cell_name(bounds.bottom, bounds.right)
    )
}
fn truncate(value: &str, len: usize) -> String {
    if value.chars().count() <= len {
        value.into()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(len.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn fit_label(painter: &egui::Painter, text: &str, font: FontId, max_width: f32) -> String {
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), TEXT)
        .size()
        .x
        <= max_width
    {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate = format!("{}…", chars.iter().collect::<String>());
        if painter
            .layout_no_wrap(candidate.clone(), font.clone(), TEXT)
            .size()
            .x
            <= max_width
        {
            return candidate;
        }
    }
    "…".into()
}
