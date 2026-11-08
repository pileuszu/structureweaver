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
- **원본 시트 보기:** 원본 글꼴·크기·굵기·색상·밑줄·취소선, 배경/패턴, 테두리, 병합, 정렬·줄바꿈·회전, 행 높이·열 너비, 숫자·날짜·통화·백분율 형식을 표시합니다. 격자선·틀 고정·숨김 설정을 읽고, 확대·양방향 스크롤·셀 선택·수식 확인을 제공합니다. 구조 분석 없이도 파일을 열어 **원본** 탭에서 볼 수 있습니다. 분석 영역 윤곽선은 선택해서 표시합니다.
- **조건부 서식:** 비교·수식·중복·상하위·평균·빈 셀·오류 규칙을 우선순위와 `stopIfTrue`에 맞춰 평가합니다. 차등 글꼴·배경·테두리·숫자 형식, 색조, 데이터 막대, 아이콘 집합을 표시하며 기본 셀 값과 서식은 보존합니다.
- **차트·도형:** 막대·선·영역·원형·도넛·산점도·버블·복합 차트와 누적 표시, 축·범례·레이블·테마 색상을 그립니다. 독립 차트 시트도 표시합니다. 이미지, 기본/사용자 지정 다각형, 연결선·화살표·텍스트 상자·그룹, 선형 그라데이션과 회전을 원본 앵커에 배치합니다. VML 버튼·텍스트 상자는 정적인 모양으로만 표시합니다.
- **3D·효과·배치:** 3D 막대·원통·원뿔·피라미드·선·영역·원형·표면 차트의 회전, 원근, 깊이를 읽습니다. 제목·범례·플롯의 저장된 배치를 반영하고, 자동 배치는 실제 글자 크기를 측정합니다. 도형과 그림의 외부/내부 그림자, 광선, 부드러운 가장자리, 반사, 베벨과 돌출을 표시합니다.
- **Luna 기반 분석:** 셀 타입을 색으로 표시한 이미지와 실제 셀 정보를 함께 전달해 표·제목·행 헤더·열 헤더·데이터 영역을 추론합니다. 원본 Luna의 영역 정규화와 실제 병합 범위 기반 헤더 보완을 적용한 뒤 범위·겹침·정렬·셀 누락을 검증하고 조각 분석의 잘못된 응답은 한 번 재시도합니다.
- **전체 범위 처리:** 32행 × 10열 단위로 실제 데이터가 있는 모든 조각을 분석합니다. 여러 조각의 표를 시트 전체 좌표로 다시 조정하고 기존 데이터가 유실되지 않는지 확인합니다. 원본 Luna의 400행 × 60열 제한은 사용하지 않습니다.
- **계층:** 병합 범위와 위치로 열/행 헤더 부모를 구성하고, 같은 열의 행 헤더 들여쓰기를 부모 후보로 해석합니다. 문서의 헤더 계층과 데이터베이스의 상속 관계는 별개의 의미입니다.
- **ERD:** 레코드 표의 계층 헤더를 필드로 펼치고, 교차표는 행 차원·열 차원·값으로 변환합니다. PK는 실제 값의 유일성/결측과 ID 이름을 확인하고, FK는 타입·이름·값 포함 관계를 함께 확인합니다.
- **출력:** `analysis.json`(원본 셀·근거·전체 분석), `schema.mmd`(Mermaid ERD), `schema.sql`(SQLite DDL 초안). FK는 검토할 후보이며 SQL에서는 주석으로 출력합니다.

## 정확도와 지원 한계

임의 문서에 대한 정확도를 보장하지 않습니다. 모든 셀을 읽었다는 사실과 모든 셀의 의미를 정확하게 파악했다는 사실을 구분합니다. 분류 영역 밖 셀, 실패 조각, 조각 통합 실패는 **partial/failed**로 표시하며 원본 데이터는 보존합니다. 모델의 구문상 유효한 오해는 사용자가 원본과 비교해야 합니다.

- 현재 입력은 `.xlsx`/`.xlsm`입니다. `.xls`, PPT/PPTX, PDF, 스캔/OCR은 아직 구현하지 않았습니다.
- 수식을 계산하지 않으며 파일에 저장된 캐시를 읽습니다. 캐시가 없거나 오래된 경우 원본 앱에서 재계산 후 저장해야 합니다. 매크로는 실행하지 않습니다.
- 분석에 보내는 이미지는 구조 판별용 타입 색상입니다. UI의 **원본** 화면은 저장된 셀 서식을 별도로 재현합니다. 설치되지 않은 원본 글꼴은 대체 글꼴을 쓰며 화면에 표시합니다.
- 조건부 서식의 교차 시트 참조·지원하지 않는 함수/확장 규칙은 경고합니다. 일반 수식 전체를 계산하는 Excel 엔진은 아닙니다. 차트는 저장된 캐시를 사용하고, 캐시가 없으면 내부 시트의 직접 범위를 읽습니다. 외부 통합 문서나 링크는 조회하지 않습니다.
- Rust 렌더러의 3D 조명·재질·일부 카메라/베벨 프리셋과 자동 배치는 Excel의 근사 구현입니다. 텍스트의 반사/3D, 2D 효과와 3D의 조합, 특수 차트·추세선·오차 막대·일부 자동 스타일·사용자 지정 경로·특수 VML 컨트롤에는 제한이 있습니다. Excel 표 자동 스타일, 셀 안의 혼합 글꼴, 셀 그라데이션, 일부 정렬/페이지 배치도 완전히 재현하지 않습니다. 감지한 미지원 항목을 화면에서 확인할 수 있습니다. 회계 서식의 폭에 따른 공백과 패턴 밀도는 근사하며, 날짜의 월·요일 이름은 영문입니다. Rust 자체 표시의 픽셀 단위 일치를 보장하지 않습니다.
- 복합 키, 자기참조 FK, 엔티티 상속, 차원 테이블 자동 분리는 후속 범위입니다. 관계 후보의 confidence는 규칙 기반 점수이며 통계적 확률이 아닙니다.
- 자원 한계는 시트 사용 범위의 면적 500만 셀, XML 파트 64 MiB, 압축 해제 총량 256 MiB입니다. 초과하면 명시적으로 실패합니다. LLM 조각/통합 문맥은 60 KB로 제한하며 초과 내용을 몰래 잘라내지 않습니다.
- 저장된 분석 JSON에는 실제 셀 값이 포함됩니다. 출력 폴더는 사용자가 선택합니다. 현재 내보내기는 같은 이름의 파일을 덮어씁니다.

