#!/usr/bin/env python3
"""Deterministic appearance fixture (development only; requires openpyxl)."""
from pathlib import Path
from xml.sax.saxutils import escape, quoteattr
from zipfile import ZipFile, ZipInfo, ZIP_DEFLATED
from io import BytesIO
import xml.etree.ElementTree as ET
import openpyxl
from openpyxl.writer.theme import theme_xml

ROOT = Path(__file__).resolve().parents[1]
NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'
REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'

def c(address, value=None, style=None, formula=None):
    attrs = f'r="{address}"' + (f' s="{style}"' if style is not None else '')
    if formula:
        return f'<c {attrs}><f>{escape(formula)}</f><v>{value}</v></c>'
    if value is None:
        return f'<c {attrs}/>'
    if isinstance(value, (float, int)):
        return f'<c {attrs}><v>{value}</v></c>'
    return f'<c {attrs} t="inlineStr"><is><t xml:space="preserve">{escape(value)}</t></is></c>'

def row(index, cells, height=24, extra=''):
    return f'<row r="{index}" ht="{height}" customHeight="1" {extra}>{"".join(cells)}</row>'

def xf(font=0, fill=0, border=1, num=0, align='vertical="center"', extra=''):
    return f'<xf fontId="{font}" fillId="{fill}" borderId="{border}" numFmtId="{num}" xfId="0" {extra}><alignment {align}/></xf>'

fonts = '''<font><sz val="11"/><name val="Arial"/><color rgb="FF24344A"/></font>
<font><b/><sz val="22"/><name val="Arial"/><color rgb="FFFFFFFF"/></font>
<font><b/><sz val="11"/><name val="Apple SD Gothic Neo"/><color rgb="FFFFFFFF"/></font>
<font><sz val="11"/><name val="Apple SD Gothic Neo"/><color rgb="FF24344A"/></font>
<font><i/><sz val="10"/><name val="Arial"/><color rgb="FF64748B"/></font>
<font><b/><sz val="12"/><name val="Arial"/><color theme="4" tint="-0.25"/></font>
<font><i/><u val="double"/><strike/><sz val="12"/><name val="Arial"/><color rgb="FF7C3AED"/></font>'''
fills = '''<fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
<fill><patternFill patternType="solid"><fgColor rgb="FF18344B"/></patternFill></fill>
<fill><patternFill patternType="solid"><fgColor theme="4"/></patternFill></fill>
<fill><patternFill patternType="solid"><fgColor theme="4" tint="0.85"/></patternFill></fill>
<fill><patternFill patternType="solid"><fgColor rgb="FFFFF3CF"/></patternFill></fill>
<fill><patternFill patternType="lightTrellis"><fgColor rgb="FFCBD5E1"/><bgColor rgb="FFF8FAFC"/></patternFill></fill>'''
thin = '<border>' + ''.join(f'<{side} style="thin"><color rgb="FFCBD5E1"/></{side}>' for side in ['left','right','top','bottom']) + '<diagonal/></border>'
borders = '<border><left/><right/><top/><bottom/><diagonal/></border>' + thin + '''<border><left/><right/><top style="thin"><color rgb="FF10B981"/></top><bottom style="double"><color rgb="FF10B981"/></bottom><diagonal/></border>
<border diagonalUp="1"><left style="medium"><color rgb="FF7C3AED"/></left><right style="dashed"><color rgb="FF7C3AED"/></right><top style="dotted"><color rgb="FF7C3AED"/></top><bottom style="dashDot"><color rgb="FF7C3AED"/></bottom><diagonal style="thin"><color rgb="FF7C3AED"/></diagonal></border>'''
formats = [(164, '#,##0.00'), (165, 'yyyy-mm-dd'), (166, '"₩" #,##0;[Red]("₩" #,##0);"—"'), (167, '0.0%'), (168, 'hh:mm AM/PM')]
xfs = [
    xf(border=0),                                      # 0 normal
    xf(font=1, fill=2, border=0, align='vertical="center"'), # 1 title
    xf(font=4, border=0),                              # 2 subtitle
    xf(font=2, fill=3, align='horizontal="center" vertical="center"'), # 3 headers
    xf(font=3),                                       # 4 text
    xf(num=164, align='horizontal="right" vertical="center"'), # 5 money
    xf(num=167, fill=4, align='horizontal="right" vertical="center"'), # 6 percent
    xf(num=165, align='horizontal="center" vertical="center"'), # 7 date
    xf(font=5, fill=4, border=2, num=166, align='horizontal="right" vertical="center"'), # 8 total
    xf(font=3, fill=5, align='wrapText="1" vertical="center" indent="1"'), # 9 wrap
    xf(font=5, border=3, align='horizontal="center" vertical="center" textRotation="45"'), # 10 rotation
    xf(font=6, align='horizontal="center" vertical="center"'), # 11 decorations
    xf(num=166),                                      # 12 currency
    xf(fill=4),                                       # 13 blank fill
    xf(fill=6),                                       # 14 pattern
    xf(align='shrinkToFit="1" horizontal="center" vertical="center"'), # 15 shrink
    xf(num=168),                                      # 16 time
]
styles = f'<styleSheet xmlns="{NS}"><numFmts count="{len(formats)}">' + ''.join(f'<numFmt numFmtId="{i}" formatCode={quoteattr(code)}/>' for i, code in formats) + f'</numFmts><fonts count="7">{fonts}</fonts><fills count="7">{fills}</fills><borders count="4">{borders}</borders><cellStyleXfs count="1">{xf(border=0)}</cellStyleXfs><cellXfs count="{len(xfs)}">{"".join(xfs)}</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>'

