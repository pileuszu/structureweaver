//! Optional, on-demand rendering of drawing objects by an installed Microsoft Excel.
//!
//! Cell data and native rendering are never replaced. Excel receives a private,
//! read-only static snapshot and returns PNGs of its drawing layer. The original workbook
//! is not opened, saved, renamed, or modified by this backend.
#[cfg(any(target_os = "macos", test))]
use anyhow::{Context, ensure};
use anyhow::{Result, bail};
#[cfg(any(target_os = "macos", test))]
use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "macos", test))]
use sha2::{Digest, Sha256};
use std::path::Path;
#[cfg(any(target_os = "macos", test))]
use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
};

#[cfg(any(target_os = "macos", test))]
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_TOTAL_IMAGE_BYTES: usize = 128 * 1024 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_OBJECTS: usize = 2048;
#[cfg(any(target_os = "macos", test))]
const SCRIPT: &str = include_str!("excel_render.applescript");

#[cfg(target_os = "macos")]
struct TemporaryRun {
    directory: PathBuf,
    keep_for_recovery: bool,
}

#[cfg(target_os = "macos")]
impl Drop for TemporaryRun {
    fn drop(&mut self) {
        if !self.keep_for_recovery {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderedObject {
    pub sheet_name: String,
    pub object_name: String,
    pub kind: String,
    pub z_order: i32,
    /// Left, top, width and height in Excel worksheet points.
    pub rect_points: [f32; 4],
    /// Rotation is already baked into the PNG; do not rotate it a second time.
    pub source_rotation_degrees: f32,
    pub png_pixels: [u32; 2],
    /// From PNG pHYs, when Excel supplied physical resolution metadata.
    pub pixels_per_point: Option<[f32; 2]>,
    pub png: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RenderedWorkbook {
    pub document_id: String,
    pub excel_version: String,
    pub objects: Vec<RenderedObject>,
    pub warnings: Vec<String>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    document_id: String,
    excel_version: String,
    objects: Vec<ObjectRecord>,
    warnings: Vec<String>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ObjectRecord {
    sheet_name: String,
    object_name: String,
    kind: String,
    z_order: i32,
    rect_points: [f32; 4],
    #[serde(default)]
    source_rotation_degrees: f32,
    file: String,
}

pub fn available() -> bool {
    cfg!(target_os = "macos")
        && Path::new("/Applications/Microsoft Excel.app/Contents/Info.plist").is_file()
}

/// Called on a worker thread only. The first use may show macOS's Excel
/// automation permission prompt. We never accept that prompt automatically.
pub fn render(source: &Path, expected_document_id: &str) -> Result<RenderedWorkbook> {
    #[cfg(target_os = "macos")]
    {
        render_macos(source, expected_document_id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (source, expected_document_id);
        bail!("Excel 원본 개체 표시는 Microsoft Excel이 설치된 macOS에서 사용할 수 있습니다.")
    }
}

#[cfg(any(target_os = "macos", test))]
fn validate_source(source: &Path, expected_document_id: &str) -> Result<String> {
    ensure!(
        expected_document_id.len() == 64
            && expected_document_id.bytes().all(|v| v.is_ascii_hexdigit()),
        "Invalid source document identity"
    );
    let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("");
    ensure!(
        extension.eq_ignore_ascii_case("xlsx") || extension.eq_ignore_ascii_case("xlsm"),
        "Excel 원본 개체 표시는 .xlsx와 .xlsm 파일을 지원합니다."
    );
    let mut input = File::open(source).context("원본 Excel 파일을 열 수 없습니다.")?;
    ensure!(
        input.metadata()?.len() <= MAX_SOURCE_BYTES,
        "Workbook exceeds 256 MiB limit"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let identity = format!("{:x}", hash.finalize());
    ensure!(
        identity == expected_document_id.to_ascii_lowercase(),
        "원본 파일이 현재 분석 문서와 다릅니다. 파일을 다시 열어 주세요."
    );
    // XLM macro sheets and linked media are not representable by the static
    // snapshot. They stay in the native renderer, without opening them in Excel.
    let mut archive = zip::ZipArchive::new(File::open(source)?)?;
    ensure!(archive.len() <= 100_000, "Workbook contains too many parts");
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_ascii_lowercase();
        ensure!(
            !name.starts_with("xl/macrosheets/") && !name.starts_with("xl/dialogsheets/"),
            "Excel 4.0 매크로 시트가 있는 파일은 원본 개체 표시를 사용할 수 없습니다. 네이티브 보기는 계속 사용할 수 있습니다."
        );
        if name.ends_with(".rels") {
            ensure!(
                entry.size() <= 4 * 1024 * 1024,
                "Relationship part too large"
            );
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            let node = crate::ooxml::parse(&xml)?;
            for relation in node.children_named("Relationship") {
                if relation
                    .attr("TargetMode")
                    .is_some_and(|s| s.eq_ignore_ascii_case("External"))
                {
                    let kind = relation.attr("Type").unwrap_or("");
                    ensure!(
                        kind.ends_with("/hyperlink") || kind.ends_with("/externalLinkPath"),
                        "외부 미디어가 연결된 파일은 원본 개체 표시를 사용할 수 없습니다. 네이티브 보기는 계속 사용할 수 있습니다."
                    );
                }
            }
        }
    }
    Ok(identity)
}

#[cfg(any(target_os = "macos", test))]
fn active_part(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("vbaproject")
        || lower.contains("vbasignature")
        || lower.contains("/activex/")
        || lower.contains("/ctrlprops/")
        || lower.contains("/embeddings/")
        || lower.contains("/externallinks/")
        || lower.contains("/querytables/")
        || lower.contains("/webextensions/")
        || lower.starts_with("customui/")
        || lower == "xl/connections.xml"
        || lower == "xl/calcchain.xml"
        || lower.starts_with("_xmlsignatures/")
}

#[cfg(any(target_os = "macos", test))]
fn active_type(kind: &str) -> bool {
    let lower = kind.to_ascii_lowercase();
    [
        "vbaproject",
        "vbasignature",
        "activex",
        "oleobject",
        "connections",
        "querytable",
        "externallink",
        "webextension",
        "calcchain",
        "customui",
        "/ui/extensibility",
        "digital-signature",
    ]
    .iter()
    .any(|value| lower.contains(value))
}

#[cfg(any(target_os = "macos", test))]
fn relationship_owner(part: &str) -> String {
    if part == "_rels/.rels" {
        return String::new();
    }
    part.replace("/_rels/", "/")
        .trim_end_matches(".rels")
        .to_owned()
}

#[cfg(any(target_os = "macos", test))]
fn direct_chart_reference(formula: &str) -> bool {
    let formula = formula.trim().trim_start_matches('=');
    let Some((sheet, range)) = formula.rsplit_once('!') else {
        return false;
    };
    if sheet.is_empty() || sheet.contains(['[', ']', '(', ')', '+', '*', '&', '=', '{', '}', '\\'])
    {
        return false;
    }
    let parts: Vec<_> = range.split(':').collect();
    !parts.is_empty()
        && parts.len() <= 2
        && parts.iter().all(|cell| {
            let cell = cell.replace('$', "");
            let Some(split) = cell.find(|ch: char| ch.is_ascii_digit()) else {
                return false;
            };
            let (column, row) = cell.split_at(split);
            !column.is_empty()
                && column.len() <= 3
                && column.bytes().all(|b| b.is_ascii_alphabetic())
                && !row.is_empty()
                && row.bytes().all(|b| b.is_ascii_digit())
                && row.parse::<u32>().is_ok_and(|n| n > 0 && n <= 1_048_576)
        })
}

/// Builds a macro-free .xlsx containing saved values and drawing parts. Excel's
/// automation-security getter is advertised in Excel.sdef but times out on the
/// installed Mac build. We avoid that unsupported API and all application-wide
/// settings: untrusted active content never reaches Excel at all.
#[cfg(any(target_os = "macos", test))]
fn static_snapshot(source: &Path, destination: &Path, identity: &str) -> Result<Vec<String>> {
    use std::{
        collections::BTreeSet,
        io::{Cursor, Write},
    };
    let bytes = fs::read(source)?;
    ensure!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "Workbook exceeds 256 MiB limit"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == identity,
        "Source changed before rendering"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    ensure!(archive.len() <= 100_000, "Too many workbook parts");
    let mut removed = BTreeSet::new();
    let mut expanded = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name();
        ensure!(
            !name.starts_with('/') && !name.contains('\\') && !name.split('/').any(|p| p == ".."),
            "Invalid workbook part path"
        );
        expanded = expanded
            .checked_add(entry.size())
            .context("Workbook size overflow")?;
        ensure!(
            expanded <= 256 * 1024 * 1024,
            "Expanded workbook exceeds 256 MiB"
        );
        if active_part(name) {
            removed.insert(name.to_owned());
        }
    }
    // Content types and relationship types also identify active parts whose
    // package names do not use Office's conventional directory names.
    if let Ok(mut entry) = archive.by_name("[Content_Types].xml") {
        ensure!(entry.size() <= 4 * 1024 * 1024, "Content types too large");
        let mut text = String::new();
        entry.read_to_string(&mut text)?;
        let root = crate::ooxml::parse(&text)?;
        for item in &root.children {
            if active_type(item.attr("ContentType").unwrap_or(""))
                && let Some(name) = item.attr("PartName")
            {
                removed.insert(name.trim_start_matches('/').into());
            }
        }
    }
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if !entry.name().ends_with(".rels") {
            continue;
        }
        ensure!(entry.size() <= 4 * 1024 * 1024, "Relationships too large");
        let owner = relationship_owner(entry.name());
        let mut xml = String::new();
        entry.read_to_string(&mut xml)?;
        let root = crate::ooxml::parse(&xml)?;
        for item in root.children_named("Relationship") {
            if active_type(item.attr("Type").unwrap_or(""))
                && item.attr("TargetMode") != Some("External")
                && let Some(target) = item.attr("Target")
            {
                removed.insert(crate::ooxml::resolve_target(&owner, target)?);
            }
        }
    }
    let mut output = zip::ZipWriter::new(File::create(destination)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut removed_chart_refs = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_owned();
        if entry.is_dir()
            || removed.contains(&name)
            || (name.ends_with(".rels") && removed.contains(&relationship_owner(&name)))
        {
            continue;
        }
        ensure!(
            entry.size() <= 64 * 1024 * 1024,
            "Workbook part exceeds 64 MiB"
        );
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let lower = name.to_ascii_lowercase();
        let data = if lower.ends_with(".xml") || lower.ends_with(".rels") || lower.ends_with(".vml")
        {
            rewrite_snapshot_xml(&name, &data, &removed, &mut removed_chart_refs)?
        } else {
            data
        };
        output.start_file(name, options)?;
        output.write_all(&data)?;
    }
    output.finish()?;
    let mut warnings = Vec::new();
    if removed_chart_refs > 0 {
        warnings.push(format!(
            "외부·계산식 차트 참조 {removed_chart_refs}개는 파일에 저장된 차트 값으로 표시합니다."
        ));
    }
    Ok(warnings)
}

#[cfg(any(target_os = "macos", test))]
fn rewrite_snapshot_xml(
    part: &str,
    xml: &[u8],
    removed: &std::collections::BTreeSet<String>,
    removed_chart_refs: &mut usize,
) -> Result<Vec<u8>> {
    use quick_xml::{
        Reader, Writer,
        events::{BytesStart, BytesText, Event},
    };
    let mut reader = Reader::from_reader(xml);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let is_chart = part.starts_with("xl/charts/");
    let owner = relationship_owner(part);
    let mut skip_depth = 0usize;
    loop {
        let event = reader.read_event()?;
        if matches!(event, Event::Eof) {
            break;
        }
        ensure!(
            !matches!(event, Event::DocType(_)),
            "Document types are not allowed in workbook XML"
        );
        if skip_depth > 0 {
            match event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth -= 1,
                _ => {}
            }
            continue;
        }
        if let Event::Start(ref element) | Event::Empty(ref element) = event {
            let local = String::from_utf8_lossy(element.local_name().as_ref()).into_owned();
            let mut attrs = Vec::new();
            for attribute in element.attributes() {
                let attribute = attribute?;
                attrs.push((
                    String::from_utf8_lossy(attribute.key.as_ref()).into_owned(),
                    attribute
                        .decode_and_unescape_value(reader.decoder())?
                        .into_owned(),
                ));
            }
            let value = |key: &str| {
                attrs
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("")
            };
            let mut drop_element = matches!(
                local.as_str(),
                "definedNames"
                    | "externalReferences"
                    | "oleObjects"
                    | "controls"
                    | "dataValidations"
                    | "conditionalFormatting"
                    | "hyperlinks"
                    | "hlinkClick"
                    | "hlinkHover"
                    | "FmlaMacro"
                    | "FmlaLink"
                    | "FmlaRange"
                    | "calculatedColumnFormula"
                    | "totalsRowFormula"
                    | "DDELink"
                    | "ddeLink"
                    | "calcPr"
            );
            if local == "Relationship" {
                drop_element |= value("TargetMode").eq_ignore_ascii_case("External")
                    || active_type(value("Type"));
                if !drop_element {
                    drop_element |=
                        removed.contains(&crate::ooxml::resolve_target(&owner, value("Target"))?);
                }
            }
            if part == "[Content_Types].xml" && (local == "Override" || local == "Default") {
                drop_element |= removed.contains(value("PartName").trim_start_matches('/'))
                    || active_type(value("ContentType"));
            }
            if local == "f" || local == "formula" {
                if is_chart && local == "f" && matches!(event, Event::Start(_)) {
                    let formula = reader.read_text(element.name())?;
                    let formula = quick_xml::escape::unescape(&formula)?;
                    if direct_chart_reference(&formula) {
                        writer.write_event(Event::Start(element.clone()))?;
                        writer.write_event(Event::Text(BytesText::new(&formula)))?;
                        writer.write_event(Event::End(element.to_end()))?;
                    } else {
                        *removed_chart_refs += 1;
                    }
                    continue;
                }
                drop_element = true;
            }
            if drop_element {
                if matches!(event, Event::Start(_)) {
                    skip_depth = 1;
                }
                continue;
            }
            let mut changed = false;
            attrs.retain(|(key, _)| {
                let keep = !matches!(
                    key.as_str(),
                    "macro" | "onAction" | "onLoad" | "refreshOnLoad"
                );
                changed |= !keep;
                keep
            });
            if part == "[Content_Types].xml" && local == "Override" {
                for (key, value) in &mut attrs {
                    if key == "ContentType" && value.contains("macroEnabled.main") {
                        *value = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml".into();
                        changed = true;
                    }
                }
            }
            if changed {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                let mut replacement = BytesStart::new(name);
                for (key, value) in &attrs {
                    replacement.push_attribute((key.as_str(), value.as_str()));
                }
                writer.write_event(if matches!(event, Event::Empty(_)) {
                    Event::Empty(replacement)
                } else {
                    Event::Start(replacement)
                })?;
                continue;
            }
        }
        writer.write_event(event)?;
    }
    ensure!(skip_depth == 0, "Truncated workbook XML");
    Ok(writer.into_inner())
}

#[cfg(any(target_os = "macos", test))]
fn load_manifest(directory: &Path, manifest: Manifest, identity: &str) -> Result<RenderedWorkbook> {
    ensure!(
        manifest.document_id == identity,
        "Excel snapshot belongs to a different workbook"
    );
    ensure!(
        manifest.objects.len() <= MAX_OBJECTS,
        "Too many Excel drawing objects"
    );
    let mut total = 0usize;
    let mut total_decoded = 0usize;
    let mut objects = Vec::with_capacity(manifest.objects.len());
    for record in manifest.objects {
        let leaf = Path::new(&record.file);
        ensure!(
            leaf.components().count() == 1
                && record.file.ends_with(".png")
                && record
                    .file
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'),
            "Invalid snapshot image path"
        );
        ensure!(
            record.rect_points.iter().all(|v| v.is_finite())
                && record.source_rotation_degrees.is_finite()
                && record.rect_points[2] > 0.0
                && record.rect_points[3] > 0.0
                && record.rect_points.iter().all(|v| v.abs() <= 1e8),
            "Invalid Excel drawing geometry"
        );
        let path = directory.join(leaf);
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Snapshot is not a regular file"
        );
        ensure!(
            metadata.len() <= MAX_IMAGE_BYTES,
            "Excel drawing image exceeds 32 MiB"
        );
        let png = fs::read(path)?;
        total = total
            .checked_add(png.len())
            .context("Image size overflow")?;
        ensure!(
            total <= MAX_TOTAL_IMAGE_BYTES,
            "Excel drawing images exceed 128 MiB"
        );
        let mut reader =
            image::ImageReader::with_format(std::io::Cursor::new(&png), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode().context("Invalid Excel snapshot PNG")?;
        let rgba_bytes = (decoded.width() as usize)
            .checked_mul(decoded.height() as usize)
            .and_then(|n| n.checked_mul(4))
            .context("Decoded image size overflow")?;
        total_decoded = total_decoded
            .checked_add(rgba_bytes)
            .context("Decoded image size overflow")?;
        ensure!(
            total_decoded <= MAX_TOTAL_IMAGE_BYTES,
            "Decoded Excel drawing images exceed 128 MiB"
        );
        let png_pixels = [decoded.width(), decoded.height()];
        let pixels_per_point = png_resolution(&png);
        objects.push(RenderedObject {
            sheet_name: record.sheet_name,
            object_name: record.object_name,
            kind: record.kind,
            z_order: record.z_order,
            rect_points: record.rect_points,
            source_rotation_degrees: record.source_rotation_degrees,
            png_pixels,
            pixels_per_point,
            png,
        });
    }
    Ok(RenderedWorkbook {
        document_id: identity.into(),
        excel_version: manifest.excel_version,
        objects,
        warnings: manifest.warnings,
    })
}

#[cfg(any(target_os = "macos", test))]
fn png_resolution(bytes: &[u8]) -> Option<[f32; 2]> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut offset = 8usize;
    while offset.checked_add(12)? <= bytes.len() {
        let length = u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
        let end = offset.checked_add(12)?.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        if &bytes[offset + 4..offset + 8] == b"pHYs" && length == 9 && bytes[offset + 16] == 1 {
            let x = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().ok()?);
            let y = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().ok()?);
            if x == 0 || y == 0 {
                return None;
            }
            return Some([x as f32 * 0.0254 / 72.0, y as f32 * 0.0254 / 72.0]);
        }
        offset = end;
    }
    None
}

