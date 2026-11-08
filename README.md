# StructureWeaver

Excel의 모든 워크시트에서 표·행/열 헤더·값의 구조를 분석하고 ERD 초안을 만드는 **Rust 네이티브 데스크톱 MVP**입니다. 추론은 컴퓨터에 설치된 **Llama.app**의 로컬 OpenAI 호환 API를 사용합니다.

## 시작

macOS 로컬 빌드 결과는 `dist/StructureWeaver.app`입니다. 새 체크아웃에서는 먼저 `./scripts/package-macos.sh`로 생성합니다. Llama에서 이미지 입력이 가능한 모델과 로컬 API 서버를 준비한 뒤 앱을 실행합니다.

1. **모델 연결 / 새로고침**로 `http://127.0.0.1:9931/v1` 연결을 확인하고 모델을 선택합니다.
2. `.xlsx` 또는 `.xlsm` 파일을 열거나 창에 끌어 놓습니다. 숨김·빈 시트도 목록에 표시됩니다.
3. **전체 시트 분석하기**으로 전체 시트를 처리합니다. UI는 백그라운드 작업의 진행과 시트별 부분 실패를 보여줍니다.
4. ERD에서 엔티티를 선택해 원본 셀·헤더·관계 근거를 확인하고 결과 폴더로 내보냅니다.

앱은 로컬 주소만 허용하며 문서를 클라우드로 업로드하지 않습니다. 분석 중 취소는 진행 중인 HTTP 요청이 종료되거나 타임아웃된 뒤 적용됩니다.

## 개발 / CLI

Rust stable과 플랫폼 개발 도구(macOS Xcode Command Line Tools)가 필요합니다. 이 작업 환경에서는 `.tools`에 격리된 Rust를 설치했고 `scripts/cargo.sh`가 이를 우선 사용합니다. 일반 체크아웃에서는 설치된 `cargo`를 사용합니다.

```bash
./scripts/cargo.sh run --locked
./scripts/cargo.sh run --locked -- models
mkdir -p output
./scripts/cargo.sh run --locked -- inspect fixtures/coverage.xlsx --output output/document.json
./scripts/cargo.sh run --locked -- analyze fixtures/demo.xlsx --model 'YOUR_MODEL_ID' --output output/demo
./scripts/cargo.sh test --locked
./scripts/cargo.sh clippy --all-targets --locked -- -D warnings
./scripts/package-macos.sh
```

`inspect`는 모델 없이 파일만 읽습니다. `analyze`는 일부 시트가 미해결이면 결과를 저장하고 종료 코드 1을 반환합니다. 모델을 생략하면 모델 목록에서 로드된 모델을 우선 선택하므로 이미지 지원 여부를 확인하는 것이 좋습니다. 기본 요청 타임아웃은 180초입니다(`--timeout`). API 주소는 `--base-url`로 변경할 수 있습니다.

렌더링은 시스템 Unicode 폰트를 사용합니다. 필요하면 `STRUCTUREWEAVER_FONT=/path/to/font.ttf`를 지정합니다. 저장된 분석은 `STRUCTUREWEAVER_OPEN_ANALYSIS=/absolute/path/analysis.json ./target/release/structureweaver`로 다시 볼 수 있습니다.

## 구현 범위

- **Excel 파서:** 모든 워크시트와 숨김 영역, 원본 좌표, 병합 범위, 타입, 수식과 저장된 계산값, 굵기·들여쓰기·배경색 메타데이터를 보존합니다.
- **Luna 기반 분석:** 셀 타입을 색으로 표시한 이미지와 실제 셀 정보를 함께 전달해 표·제목·행 헤더·열 헤더·데이터 영역을 추론합니다. 원본 Luna의 영역 정규화와 실제 병합 범위 기반 헤더 보완을 적용한 뒤 범위·겹침·정렬·셀 누락을 검증하고 조각 분석의 잘못된 응답은 한 번 재시도합니다.
- **전체 범위 처리:** 32행 × 10열 단위로 실제 데이터가 있는 모든 조각을 분석합니다. 여러 조각의 표를 시트 전체 좌표로 다시 조정하고 기존 데이터가 유실되지 않는지 확인합니다. 원본 Luna의 400행 × 60열 제한은 사용하지 않습니다.
- **계층:** 병합 범위와 위치로 열/행 헤더 부모를 구성하고, 같은 열의 행 헤더 들여쓰기를 부모 후보로 해석합니다. 문서의 헤더 계층과 데이터베이스의 상속 관계는 별개의 의미입니다.
- **ERD:** 레코드 표의 계층 헤더를 필드로 펼치고, 교차표는 행 차원·열 차원·값으로 변환합니다. PK는 실제 값의 유일성/결측과 ID 이름을 확인하고, FK는 타입·이름·값 포함 관계를 함께 확인합니다.
- **출력:** `analysis.json`(원본 셀·근거·전체 분석), `schema.mmd`(Mermaid ERD), `schema.sql`(SQLite DDL 초안). FK는 검토할 후보이며 SQL에서는 주석으로 출력합니다.