rows = [
    row(1, [c('A1','QUARTERLY SALES',1)] + [c(f'{x}1',style=1) for x in 'BCDEFGH'], 28),
    row(2, [c(f'{x}2',style=1) for x in 'ABCDEFGH'], 22),
    row(3, [c('A3','Original workbook appearance · Q4 2026',2)], 23),
    row(5, [c(f'{x}5',text,3) for x,text in zip('ABCDEFGH',['코드','제품명','지역','수량','단가','할인율','판매일','합계'])], 30),
]
data = [('SW-101','기계식 키보드','서울',12,129000,0.05,46328), ('SW-102','무선 마우스','부산',24,59000,0.10,46329), ('SW-103','모니터 스탠드','대전',8,79000,0,46330), ('SW-104','USB-C 허브','서울',15,49000,0.075,46331)]
for r, values in enumerate(data,6):
    code,name,city,qty,price,discount,date = values
    rows.append(row(r,[c(f'A{r}',code,0),c(f'B{r}',name,4),c(f'C{r}',city,4),c(f'D{r}',qty,0),c(f'E{r}',price,5),c(f'F{r}',discount,6),c(f'G{r}',date,7),c(f'H{r}',qty*price*(1-discount),12,formula=f'D{r}*E{r}*(1-F{r})')],26))
