#!/usr/bin/env python3
"""Generate deterministic, synthetic OOXML test/demo workbooks; no external packages."""
from pathlib import Path
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
from xml.sax.saxutils import escape, quoteattr

ROOT = Path(__file__).resolve().parents[1] / "fixtures"
NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
STYLES = f'''<styleSheet xmlns="{NS}">
<numFmts count="0"/><fonts count="2"><font><sz val="11"/><name val="Arial"/></font><font><b/><sz val="11"/><name val="Arial"/></font></fonts>
<fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFDCEAFE"/><bgColor indexed="64"/></patternFill></fill></fills>
<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="4"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"><alignment indent="2"/></xf><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>'''

def cell(ref, value=None, style=0, formula=None, kind=None):
    a = f'r="{ref}" s="{style}"'
    if formula is not None:
        cached = '' if value is None else f'<v>{escape(str(value))}</v>'
        return f'<c {a}><f>{escape(formula)}</f>{cached}</c>'
    if kind:
        return f'<c {a} t="{kind}"><v>{escape(str(value))}</v></c>'
    if value is None:
        return f'<c {a}/>'
    if isinstance(value, bool):
        return f'<c {a} t="b"><v>{int(value)}</v></c>'
    if isinstance(value, (int, float)):
        return f'<c {a}><v>{value}</v></c>'
    return f'<c {a} t="inlineStr"><is><t xml:space="preserve">{escape(value)}</t></is></c>'

def sheet(rows, merges=(), hidden_rows=(), hidden_cols=()):
    cols = ''.join(f'<col min="{a}" max="{b}" hidden="1"/>' for a, b in hidden_cols)
    data = ''.join(f'<row r="{r}"' + (' hidden="1"' if r in hidden_rows else '') + '>' + ''.join(cells) + '</row>' for r, cells in rows)
    merge_xml = ''.join(f'<mergeCell ref="{m}"/>' for m in merges)
    return f'<worksheet xmlns="{NS}"><cols>{cols}</cols><sheetData>{data}</sheetData><mergeCells count="{len(merges)}">{merge_xml}</mergeCells></worksheet>'

def records(headers, rows):
    def col(i):
        result = ''
        while i:
            i, rem = divmod(i-1,26)
            result = chr(65+rem)+result
        return result
    return sheet([(1, [cell(f'{col(i)}1', h, style=1) for i, h in enumerate(headers,1)])] + [(r, [cell(f'{col(i)}{r}', v) for i, v in enumerate(values,1)]) for r, values in enumerate(rows,2)])

def write(name, sheets, shared_strings=False):
    files = {
        '_rels/.rels': f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
        'xl/workbook.xml': f'<workbook xmlns="{NS}" xmlns:r="{REL}"><bookViews><workbookView/></bookViews><sheets>'+''.join(f'<sheet name={quoteattr(title)} sheetId="{i}" state="{state}" r:id="rId{i}"/>' for i,(title,state,_) in enumerate(sheets,1))+'</sheets><calcPr calcId="0" fullCalcOnLoad="1"/></workbook>',
        'xl/_rels/workbook.xml.rels': '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'+''.join(f'<Relationship Id="rId{i}" Type="{REL}/worksheet" Target="worksheets/sheet{i}.xml"/>' for i in range(1,len(sheets)+1))+f'<Relationship Id="styles" Type="{REL}/styles" Target="styles.xml"/>'+(f'<Relationship Id="strings" Type="{REL}/sharedStrings" Target="sharedStrings.xml"/>' if shared_strings else '')+'</Relationships>',
        'xl/styles.xml': STYLES,
    }
    overrides = '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
    for i,(_,_,xml) in enumerate(sheets,1):
        files[f'xl/worksheets/sheet{i}.xml'] = xml
        overrides += f'<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    if shared_strings:
        files['xl/sharedStrings.xml'] = f'<sst xmlns="{NS}" count="1" uniqueCount="1"><si><t>서울 &amp; 부산</t></si></sst>'
        overrides += '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'
    files['[Content_Types].xml'] = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'+overrides+'</Types>'
    ROOT.mkdir(exist_ok=True)
    with ZipFile(ROOT/name,'w') as z:
        for path,text in files.items():
            info=ZipInfo(path, (2026,1,1,0,0,0)); info.compress_type=ZIP_DEFLATED
            z.writestr(info, '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'+text)