#[cfg(target_os = "macos")]
fn render_macos(source: &Path, expected_document_id: &str) -> Result<RenderedWorkbook> {
    use std::{
        os::unix::fs::DirBuilderExt,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    static RENDER_LOCK: Mutex<()> = Mutex::new(());
    let _guard = RENDER_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Excel renderer lock unavailable"))?;
    ensure!(available(), "Microsoft Excel이 설치되어 있지 않습니다.");
    let identity = validate_source(source, expected_document_id)?;
    // Include the installed Excel build so an Office update cannot reuse stale
    // layout results. All paths under this directory are generated, never names
    // from an untrusted document.
    let excel_info = fs::read("/Applications/Microsoft Excel.app/Contents/Info.plist")?;
    let build_hash = format!("{:x}", Sha256::digest(excel_info));
    let cache_root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("Cannot locate user cache directory")?
        .join("Library/Caches/dev.structureweaver/excel-objects-v1")
        .join(format!("{}-{}", identity, &build_hash[..16]));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&cache_root)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let run = cache_root.join(format!("run-{}-{nonce}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&run)?;
    let mut temporary_run = TemporaryRun {
        directory: run.clone(),
        keep_for_recovery: false,
    };
    let copy = run.join(format!(
        "StructureWeaver-{}-{nonce}.xlsx",
        std::process::id()
    ));
    let snapshot_warnings = static_snapshot(source, &copy, &identity)?;
    let script = run.join("render.applescript");
    fs::write(&script, SCRIPT)?;
    let result = run_script(
        &script,
        &["render".as_ref(), copy.as_os_str(), run.as_os_str()],
        &run,
        180,
    );
    // The script records only its own temporary workbook for timeout cleanup.
    // It never reads or changes application-wide Excel settings.
    if run.join("recovery.json").exists() {
        let recovery = run_script(
            &script,
            &["recover".as_ref(), copy.as_os_str(), run.as_os_str()],
            &run,
            45,
        );
        if let Err(error) = recovery {
            // Keep the recovery instructions/copy for a subsequent retry; never
            // kill Excel itself or close unrelated workbooks to make progress.
            temporary_run.keep_for_recovery = true;
            bail!(
                "Excel 응답이 중단되어 임시 문서를 닫지 못했습니다: {error}. 복구 파일: {}",
                run.display()
            );
        }
    }
    let outcome = (|| {
        let bytes = result?;
        let mut manifest: Manifest = serde_json::from_slice(&bytes)
            .context("Excel did not return a valid drawing snapshot")?;
        manifest.document_id = identity.clone();
        manifest.warnings.extend(snapshot_warnings);
        let rendered = load_manifest(&run, manifest, &identity)?;
        Ok(rendered)
    })();
    // The UI owns the in-memory images. Every explicit refresh obtains a fresh
    // Excel result, and no workbook/image cache persists after completion.
    let _ = fs::remove_dir_all(&run);
    outcome
}

#[cfg(target_os = "macos")]
fn run_script(
    script: &Path,
    args: &[&std::ffi::OsStr],
    directory: &Path,
    seconds: u64,
) -> Result<Vec<u8>> {
    use std::{
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    // Files avoid a pipe deadlock when Excel returns many records. A bounded
    // reader below prevents unbounded response memory use.
    let out = directory.join("stdout.txt");
    let err = directory.join("stderr.txt");
    let mut child = Command::new("/usr/bin/osascript")
        .arg(script)
        .args(args)
        .stdin(Stdio::null())
        .stdout(File::create(&out)?)
        .stderr(File::create(&err)?)
        .spawn()
        .context("Cannot start Excel rendering bridge")?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Excel 개체 렌더링 제한 시간을 초과했습니다.");
        }
        thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut message = String::new();
        File::open(err)?
            .take(16 * 1024)
            .read_to_string(&mut message)?;
        if message.contains("-1743") {
            bail!(
                "Excel 자동화 권한이 허용되지 않았습니다. macOS 시스템 설정의 개인정보 보호 및 보안 → 자동화에서 StructureWeaver의 Microsoft Excel 접근을 허용해 주세요."
            );
        }
        bail!("Excel 개체 렌더링 실패: {}", message.trim());
    }
    ensure!(
        fs::metadata(&out)?.len() <= 8 * 1024 * 1024,
        "Excel response exceeds 8 MiB"
    );
    fs::read(out).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn source(parts: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf, String) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("a \"quoted\" workbook.xlsm");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in parts {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let hash = format!("{:x}", Sha256::digest(fs::read(&path).unwrap()));
        (directory, path, hash)
    }

    #[test]
    fn source_identity_and_active_content_are_checked_before_excel() {
        let (_temp, path, hash) = source(&[("xl/workbook.xml", "<workbook/>")]);
        assert_eq!(validate_source(&path, &hash).unwrap(), hash);
        assert!(validate_source(&path, &"0".repeat(64)).is_err());
        for part in ["xl/macrosheets/sheet1.xml", "xl/dialogsheets/sheet1.xml"] {
            let (_temp, path, hash) = source(&[(part, "<data/>")]);
            assert!(validate_source(&path, &hash).is_err());
        }
        let (_temp, path, hash) = source(&[(
            "xl/_rels/workbook.xml.rels",
            "<Relationships><Relationship Type='image' TargetMode='External' Target='https://example.test/a.png'/></Relationships>",
        )]);
        assert!(validate_source(&path, &hash).is_err());
    }

    #[test]
    fn snapshots_validate_paths_identity_and_image_content() {
        let directory = tempfile::tempdir().unwrap();
        let png = image::RgbaImage::from_pixel(3, 2, image::Rgba([40, 80, 120, 255]));
        png.save(directory.path().join("object-1.png")).unwrap();
        let json = r#"{"document_id":"abc","excel_version":"16","objects":[{"sheet_name":"한글 \\\" 시트","object_name":"Chart 1","kind":"chart","z_order":2,"rect_points":[12,24,200,100],"file":"object-1.png"}],"warnings":[]}"#;
        let result =
            load_manifest(directory.path(), serde_json::from_str(json).unwrap(), "abc").unwrap();
        assert_eq!(result.objects[0].rect_points, [12., 24., 200., 100.]);
        assert!(!result.objects[0].png.is_empty());
        assert!(
            load_manifest(directory.path(), serde_json::from_str(json).unwrap(), "def").is_err()
        );
        let bad = json.replace("object-1.png", "../object-1.png");
        assert!(
            load_manifest(directory.path(), serde_json::from_str(&bad).unwrap(), "abc").is_err()
        );
        fs::write(directory.path().join("object-1.png"), b"invalid").unwrap();
        assert!(
            load_manifest(directory.path(), serde_json::from_str(json).unwrap(), "abc").is_err()
        );
    }

    #[test]
    fn script_uses_arguments_and_never_changes_global_excel_settings() {
        assert!(SCRIPT.contains("item 2 of argv"));
        assert!(SCRIPT.contains("read only true"));
        assert!(SCRIPT.contains("update links do not update links"));
        assert!(!SCRIPT.contains("automation security"));
        assert!(!SCRIPT.contains("set enable events"));
        assert!(!SCRIPT.contains("set calculation"));
        assert!(SCRIPT.contains("my recoverState(outputFolder)"));
        assert!(!SCRIPT.contains("do shell script"));
        assert!(!SCRIPT.contains("run VB macro"));
        assert!(!SCRIPT.contains("saving yes"));
        assert!(!SCRIPT.contains("set records to"));
    }

    #[test]
    fn static_snapshot_removes_code_links_and_formulas_but_preserves_values_and_chart_style() {
        let (temp, path, hash) = source(&[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.ms-excel.sheet.macroEnabled.main+xml"/><Override PartName="/xl/unusual.bin" ContentType="application/vnd.ms-office.vbaProject"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><definedNames><definedName name="Auto_Open">Sheet1!A1</definedName></definedNames><externalReferences><externalReference/></externalReferences><calcPr calcMode="auto" fullCalcOnLoad="1"/></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rCode" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="unusual.bin"/><Relationship Id="rSheet" Type="worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<s:worksheet xmlns:s="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><s:sheetData><s:row r="1"><s:c r="A1" s="3"><s:f>WEBSERVICE("https://example.test")</s:f><s:v>42</s:v></s:c></s:row></s:sheetData><s:conditionalFormatting><s:cfRule><s:formula>A1&gt;4</s:formula></s:cfRule></s:conditionalFormatting><s:oleObjects><s:oleObject/></s:oleObjects></s:worksheet>"#,
            ),
            (
                "xl/charts/chart1.xml",
                r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:view3D><c:rotX val="30"/></c:view3D><c:f>'Sheet1'!$A$1:$A$3</c:f><c:f>'[external.xlsx]Sheet1'!A1</c:f><c:numCache><c:pt idx="0"><c:v>42</c:v></c:pt></c:numCache></c:chart></c:chartSpace>"#,
            ),
            ("xl/unusual.bin", "VBA-CODE"),
            ("xl/activeX/activeX1.bin", "ACTIVE-X"),
            ("xl/connections.xml", "<connections/>"),
            ("xl/queryTables/query1.xml", "<queryTable/>"),
            ("xl/externalLinks/externalLink1.xml", "<externalLink/>"),
            ("xl/media/image1.png", "image-unchanged"),
        ]);
        let snapshot = temp.path().join("static.xlsx");
        assert_eq!(static_snapshot(&path, &snapshot, &hash).unwrap().len(), 1);
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
            hash
        );
        let mut archive = zip::ZipArchive::new(File::open(snapshot).unwrap()).unwrap();
        assert!(archive.by_name("xl/unusual.bin").is_err());
        assert!(archive.by_name("xl/connections.xml").is_err());
        assert!(archive.by_name("xl/activeX/activeX1.bin").is_err());
        let mut read = |part: &str| {
            let mut value = String::new();
            archive
                .by_name(part)
                .unwrap()
                .read_to_string(&mut value)
                .unwrap();
            value
        };
        let types = read("[Content_Types].xml");
        assert!(!types.contains("macroEnabled"));
        assert!(!types.contains("vbaProject"));
        assert!(types.contains("spreadsheetml.sheet.main+xml"));
        let workbook = read("xl/workbook.xml");
        assert!(!workbook.contains("Auto_Open"));
        assert!(!workbook.contains("calcPr"));
        let sheet = read("xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<s:v>42</s:v>"));
        assert!(sheet.contains("s=\"3\""));
        assert!(!sheet.contains("WEBSERVICE"));
        assert!(!sheet.contains("conditionalFormatting"));
        assert!(!sheet.contains("oleObjects"));
        let chart = read("xl/charts/chart1.xml");
        assert!(chart.contains("<c:rotX val=\"30\"/>"));
        assert!(chart.contains("$A$1:$A$3"));
        assert!(!chart.contains("external.xlsx"));
        assert!(chart.contains("<c:v>42</c:v>"));
        assert_eq!(read("xl/media/image1.png"), "image-unchanged");
    }

    #[test]
    fn static_snapshot_retains_existing_visual_fixture_structure() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/visuals.xlsx");
        let original = crate::ingest::read_document(&path).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let snapshot = temp.path().join("static.xlsx");
        static_snapshot(&path, &snapshot, &original.id).unwrap();
        let rendered = crate::ingest::read_document(&snapshot).unwrap();
        assert_eq!(original.pages.len(), rendered.pages.len());
        for (a, b) in original.pages.iter().zip(&rendered.pages) {
            assert_eq!(a.name, b.name);
            if let (Some(a), Some(b)) = (&a.grid, &b.grid) {
                assert_eq!(a.layout.drawings.len(), b.layout.drawings.len());
                assert_eq!(a.cells.len(), b.cells.len());
                for (a, b) in a.cells.iter().zip(&b.cells) {
                    assert_eq!(a.value, b.value);
                    assert!(b.formula.is_none());
                    assert_eq!(
                        serde_json::to_value(&a.style).unwrap(),
                        serde_json::to_value(&b.style).unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn png_physical_resolution_is_optional_and_bounded() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(b"pHYs");
        bytes.extend_from_slice(&5669u32.to_be_bytes());
        bytes.extend_from_slice(&5669u32.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let ppi = png_resolution(&bytes).unwrap();
        assert!((ppi[0] - 2.0).abs() < 0.001);
        assert!(png_resolution(&bytes[..20]).is_none());
        bytes[24] = 0;
        assert!(png_resolution(&bytes).is_none());
    }
}