rows += [row(10,[c('G10','TOTAL',8),c('H10',4056875,8,formula='SUM(H6:H9)')],32),
    row(12,[c('A12','병합 셀과 줄 바꿈을 유지합니다.\n원본의 배경색, 테두리, 글꼴과 숫자 표시 형식을 확인하세요.',9)],42),
    row(13,[c('A13',style=9)],22),
    row(15,[c('A15','Decorations',11),c('C15','Rotated',10),c('E15',style=14),c('F15','Long text shrinks to fit',15),c('G15',0.625,16),c('H15',-125000,12)],65),
    row(16,[c('A16','Hidden source row',4)],24,'hidden="1"'),
    row(18,[c('A18','Blank styled cells →',2),c('E18',style=13),c('F18',style=13),c('G18',style=13),c('H18',style=13)],30),
    row(21,[c('A21','Inherited row style'),c('B21','서식이 행에서 상속됩니다')],26,'s="13" customFormat="1"'),
    row(26,[c('A26','Scroll down: source cells remain accessible.',2)],24),
]
widths=[12,24,12,8,17,12,17,20]
cols=''.join(f'<col min="{i}" max="{i}" width="{w}" customWidth="1"/>' for i,w in enumerate(widths,1)) + '<col min="9" max="9" hidden="1" width="12" customWidth="1"/>'
merges=['A1:H2','A3:H3','A12:H13','A15:B15','A18:D18','A26:H26']
worksheet=f'<worksheet xmlns="{NS}"><sheetViews><sheetView workbookViewId="0" showGridLines="0" zoomScale="90"><pane xSplit="1" ySplit="5" topLeftCell="B6" activePane="bottomRight" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="18" defaultColWidth="10"/><cols>{cols}</cols><sheetData>{"".join(rows)}</sheetData><mergeCells count="{len(merges)}">'+''.join(f'<mergeCell ref="{m}"/>' for m in merges)+'</mergeCells></worksheet>'
theme='''<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="StructureWeaver"><a:themeElements><a:clrScheme name="Workbook"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="18344B"/></a:dk2><a:lt2><a:srgbClr val="F8FAFC"/></a:lt2><a:accent1><a:srgbClr val="10B981"/></a:accent1><a:accent2><a:srgbClr val="7C3AED"/></a:accent2><a:accent3><a:srgbClr val="F59E0B"/></a:accent3><a:accent4><a:srgbClr val="3B82F6"/></a:accent4><a:accent5><a:srgbClr val="EF4444"/></a:accent5><a:accent6><a:srgbClr val="64748B"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Workbook"><a:majorFont><a:latin typeface="Arial"/><a:ea typeface="Apple SD Gothic Neo"/><a:cs typeface="Arial"/></a:majorFont><a:minorFont><a:latin typeface="Arial"/><a:ea typeface="Apple SD Gothic Neo"/><a:cs typeface="Arial"/></a:minorFont></a:fontScheme><a:fmtScheme name="Workbook"><a:fillStyleLst/><a:lnStyleLst/><a:effectStyleLst/><a:bgFillStyleLst/></a:fmtScheme></a:themeElements></a:theme>'''
parts={
    '_rels/.rels':f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
    'xl/workbook.xml':f'<workbook xmlns="{NS}" xmlns:r="{REL}"><workbookPr date1904="0"/><sheets><sheet name="Sales report" sheetId="1" r:id="s1"/></sheets></workbook>',
    'xl/_rels/workbook.xml.rels':f'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="s1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="st" Type="{REL}/styles" Target="styles.xml"/><Relationship Id="th" Type="{REL}/theme" Target="theme/theme1.xml"/></Relationships>',
    'xl/styles.xml':styles, 'xl/worksheets/sheet1.xml':worksheet, 'xl/theme/theme1.xml':theme,
}
contenttypes={'/xl/workbook.xml':'sheet.main','/xl/worksheets/sheet1.xml':'worksheet','/xl/styles.xml':'styles'}
parts['[Content_Types].xml']='<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'+''.join(f'<Override PartName="{path}" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.{kind}+xml"/>' for path,kind in contenttypes.items())+'<Override PartName="/xl/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>'
out=ROOT/'fixtures/styled.xlsx'
with ZipFile(out,'w') as archive:
    for name,text in parts.items():
        info=ZipInfo(name,(2026,1,1,0,0,0));info.compress_type=ZIP_DEFLATED
        archive.writestr(info,'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'+text)
# Normalize package/style records with a standards-based writer and a complete
# DrawingML theme. Restore our deterministic formula caches after serialization.
book = openpyxl.load_workbook(out)
theme_root = ET.fromstring(theme_xml)
drawing_ns = {'a': 'http://schemas.openxmlformats.org/drawingml/2006/main'}
for tag, color in [('accent1', '10B981'), ('accent2', '7C3AED')]:
    theme_root.find(f'.//a:{tag}/a:srgbClr', drawing_ns).set('val', color)
for tag in ['majorFont', 'minorFont']:
    theme_root.find(f'.//a:{tag}/a:latin', drawing_ns).set('typeface', 'Arial')
book.loaded_theme = ET.tostring(theme_root, encoding='utf-8')
buffer = BytesIO()
book.save(buffer)
with ZipFile(buffer) as normalized:
    normalized_parts = {name: normalized.read(name) for name in normalized.namelist()}
sheet_root = ET.fromstring(normalized_parts['xl/worksheets/sheet1.xml'])
for element in sheet_root.iter(f'{{{NS}}}c'):
    if element.find(f'{{{NS}}}f') is not None:
        address = element.attrib['r']
        original = ET.fromstring(worksheet).find(f'.//{{{NS}}}c[@r="{address}"]/{{{NS}}}v')
        value = element.find(f'{{{NS}}}v')
        if value is None:
            value = ET.SubElement(element, f'{{{NS}}}v')
        value.text = original.text
normalized_parts['xl/worksheets/sheet1.xml'] = ET.tostring(sheet_root, encoding='utf-8')
with ZipFile(out, 'w') as archive:
    for name, data in normalized_parts.items():
        info = ZipInfo(name, (2026, 1, 1, 0, 0, 0))
        info.compress_type = ZIP_DEFLATED
        archive.writestr(info, data)
print(out)