customers = ('Customers','visible', records(['customer_id','customer_name','country'],[(101,'Alice','KR'),(102,'Bob','US'),(103,'Carol','KR')]))
orders = ('Orders','visible', records(['order_id','customer_id','amount'],[(1001,101,120),(1002,101,75),(1003,102,260),(1004,103,50)]))
items = ('LineItems','visible', records(['line_id','order_id','product_name','quantity'],[(1,1001,'Keyboard',1),(2,1001,'Mouse',2),(3,1002,'Monitor',1),(4,1003,'Cable',3),(5,1004,'Stand',1)]))
write('demo.xlsx',[customers,orders,items])
matrix = ('Revenue Matrix','visible', sheet([
    (1,[cell('A1','Revenue by category',1)]),
    (2,[cell('B2','FY2026',1)]),
    (3,[cell('A3','Category',1),cell('B3','Q1',1),cell('C3','Q2',1)]),
    (4,[cell('A4','Hardware',1),cell('B4',100),cell('C4',150)]),
    (5,[cell('A5','Accessories',2),cell('B5',20),cell('C5',30)]),
    (6,[cell('A6','Software',1),cell('B6',300),cell('C6',350)]),
], ['A1:C1','B2:C2']))
audit = ('Hidden Audit','veryHidden', sheet([
    (1,[cell('A1','audit_id',1),cell('B1','status',1)]),
    (2,[cell('A2',1),cell('B2',True)]),
    (449,[cell('BM449','late_id',1),cell('BN449','late_value',1)]),
    (450,[cell('BM450',77),cell('BN450','covered after old limits')]),
],hidden_rows=[450],hidden_cols=[(65,66)]))
semantics = ('Cell Types','visible',sheet([
    (1,[cell('A1','label',1),cell('B1','value',1)]),
    (2,[cell('A2','cached formula'),cell('B2',3,formula='1+2')]),
    (3,[cell('A3','unresolved formula'),cell('B3',formula='RAND()')]),
    (4,[cell('A4','date'),cell('B4',45292,style=3)]),
    (5,[cell('A5','shared string'),cell('B5',0,kind='s')]),
    (6,[cell('A6','error'),cell('B6','#DIV/0!',kind='e')]),
    (7,[cell('A7','boolean'),cell('B7',False)]),
    (8,[cell('A8','zero'),cell('B8',0)]),
    (9,[cell('A9','merge ending outside populated range'),cell('D9','wide merged')]),
],['D9:G12']))
write('coverage.xlsx',[customers,orders,matrix,audit,semantics,('Empty','hidden',sheet([]))],shared_strings=True)
write('resource-limit.xlsx',[('Too Large','visible',sheet([(1,[cell('A1','first')]),(1048576,[cell('XFD1048576','last')])]))])
wide_headers = ['customer_id', 'customer_name', 'country', 'city', 'segment', 'active',
                'orders_count', 'revenue', 'discount', 'currency', 'account_owner', 'postal_code']
wide_rows = [
    (101, 'Alice', 'KR', 'Seoul', 'Business', True, 4, 1200, 0.05, 'KRW', 'Min', '04524'),
    (102, 'Bob', 'US', 'Boston', 'Retail', True, 2, 800, 0.00, 'USD', 'Sam', '02108'),
    (103, 'Carol', 'KR', 'Busan', 'Business', False, 1, 450, 0.10, 'KRW', 'Min', '48942'),
]
write('wide.xlsx', [('Wide Customers', 'visible', records(wide_headers, wide_rows))])

hierarchical_matrix = sheet([
    (1, [cell('C1', 'FY2026', 1)]),
    (2, [cell('C2', 'H1', 1), cell('E2', 'H2', 1)]),
    (3, [cell('C3', 'Q1', 1), cell('D3', 'Q2', 1), cell('E3', 'Q3', 1), cell('F3', 'Q4', 1)]),
    (4, [cell('A4', 'Hardware', 1), cell('B4', 'Laptops', 2), cell('C4', 100), cell('D4', 120), cell('E4', 140), cell('F4', 160)]),
    (5, [cell('B5', 'Accessories', 2), cell('C5', 20), cell('D5', 25), cell('E5', 30), cell('F5', 35)]),
    (6, [cell('A6', 'Software', 1), cell('B6', 'Licenses', 2), cell('C6', 200), cell('D6', 220), cell('E6', 240), cell('F6', 260)]),
    (7, [cell('B7', 'Support', 2), cell('C7', 50), cell('D7', 55), cell('E7', 60), cell('F7', 65)]),
], ['C1:F1', 'C2:D2', 'E2:F2', 'A4:A5', 'A6:A7'])
write('matrix.xlsx', [('Revenue Hierarchy', 'visible', hierarchical_matrix)])
print('Created demo.xlsx, coverage.xlsx, resource-limit.xlsx, wide.xlsx, matrix.xlsx')