## 확장 구조와 참고

`DocumentAdapter → Document/Page/SourceRegion → 분석 → Schema → UI/내보내기`로 분리했습니다. `PageKind::Slide`, `PageKind::PdfPage`, 페이지 좌표 `PageRect`, 텍스트 블록을 미리 정의했으며, PPT/PDF 입력은 새 어댑터와 페이지 분석기를 추가하는 방식입니다. [상세 구조](docs/architecture.md)를 참고하세요.

분석 흐름의 참고 소스는 [bist-mini-final의 Luna detector](https://github.com/bist-mini-final/bist-mini-final/blob/0e0d7638b96c9f9db816c3959cec6064a40b5045/modules/structure/luna_vlm_structure_detector.py)입니다. `Luna`는 모듈 이름이며 별도 모델 가중치를 의미하지 않습니다. Python 런타임 없이 Rust로 재구현하고, 모델 전송을 Llama의 `/v1/chat/completions`로 교체했습니다.

## 디자인 / 검증 자료

화면에는 Pretendard Regular·SemiBold를 내장해 한글·영문을 함께 표시합니다. 별도 폰트 설치가 필요 없으며, 글꼴 라이선스는 `assets/fonts/OFL.txt`와 앱 번들에 포함됩니다.

원본 시트에는 문서가 지정한 설치 글꼴을 우선 사용합니다. `fixtures/styled.xlsx`는 병합·색상·테두리·날짜·통화·틀 고정 등을 확인하는 예제입니다. 재생성은 `python3 scripts/make_style_fixture.py`로 하며, 이 개발용 스크립트에만 `openpyxl`이 필요합니다. 앱 실행은 Rust 바이너리만 사용합니다.

`fixtures/visuals.xlsx`에는 조건부 서식과 차트·도형·이미지, 독립 차트 시트 예제가 있습니다. `python3 scripts/make_visual_fixture.py`로 재생성하며, 개발용 `openpyxl`과 `Pillow`를 사용합니다. 원본 미리보기는 Excel 설치나 Python 실행 없이 Rust에서 동작합니다.

`fixtures/fidelity.xlsx`에는 6종의 3D 차트, 자동/수동 배치와 8종의 도형 효과가 있습니다. `python3 scripts/make_fidelity_fixture.py`로 재생성합니다. 테스트는 원본 값, 카메라 좌표, 배치, 효과와 저장된 분석의 복원을 함께 확인합니다.

CLI에서 원본 보기로 바로 열려면 `./scripts/cargo.sh run --release -- open fixtures/fidelity.xlsx`를 실행합니다. 모델 분석은 자동으로 시작하지 않습니다.

설치된 Excel의 차트·도형 PNG를 가져오는 경로는 `excel-render-experimental` 기능으로 분리했습니다. `./scripts/cargo.sh run --features excel-render-experimental -- open fixtures/fidelity.xlsx`로 시험할 수 있습니다. 현재 이 컴퓨터의 Excel은 통합 문서를 여는 Apple Event 요청에 `-1712` 시간 초과로 응답해 전체 동작 검증을 완료하지 못했습니다. 기본 릴리스에는 이 버튼이 표시되지 않습니다. 이 경로도 원본 셀은 Rust로 표시하며, PNG의 효과 여백과 3D 도형 위치까지 픽셀 단위 일치를 검증한 상태는 아닙니다.

`design/`에는 DesignCraft CLI로 만든 화면 설계 원본과 FigCraft의 로컬 제어 프로토콜로 제작한 벡터 브랜드 아이콘이 있습니다. 화면 설계 문서는 편집 가능한 디자인 참고 자료이며, 실행되는 앱 화면은 `src/ui.rs`에서 구현합니다. 요청하신 [FigCraft의 UI 스타일](https://github.com/storytold/figcraft)을 참고해 흰색 패널·회색 캔버스·초록색 선택 상태·하단 툴바를 실제 Rust UI에 반영했습니다. 재생성 스크립트는 `scripts/design_*`에 있습니다. [실제 로컬 모델 검증 결과](docs/verification/README.md)와 합성 예제도 포함했습니다.