## 정확도와 지원 한계

임의 문서에 대한 정확도를 보장하지 않습니다. 모든 셀을 읽었다는 사실과 모든 셀의 의미를 정확하게 파악했다는 사실을 구분합니다. 분류 영역 밖 셀, 실패 조각, 조각 통합 실패는 **partial/failed**로 표시하며 원본 데이터는 보존합니다. 모델의 구문상 유효한 오해는 사용자가 원본과 비교해야 합니다.

- 현재 입력은 `.xlsx`/`.xlsm`입니다. `.xls`, PPT/PPTX, PDF, 스캔/OCR은 아직 구현하지 않았습니다. Excel의 차트 시트는 지원되지 않는 페이지로 표시됩니다.
- 수식을 계산하지 않으며 파일에 저장된 캐시를 읽습니다. 캐시가 없거나 오래된 경우 원본 앱에서 재계산 후 저장해야 합니다. 매크로는 실행하지 않습니다.
- 렌더링은 구조 판별용이며 Excel의 원본 화면과 픽셀 단위로 같지 않습니다. 완전한 스타일·그림·차트 추출을 지원하지 않습니다.
- 복합 키, 자기참조 FK, 엔티티 상속, 차원 테이블 자동 분리는 후속 범위입니다. 관계 후보의 confidence는 규칙 기반 점수이며 통계적 확률이 아닙니다.
- 자원 한계는 시트 사용 범위의 면적 500만 셀, XML 파트 64 MiB, 압축 해제 총량 256 MiB입니다. 초과하면 명시적으로 실패합니다. LLM 조각/통합 문맥은 60 KB로 제한하며 초과 내용을 몰래 잘라내지 않습니다.
- 저장된 분석 JSON에는 실제 셀 값이 포함됩니다. 출력 폴더는 사용자가 선택합니다. 현재 내보내기는 같은 이름의 파일을 덮어씁니다.

## 확장 구조와 참고

`DocumentAdapter → Document/Page/SourceRegion → 분석 → Schema → UI/내보내기`로 분리했습니다. `PageKind::Slide`, `PageKind::PdfPage`, 페이지 좌표 `PageRect`, 텍스트 블록을 미리 정의했으며, PPT/PDF 입력은 새 어댑터와 페이지 분석기를 추가하는 방식입니다. [상세 구조](docs/architecture.md)를 참고하세요.

분석 흐름의 참고 소스는 [bist-mini-final의 Luna detector](https://github.com/bist-mini-final/bist-mini-final/blob/0e0d7638b96c9f9db816c3959cec6064a40b5045/modules/structure/luna_vlm_structure_detector.py)입니다. `Luna`는 모듈 이름이며 별도 모델 가중치를 의미하지 않습니다. Python 런타임 없이 Rust로 재구현하고, 모델 전송을 Llama의 `/v1/chat/completions`로 교체했습니다.

## 디자인 / 검증 자료

`design/`에는 DesignCraft CLI로 만든 화면 설계 원본과 FigCraft의 로컬 제어 프로토콜로 제작한 벡터 브랜드 아이콘이 있습니다. 화면 설계 문서는 편집 가능한 디자인 참고 자료이며, 실행되는 앱 화면은 `src/ui.rs`에서 구현합니다. 요청하신 [FigCraft의 UI 스타일](https://github.com/storytold/figcraft)을 참고해 흰색 패널·회색 캔버스·초록색 선택 상태·하단 툴바를 실제 Rust UI에 반영했습니다. 재생성 스크립트는 `scripts/design_*`에 있습니다. [실제 로컬 모델 검증 결과](docs/verification/README.md)와 합성 예제도 포함했습니다.
